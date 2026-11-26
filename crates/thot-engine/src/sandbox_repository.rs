//! Portable, single-writer folder storage for sandbox runs.
//!
//! A run is visible as committed only after its immutable revision directory is
//! durable and the `run.json` pointer has been atomically replaced. The journal
//! is hash chained for corruption detection; it is not an authenticity proof.

use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
};
use thot_core::sandbox_contracts::{
    ArtifactReference, DeliveryProvenance, RunJournalEvent, RunRevision, RunSnapshot, RunStatus,
    validate_sha256,
};
use zip::{ZipArchive, ZipWriter, write::FileOptions};

const RUN_SCHEMA: &str = "thot.run/v1";
const MAX_RUN_ARCHIVE_ENTRIES: usize = 100_000;
const MAX_RUN_ARCHIVE_BYTES: u64 = 128 * 1024 * 1024 * 1024;

pub struct ProjectRunRepository {
    run_dir: PathBuf,
    _lock: File,
    snapshot: RunSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunVerification {
    pub run_id: String,
    pub status: RunStatus,
    pub revision_id: Option<String>,
    pub verified_files: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InputRecord {
    student_unit_id: String,
    relative_path: String,
    provenance: DeliveryProvenance,
}

impl ProjectRunRepository {
    /// Start an immutable run snapshot while holding the project's writer lock.
    /// `snapshot_files` keys are safe project-relative names; credential caches
    /// and temporary guest bootstrap material must never be included.
    pub fn begin(
        project_root: &Path,
        run_id: &str,
        project_id: &str,
        snapshot_files: &[(String, Vec<u8>)],
    ) -> Result<Self, String> {
        validate_component(run_id)?;
        if project_id.trim().is_empty() || snapshot_files.is_empty() {
            return Err("run snapshot requires project identity and files".into());
        }
        let project_root = project_root
            .canonicalize()
            .map_err(|error| format!("cannot resolve project directory: {error}"))?;
        let results_root = project_root.join("results");
        fs::create_dir_all(&results_root)
            .map_err(|error| format!("cannot create results directory: {error}"))?;
        let mut config_digest = None;
        let mut seen = std::collections::HashSet::new();
        let mut total_snapshot_bytes = 0u64;
        for (relative, bytes) in snapshot_files {
            let relative_path = safe_relative_path(relative)?;
            if !seen.insert(relative_path.clone()) {
                return Err(format!("duplicate run snapshot path `{relative}`"));
            }
            total_snapshot_bytes = total_snapshot_bytes.saturating_add(bytes.len() as u64);
            if bytes.len() > 4 * 1024 * 1024 || total_snapshot_bytes > 16 * 1024 * 1024 {
                return Err("run metadata snapshot exceeds its bounded size policy".into());
            }
            if relative_path == Path::new("configuration.thot.yaml") {
                config_digest = Some(sha256(bytes));
            }
        }
        let configuration_sha256 =
            config_digest.ok_or("run snapshot does not contain configuration.thot.yaml")?;
        let lock_path = results_root.join(".project-writer.lock");
        let lock = open_lock_file(&lock_path)?;
        acquire_lock(&lock)?;
        recover_incomplete_runs(&results_root)?;

        let run_dir = results_root.join(run_id);
        fs::create_dir(&run_dir)
            .map_err(|error| format!("cannot create unique run directory: {error}"))?;
        let snapshot_dir = run_dir.join("snapshot");
        fs::create_dir(&snapshot_dir)
            .map_err(|error| format!("cannot create run snapshot: {error}"))?;

        for (relative, bytes) in snapshot_files {
            let relative_path = safe_relative_path(relative)?;
            let target = snapshot_dir.join(relative_path);
            write_new_file(&target, bytes)?;
        }
        let now = timestamp();
        let mut repository = Self {
            run_dir,
            _lock: lock,
            snapshot: RunSnapshot {
                schema_version: RUN_SCHEMA.into(),
                run_id: run_id.into(),
                project_id: project_id.into(),
                created_at: now,
                configuration_sha256,
                status: RunStatus::Created,
                current_revision: None,
            },
        };
        repository.write_snapshot()?;
        repository.append_event(
            "run.created",
            serde_json::json!({"schema_version": RUN_SCHEMA}),
        )?;
        Ok(repository)
    }

    pub fn run_dir(&self) -> &Path {
        &self.run_dir
    }

    pub fn snapshot(&self) -> &RunSnapshot {
        &self.snapshot
    }

    pub fn start(&mut self) -> Result<(), String> {
        if self.snapshot.status != RunStatus::Created {
            return Err("only a newly created run can start".into());
        }
        self.snapshot.status = RunStatus::Running;
        self.write_snapshot()?;
        self.append_event("run.started", serde_json::json!({}))
    }

    /// Persist an acquired delivery immutably and bind its source provenance to
    /// the copied bytes. When source and run storage share a filesystem, a
    /// hardlink avoids a second physical copy of large immutable inputs; a
    /// cross-filesystem or unsupported link falls back to a verified copy. A
    /// changed digest rejects the intake before execution.
    pub fn persist_input(
        &mut self,
        student_unit_id: &str,
        source: &Path,
        provenance: DeliveryProvenance,
    ) -> Result<String, String> {
        validate_component(student_unit_id)?;
        provenance.validate().map_err(|error| error.to_string())?;
        if self.snapshot.status != RunStatus::Running {
            return Err("cannot add inputs to a non-running run".into());
        }
        let student_dir = self.run_dir.join("inputs").join(student_unit_id);
        fs::create_dir_all(&student_dir)
            .map_err(|error| format!("cannot create input directory: {error}"))?;
        let payload_name = safe_filename(&provenance.name);
        let destination = student_dir.join(payload_name);
        if destination.exists() {
            return Err("run input destination already exists".into());
        }
        let temporary = student_dir.join(format!(".input-pending-{}", uuid::Uuid::new_v4()));
        let source_metadata = fs::symlink_metadata(source)
            .map_err(|error| format!("cannot inspect acquired input: {error}"))?;
        if source_metadata.file_type().is_symlink()
            || !(source_metadata.is_file() || source_metadata.is_dir())
        {
            return Err("acquired input must be a regular file or directory tree".into());
        }
        let digest = if source_metadata.is_dir() {
            copy_tree(source, &temporary)?;
            hash_tree(&temporary)?
        } else {
            match fs::hard_link(source, &temporary) {
                Ok(()) => match hash_file(&temporary) {
                    Ok(digest) => digest,
                    Err(error) => {
                        remove_any(&temporary);
                        return Err(format!("cannot verify linked input: {error}"));
                    }
                },
                Err(_) => copy_and_digest(source, &temporary)?,
            }
        };
        if digest != provenance.sha256 {
            remove_any(&temporary);
            return Err("delivery SHA-256 changed while snapshotting the input".into());
        }
        #[cfg(unix)]
        {
            let mode = if source_metadata.is_dir() {
                0o555
            } else {
                0o444
            };
            if let Err(error) = set_tree_permissions(&temporary, mode) {
                remove_any(&temporary);
                return Err(format!("cannot make persisted input immutable: {error}"));
            }
        }
        fs::rename(&temporary, &destination)
            .map_err(|error| format!("cannot publish immutable input: {error}"))?;
        sync_directory(&student_dir)?;
        let relative_path = relative_to_run(&self.run_dir, &destination)?;
        let record = InputRecord {
            student_unit_id: student_unit_id.into(),
            relative_path: relative_path.clone(),
            provenance,
        };
        let record_path = self
            .run_dir
            .join("deliveries")
            .join(format!("{student_unit_id}.json"));
        write_new_file(
            &record_path,
            &serde_json::to_vec_pretty(&record).map_err(json_error)?,
        )?;
        self.append_event(
            "delivery.persisted",
            serde_json::json!({
                "student_unit_id": student_unit_id,
                "relative_path": relative_path,
                "sha256": digest
            }),
        )?;
        Ok(relative_path)
    }

    /// Store a bounded captured stream or declared artifact by content digest.
    pub fn put_artifact(
        &mut self,
        bytes: &[u8],
        media_type: &str,
        complete: bool,
    ) -> Result<ArtifactReference, String> {
        if self.snapshot.status != RunStatus::Running || media_type.trim().is_empty() {
            return Err("artifact requires a running run and media type".into());
        }
        let digest = sha256(bytes);
        let relative_path = format!("artifacts/sha256/{}/{}", &digest[..2], digest);
        let path = self.run_dir.join(&relative_path);
        if path.exists() {
            if hash_file(&path)? != digest {
                return Err("content-addressed artifact has a conflicting digest".into());
            }
        } else {
            write_new_file(&path, bytes)?;
        }
        Ok(ArtifactReference {
            sha256: digest,
            size_bytes: bytes.len() as u64,
            media_type: media_type.into(),
            relative_path,
            complete,
        })
    }

    pub fn append_event(
        &mut self,
        event_type: &str,
        details: serde_json::Value,
    ) -> Result<(), String> {
        if event_type.trim().is_empty() {
            return Err("journal event type must not be empty".into());
        }
        let journal = self.run_dir.join("journal").join("events.jsonl");
        fs::create_dir_all(journal.parent().expect("journal path has parent"))
            .map_err(|error| format!("cannot create journal directory: {error}"))?;
        let (sequence, previous_sha256) = last_event(&journal)?;
        let mut event = RunJournalEvent {
            sequence: sequence + 1,
            event_id: uuid::Uuid::new_v4().to_string(),
            occurred_at: timestamp(),
            event_type: event_type.into(),
            previous_sha256,
            details,
            event_sha256: String::new(),
        };
        event.event_sha256 = event_hash(&event)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&journal)
            .map_err(|error| format!("cannot open run journal: {error}"))?;
        serde_json::to_writer(&mut file, &event).map_err(json_error)?;
        file.write_all(b"\n")
            .and_then(|()| file.sync_all())
            .map_err(|error| format!("cannot sync run journal: {error}"))
    }

    /// Commit a revision only after all result and artifact records are durable.
    pub fn commit_revision(
        &mut self,
        operation: &str,
        result_paths: Vec<String>,
    ) -> Result<RunRevision, String> {
        if self.snapshot.status != RunStatus::Running || operation.trim().is_empty() {
            return Err("only a running run can commit a revision".into());
        }
        for path in &result_paths {
            safe_relative_path(path)?;
            if !self.run_dir.join(path).is_file() {
                return Err(format!("revision result does not exist: {path}"));
            }
        }
        let evidence_manifest_sha256 = self.evidence_manifest_digest()?;
        let revision = RunRevision {
            revision_id: uuid::Uuid::new_v4().to_string(),
            parent_revision: self.snapshot.current_revision.clone(),
            created_at: timestamp(),
            operation: operation.into(),
            result_paths,
            configuration_sha256: self.snapshot.configuration_sha256.clone(),
            evidence_manifest_sha256,
        };
        let revisions_dir = self.run_dir.join("revisions");
        fs::create_dir_all(&revisions_dir)
            .map_err(|error| format!("cannot create revisions directory: {error}"))?;
        let staging = revisions_dir.join(format!(".pending-{}", revision.revision_id));
        fs::create_dir(&staging).map_err(|error| format!("cannot stage revision: {error}"))?;
        write_new_file(
            &staging.join("revision.json"),
            &serde_json::to_vec_pretty(&revision).map_err(json_error)?,
        )?;
        sync_directory(&staging)?;
        fs::rename(&staging, revisions_dir.join(&revision.revision_id))
            .map_err(|error| format!("cannot publish revision: {error}"))?;
        sync_directory(&revisions_dir)?;

        self.snapshot.current_revision = Some(revision.revision_id.clone());
        self.snapshot.status = RunStatus::Complete;
        self.write_snapshot()?;
        self.append_event(
            "revision.committed",
            serde_json::json!({"revision_id": revision.revision_id}),
        )?;
        Self::verify_run(&self.run_dir)?;
        self.restore_invoking_user_ownership()?;
        Ok(revision)
    }

    /// Validate the committed folder package before export or re-import.
    pub fn verify_run(run_dir: &Path) -> Result<RunVerification, String> {
        let metadata = fs::symlink_metadata(run_dir).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err("run folder must be a real directory".into());
        }
        let bytes = read_bounded_file(&run_dir.join("run.json"), 4 * 1024 * 1024)?;
        let snapshot: RunSnapshot = serde_json::from_slice(&bytes)
            .map_err(|error| format!("invalid run manifest: {error}"))?;
        if snapshot.schema_version != RUN_SCHEMA {
            return Err(format!(
                "unsupported run schema `{}`",
                snapshot.schema_version
            ));
        }
        validate_component(&snapshot.run_id)?;
        validate_sha256(&snapshot.configuration_sha256).map_err(|error| error.to_string())?;
        let config_path = run_dir.join("snapshot/configuration.thot.yaml");
        if hash_file(&config_path)? != snapshot.configuration_sha256 {
            return Err("configuration snapshot digest does not match run manifest".into());
        }
        let journal_path = run_dir.join("journal/events.jsonl");
        verify_journal_file(&journal_path)?;

        let mut verified_files = 0;
        if let Some(revision_id) = &snapshot.current_revision {
            validate_component(revision_id)?;
            let revision_path = run_dir
                .join("revisions")
                .join(revision_id)
                .join("revision.json");
            let revision_meta =
                fs::symlink_metadata(&revision_path).map_err(|error| error.to_string())?;
            if revision_meta.file_type().is_symlink() || !revision_meta.is_file() {
                return Err("committed revision must be a regular file".into());
            }
            let bytes = read_bounded_file(&revision_path, 4 * 1024 * 1024)?;
            let revision: RunRevision = serde_json::from_slice(&bytes)
                .map_err(|error| format!("invalid committed revision: {error}"))?;
            if revision.revision_id != *revision_id
                || revision.configuration_sha256 != snapshot.configuration_sha256
                || evidence_digest(run_dir)? != revision.evidence_manifest_sha256
            {
                return Err("committed revision integrity check failed".into());
            }
            for relative in &revision.result_paths {
                let path = run_dir.join(safe_relative_path(relative)?);
                if !path.is_file() {
                    return Err(format!("committed result is missing: {relative}"));
                }
                verified_files += 1;
            }
        } else if snapshot.status == RunStatus::Complete {
            return Err("complete run has no committed revision".into());
        }
        Ok(RunVerification {
            run_id: snapshot.run_id,
            status: snapshot.status,
            revision_id: snapshot.current_revision,
            verified_files,
        })
    }

    pub fn mark_failed(&mut self, reason_code: &str) -> Result<(), String> {
        self.snapshot.status = RunStatus::Failed;
        self.write_snapshot()?;
        self.append_event(
            "run.failed",
            serde_json::json!({"reason_code": reason_code}),
        )
    }

    pub fn verify_journal(&self) -> Result<(), String> {
        let path = self.run_dir.join("journal").join("events.jsonl");
        verify_journal_file(&path)
    }

    fn restore_invoking_user_ownership(&self) -> Result<(), String> {
        let (uid, gid) = match (std::env::var("SUDO_UID"), std::env::var("SUDO_GID")) {
            (Ok(uid), Ok(gid)) => (uid.parse::<u32>(), gid.parse::<u32>()),
            _ => return Ok(()),
        };
        let uid = uid.map_err(|error| error.to_string())?;
        let gid = gid.map_err(|error| error.to_string())?;
        let results_root = self
            .run_dir
            .parent()
            .ok_or("run directory has no results parent")?;
        let lock_path = results_root.join(".project-writer.lock");
        set_private_permissions(&lock_path)?;
        chown_path(&lock_path, uid, gid)?;
        set_private_tree_permissions(&self.run_dir)?;
        chown_tree(&self.run_dir, uid, gid)?;
        set_private_directory_permissions(results_root)?;
        chown_path(results_root, uid, gid)
    }

    fn write_snapshot(&self) -> Result<(), String> {
        atomic_write(
            &self.run_dir.join("run.json"),
            &serde_json::to_vec_pretty(&self.snapshot).map_err(json_error)?,
        )
    }

    fn evidence_manifest_digest(&self) -> Result<String, String> {
        evidence_digest(&self.run_dir)
    }
}

/// Export a verified completed run as an uncompressed ZIP. Student deliveries
/// are often already compressed disk images, so recompressing them wastes CPU.
pub fn export_run_archive(run_dir: &Path, destination: &Path) -> Result<(), String> {
    let verification = ProjectRunRepository::verify_run(run_dir)?;
    if verification.status != RunStatus::Complete {
        return Err("only a completed verified run can be archived".into());
    }
    let root = run_dir.canonicalize().map_err(|error| error.to_string())?;
    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let parent = parent.canonicalize().map_err(|error| error.to_string())?;
    if parent.starts_with(&root) {
        return Err("run archive destination must be outside the run folder".into());
    }
    let output = parent.join(
        destination
            .file_name()
            .ok_or("archive path has no filename")?,
    );
    if fs::symlink_metadata(&output).is_ok() {
        return Err("run archive destination already exists".into());
    }
    let temp = output.with_extension(format!("zip-pending-{}", uuid::Uuid::new_v4()));
    let result = (|| {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|error| format!("cannot create run archive: {error}"))?;
        let mut writer = ZipWriter::new(file);
        let mut count = 0usize;
        let mut total = 0u64;
        add_run_directory(&root, &root, &mut writer, &mut count, &mut total)?;
        let file = writer
            .finish()
            .map_err(|error| format!("cannot finish run archive: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("cannot sync run archive: {error}"))?;
        fs::rename(&temp, &output)
            .map_err(|error| format!("cannot publish run archive: {error}"))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn add_run_directory(
    root: &Path,
    directory: &Path,
    writer: &mut ZipWriter<File>,
    count: &mut usize,
    total: &mut u64,
) -> Result<(), String> {
    let mut entries = fs::read_dir(directory)
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() {
            return Err(format!("run archive refuses symlinks: {}", path.display()));
        }
        if metadata.is_dir() {
            add_run_directory(root, &path, writer, count, total)?;
            continue;
        }
        if !metadata.is_file() {
            return Err(format!(
                "run archive refuses special files: {}",
                path.display()
            ));
        }
        *count += 1;
        *total = total.saturating_add(metadata.len());
        if *count > MAX_RUN_ARCHIVE_ENTRIES || *total > MAX_RUN_ARCHIVE_BYTES {
            return Err("run exceeds archive entry or uncompressed-size limits".into());
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| "run file escapes archive root")?;
        let name = relative
            .to_str()
            .ok_or("run archive path is not UTF-8")?
            .replace('\\', "/");
        writer
            .start_file(
                name,
                FileOptions::default().compression_method(zip::CompressionMethod::Stored),
            )
            .map_err(|error| format!("cannot add run file to archive: {error}"))?;
        let mut source = File::open(path).map_err(|error| error.to_string())?;
        io::copy(&mut source, writer)
            .map_err(|error| format!("cannot copy run file into archive: {error}"))?;
    }
    Ok(())
}

/// Import a run ZIP into a new directory, rejecting traversal, links, duplicate
/// members and oversized expansions before accepting its integrity manifest.
pub fn import_run_archive(
    archive_path: &Path,
    destination: &Path,
) -> Result<RunVerification, String> {
    let archive_meta = fs::symlink_metadata(archive_path).map_err(|error| error.to_string())?;
    if archive_meta.file_type().is_symlink()
        || !archive_meta.is_file()
        || archive_meta.len() > MAX_RUN_ARCHIVE_BYTES
    {
        return Err("run archive must be a regular file within the 128 GiB limit".into());
    }
    if destination.exists() {
        return Err("run import destination already exists".into());
    }
    fs::create_dir_all(destination).map_err(|error| error.to_string())?;
    let result = (|| {
        let mut archive =
            ZipArchive::new(File::open(archive_path).map_err(|error| error.to_string())?)
                .map_err(|error| format!("invalid run ZIP archive: {error}"))?;
        if archive.len() > MAX_RUN_ARCHIVE_ENTRIES {
            return Err("run archive has too many entries".into());
        }
        let root = destination
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let mut seen = std::collections::HashSet::new();
        let mut total = 0u64;
        for index in 0..archive.len() {
            let mut entry = archive
                .by_index(index)
                .map_err(|error| format!("invalid run ZIP entry: {error}"))?;
            if entry.compression() != zip::CompressionMethod::Stored
                || entry.compressed_size() != entry.size()
            {
                return Err("run archive members must use stored, uncompressed ZIP entries".into());
            }
            if entry.name().contains('\\') || entry.name().contains(':') {
                return Err("run archive path contains a platform-specific separator".into());
            }
            let relative = safe_relative_path(entry.name())?;
            if !seen.insert(relative.clone()) {
                return Err(format!(
                    "duplicate run archive path `{}`",
                    relative.display()
                ));
            }
            if entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
            {
                return Err("run archive symlinks are not allowed".into());
            }
            total = total.saturating_add(entry.size());
            if total > MAX_RUN_ARCHIVE_BYTES {
                return Err("run archive expands beyond 128 GiB".into());
            }
            let output = root.join(&relative);
            if !output.starts_with(&root) {
                return Err("run archive path escapes destination".into());
            }
            if entry.is_dir() {
                fs::create_dir_all(&output).map_err(|error| error.to_string())?;
                set_private_directory(&output)?;
                continue;
            }
            let parent = output.parent().ok_or("run archive member has no parent")?;
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            set_private_tree_directories(&root, parent)?;
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&output)
                .map_err(|error| format!("cannot create imported run file: {error}"))?;
            let copied = io::copy(&mut entry, &mut file)
                .map_err(|error| format!("cannot extract run archive member: {error}"))?;
            if copied != entry.size() {
                return Err("run archive member size does not match its directory".into());
            }
            file.sync_all().map_err(|error| error.to_string())?;
            set_private_file(&output)?;
        }
        set_private_directory(&root)?;
        ProjectRunRepository::verify_run(&root)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(destination);
    }
    result
}

#[cfg(unix)]
fn set_private_directory(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|error| error.to_string())
}

#[cfg(not(unix))]
fn set_private_directory(_path: &Path) -> Result<(), String> {
    Ok(())
}

fn set_private_tree_directories(root: &Path, directory: &Path) -> Result<(), String> {
    let mut path = directory;
    while path.starts_with(root) {
        set_private_directory(path)?;
        if path == root {
            break;
        }
        path = path
            .parent()
            .ok_or("imported run directory lost its root")?;
    }
    Ok(())
}

#[cfg(unix)]
fn set_private_file(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| error.to_string())
}

#[cfg(not(unix))]
fn set_private_file(_path: &Path) -> Result<(), String> {
    Ok(())
}

impl Drop for ProjectRunRepository {
    fn drop(&mut self) {
        // Preserve professor access when an early error exits the run. The
        // successful commit path reports ownership errors explicitly above.
        let _ = self.restore_invoking_user_ownership();
    }
}

fn chown_tree(path: &Path, uid: u32, gid: u32) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        for entry in fs::read_dir(path).map_err(|error| error.to_string())? {
            chown_tree(&entry.map_err(|error| error.to_string())?.path(), uid, gid)?;
        }
    }
    chown_path(path, uid, gid)
}

#[cfg(unix)]
fn set_private_tree_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err("run package contains a symlink while applying private permissions".into());
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path).map_err(|error| error.to_string())? {
            set_private_tree_permissions(&entry.map_err(|error| error.to_string())?.path())?;
        }
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
    } else if metadata.is_file() {
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|error| error.to_string())?;
    } else {
        return Err("run package contains a special file".into());
    }
    Ok(())
}

#[cfg(not(unix))]
fn set_private_tree_permissions(_path: &Path) -> Result<(), String> {
    Err("private run-folder permissions require Unix".into())
}

#[cfg(unix)]
fn set_private_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| error.to_string())
}

#[cfg(not(unix))]
fn set_private_permissions(_path: &Path) -> Result<(), String> {
    Err("private run-folder permissions require Unix".into())
}

#[cfg(unix)]
fn set_private_directory_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|error| error.to_string())
}

#[cfg(not(unix))]
fn set_private_directory_permissions(_path: &Path) -> Result<(), String> {
    Err("private run-folder permissions require Unix".into())
}

#[cfg(unix)]
fn chown_path(path: &Path, uid: u32, gid: u32) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt;
    let path =
        std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|error| error.to_string())?;
    if unsafe { libc::lchown(path.as_ptr(), uid, gid) } != 0 {
        return Err(format!(
            "cannot return run ownership to invoking user: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn chown_path(_path: &Path, _uid: u32, _gid: u32) -> Result<(), String> {
    Err("invoking-user ownership restoration requires Unix".into())
}

fn evidence_digest(run_dir: &Path) -> Result<String, String> {
    let mut files = Vec::new();
    for entry in fs::read_dir(run_dir).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if entry.file_name() == "run.json" || entry.file_name() == "journal" {
            continue;
        }
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() {
            return Err("run evidence contains a symlink".into());
        }
        if metadata.is_file() {
            files.push((relative_to_run(run_dir, &path)?, hash_file(&path)?));
        } else if metadata.is_dir() && entry.file_name() != "revisions" {
            collect_file_digests(run_dir, &path, &mut files)?;
        }
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let bytes = serde_json::to_vec(&files).map_err(json_error)?;
    Ok(sha256(&bytes))
}

fn recover_incomplete_runs(results_root: &Path) -> Result<(), String> {
    for entry in fs::read_dir(results_root).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        if !entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_dir()
        {
            continue;
        }
        let run_path = entry.path().join("run.json");
        let bytes = match fs::read(&run_path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("cannot read prior run manifest: {error}")),
        };
        let mut snapshot: RunSnapshot = serde_json::from_slice(&bytes)
            .map_err(|error| format!("invalid prior run manifest: {error}"))?;
        if matches!(snapshot.status, RunStatus::Created | RunStatus::Running) {
            snapshot.status = RunStatus::Interrupted;
            atomic_write(
                &run_path,
                &serde_json::to_vec_pretty(&snapshot).map_err(json_error)?,
            )?;
            append_event_file(
                &entry.path().join("journal/events.jsonl"),
                "run.recovered_interrupted",
                serde_json::json!({"previous_status": "running_or_created"}),
            )?;
        }
    }
    Ok(())
}

fn append_event_file(
    path: &Path,
    event_type: &str,
    details: serde_json::Value,
) -> Result<(), String> {
    fs::create_dir_all(path.parent().ok_or("journal path has no parent")?)
        .map_err(|error| error.to_string())?;
    let (sequence, previous_sha256) = last_event(path)?;
    let mut event = RunJournalEvent {
        sequence: sequence + 1,
        event_id: uuid::Uuid::new_v4().to_string(),
        occurred_at: timestamp(),
        event_type: event_type.into(),
        previous_sha256,
        details,
        event_sha256: String::new(),
    };
    event.event_sha256 = event_hash(&event)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    serde_json::to_writer(&mut file, &event).map_err(json_error)?;
    file.write_all(b"\n")
        .and_then(|()| file.sync_all())
        .map_err(|error| error.to_string())
}

fn last_event(path: &Path) -> Result<(u64, String), String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok((0, "".into())),
        Err(error) => return Err(format!("cannot read journal: {error}")),
    };
    let Some(last) = text.lines().last() else {
        return Ok((0, "".into()));
    };
    let event: RunJournalEvent = serde_json::from_str(last)
        .map_err(|error| format!("invalid last journal event: {error}"))?;
    Ok((event.sequence, event.event_sha256))
}

fn verify_journal_file(path: &Path) -> Result<(), String> {
    let text = String::from_utf8(read_bounded_file(path, 64 * 1024 * 1024)?)
        .map_err(|error| format!("run journal is not UTF-8: {error}"))?;
    let mut previous = String::new();
    let mut expected_sequence = 1;
    for line in text.lines() {
        let event: RunJournalEvent = serde_json::from_str(line)
            .map_err(|error| format!("invalid journal event: {error}"))?;
        if event.sequence != expected_sequence || event.previous_sha256 != previous {
            return Err("run journal sequence or hash chain is inconsistent".into());
        }
        if event_hash(&event)? != event.event_sha256 {
            return Err("run journal event digest does not match".into());
        }
        previous = event.event_sha256;
        expected_sequence += 1;
    }
    Ok(())
}

fn event_hash(event: &RunJournalEvent) -> Result<String, String> {
    let bytes = serde_json::to_vec(&(
        event.sequence,
        &event.event_id,
        &event.occurred_at,
        &event.event_type,
        &event.previous_sha256,
        &event.details,
    ))
    .map_err(json_error)?;
    Ok(sha256(&bytes))
}

fn copy_and_digest(source: &Path, destination: &Path) -> Result<String, String> {
    let metadata = fs::symlink_metadata(source)
        .map_err(|error| format!("cannot inspect acquired input: {error}"))?;
    if metadata.file_type().is_symlink() {
        return Err("acquired input cannot be a symlink".into());
    }
    let mut digest = Sha256::new();
    if metadata.is_file() {
        copy_file(source, destination, &mut digest)?;
    } else if metadata.is_dir() {
        fs::create_dir(destination).map_err(|error| error.to_string())?;
        copy_directory(source, destination, Path::new(""), &mut digest)?;
    } else {
        return Err("acquired input must be a regular file or directory".into());
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(source).map_err(|e| e.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "refusing symlink in delivery tree: {}",
            source.display()
        ));
    }
    if metadata.is_file() {
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut input = File::open(source).map_err(|e| e.to_string())?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)
            .map_err(|e| e.to_string())?;
        io::copy(&mut input, &mut output).map_err(|e| e.to_string())?;
        output.sync_all().map_err(|e| e.to_string())?;
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err(format!(
            "unsupported file type in delivery tree: {}",
            source.display()
        ));
    }
    fs::create_dir_all(destination).map_err(|e| e.to_string())?;
    let mut entries = fs::read_dir(source)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        copy_tree(&entry.path(), &destination.join(entry.file_name()))?;
    }
    Ok(())
}

fn hash_tree(root: &Path) -> Result<String, String> {
    fn visit(root: &Path, path: &Path, hash: &mut Sha256) -> Result<(), String> {
        let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "refusing symlink in delivery tree: {}",
                path.display()
            ));
        }
        if metadata.is_file() {
            let relative = path.strip_prefix(root).map_err(|e| e.to_string())?;
            hash.update(relative.to_string_lossy().as_bytes());
            hash.update([0]);
            let mut file = File::open(path).map_err(|e| e.to_string())?;
            let mut buffer = [0u8; 32 * 1024];
            loop {
                let read = file.read(&mut buffer).map_err(|e| e.to_string())?;
                if read == 0 {
                    break;
                }
                hash.update(&buffer[..read]);
            }
            return Ok(());
        }
        if !metadata.is_dir() {
            return Err(format!(
                "unsupported file type in delivery tree: {}",
                path.display()
            ));
        }
        let mut entries = fs::read_dir(path)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            visit(root, &entry.path(), hash)?;
        }
        Ok(())
    }
    let mut hash = Sha256::new();
    visit(root, root, &mut hash)?;
    Ok(format!("{:x}", hash.finalize()))
}

#[cfg(unix)]
fn set_tree_permissions(path: &Path, mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err("refusing symlink in delivery tree".into());
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
            set_tree_permissions(&entry.map_err(|e| e.to_string())?.path(), mode)?;
        }
    }
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|e| e.to_string())
}

#[cfg(not(unix))]
fn set_tree_permissions(_path: &Path, _mode: u32) -> Result<(), String> {
    Ok(())
}

fn copy_directory(
    source: &Path,
    destination: &Path,
    relative: &Path,
    digest: &mut Sha256,
) -> Result<(), String> {
    let mut entries = fs::read_dir(source)
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let source_path = entry.path();
        let relative_path = relative.join(entry.file_name());
        let target = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source_path).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() {
            return Err("acquired input contains a symlink".into());
        }
        if metadata.is_dir() {
            fs::create_dir(&target).map_err(|error| error.to_string())?;
            copy_directory(&source_path, &target, &relative_path, digest)?;
        } else if metadata.is_file() {
            digest.update(relative_path.to_string_lossy().as_bytes());
            digest.update([0]);
            copy_file(&source_path, &target, digest)?;
        } else {
            return Err("acquired input contains a special file".into());
        }
    }
    Ok(())
}

fn copy_file(source: &Path, destination: &Path, digest: &mut Sha256) -> Result<(), String> {
    let mut input = open_read_nofollow(source)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| error.to_string())?;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = input.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
        output
            .write_all(&buffer[..count])
            .map_err(|error| error.to_string())?;
    }
    output.sync_all().map_err(|error| error.to_string())
}

fn collect_file_digests(
    root: &Path,
    current: &Path,
    files: &mut Vec<(String, String)>,
) -> Result<(), String> {
    for entry in fs::read_dir(current).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() {
            return Err("run evidence contains a symlink".into());
        }
        if metadata.is_dir() {
            collect_file_digests(root, &path, files)?;
        } else if metadata.is_file() {
            files.push((relative_to_run(root, &path)?, hash_file(&path)?));
        } else {
            return Err("run evidence contains a special file".into());
        }
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<String, String> {
    let mut file = open_read_nofollow(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn open_read_nofollow(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    options
        .open(path)
        .map_err(|error| format!("cannot safely open {}: {error}", path.display()))
}

fn read_bounded_file(path: &Path, maximum: u64) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "{} must be a regular non-symlink file",
            path.display()
        ));
    }
    if metadata.len() > maximum {
        return Err(format!("{} exceeds the read limit", path.display()));
    }
    let file = open_read_nofollow(path)?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > maximum {
        return Err(format!("{} exceeds the read limit", path.display()));
    }
    Ok(bytes)
}

fn open_lock_file(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    options
        .open(path)
        .map_err(|error| format!("cannot open project writer lock: {error}"))
}

fn acquire_lock(file: &File) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if result == 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::WouldBlock {
            return Err("another Thot process is writing this project".into());
        }
        return Err(format!("cannot acquire project writer lock: {error}"));
    }
    #[cfg(not(unix))]
    {
        let _ = file;
        Err("folder project locking is supported only on Unix hosts".into())
    }
}

fn write_new_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("file path has no parent")?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("cannot create {}: {error}", path.display()))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("cannot sync {}: {error}", path.display()))?;
    sync_directory(parent)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("atomic file path has no parent")?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temporary = parent.join(format!(".pending-{}", uuid::Uuid::new_v4()));
    write_new_file(&temporary, bytes)?;
    fs::rename(&temporary, path).map_err(|error| error.to_string())?;
    sync_directory(parent)
}

fn sync_directory(path: &Path) -> Result<(), String> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("cannot sync directory {}: {error}", path.display()))
}

fn safe_relative_path(value: &str) -> Result<PathBuf, String> {
    let path = Path::new(value);
    if value.is_empty() || path.is_absolute() {
        return Err("run path must be a non-empty relative path".into());
    }
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => result.push(part),
            _ => return Err("run path contains an unsafe component".into()),
        }
    }
    Ok(result)
}

fn validate_component(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 120
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        || value == "."
        || value == ".."
    {
        return Err("run path component is invalid".into());
    }
    Ok(())
}

fn safe_filename(value: &str) -> String {
    let name = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .take(120)
        .collect::<String>();
    if name.is_empty() || name == "." || name == ".." {
        "submission".into()
    } else {
        name
    }
}

fn relative_to_run(root: &Path, path: &Path) -> Result<String, String> {
    path.strip_prefix(root)
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .map_err(|_| "run artifact is outside the run folder".into())
}

fn remove_any(path: &Path) {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir()) {
        let _ = fs::remove_dir_all(path);
    } else {
        let _ = fs::remove_file(path);
    }
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true)
}

fn json_error(error: serde_json::Error) -> String {
    error.to_string()
}
