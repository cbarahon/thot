//! Purpose: Detects Tiny Core Linux boot images on disk filesystems and reads their gzipped CPIO root filesystems without extracting archive paths to the host.
use flate2::read::GzDecoder;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    time::Instant,
};
use thot_core::test_results::TestStatus;

use crate::{
    test_execution::{TestInvocation, TestOutcome},
    tests::libguestfs::{
        ResolvedDisk, append_stderr, collect_usage, failed_command, labeled_stderr, parse_csv,
        parse_filesystems_csv, parse_group, parse_passwd, partition_summary, resolve_disk,
        run_filesystems, run_tool, success_outcome,
    },
};

const MAX_INITRD_COMPRESSED_BYTES: u64 = 64 * 1024 * 1024;
const MAX_INITRD_EXPANDED_BYTES: u64 = 256 * 1024 * 1024;
const MAX_CPIO_CANDIDATES: usize = 16;

pub(super) async fn run(invocation: &TestInvocation) -> TestOutcome {
    let disk = match resolve_disk(invocation) {
        Ok(disk) => disk,
        Err(error) => return TestOutcome::error(error),
    };
    let started = Instant::now();
    let filesystems_output = match run_filesystems(invocation, &disk, started).await {
        Ok(output) if output.exit_code == Some(0) && !output.timed_out => output,
        Ok(output) => return failed_command("virt-filesystems", output),
        Err(error) => return TestOutcome::error(error),
    };
    let objects = match parse_filesystems_csv(&filesystems_output.stdout) {
        Ok(objects) => objects,
        Err(error) => {
            return TestOutcome::error(format!("cannot parse virt-filesystems output: {error}"));
        }
    };

    let (candidates, scan_errors, scan_timed_out) =
        find_initrd_candidates(invocation, &disk, &objects, started).await;
    if scan_timed_out {
        return timed_out_outcome(
            "timed out while searching filesystem partitions for a Tiny Core initramfs",
            "Tiny Core filesystem scan exceeded the configured timeout",
            &objects,
        );
    }

    if !scan_errors.is_empty() {
        let reason = format!(
            "could not reliably search all disk filesystems for a Tiny Core initramfs: {}",
            scan_errors.join("; ")
        );
        return TestOutcome {
            status: TestStatus::Error,
            stdout: String::new(),
            stderr: partitions_stderr(&reason, &objects),
            exit_code: Some(1),
            diagnostics: reason,
            observation: json!({}),
            evidence: Vec::new(),
        };
    }

    if candidates.is_empty() {
        let reason = if scan_errors.is_empty() {
            "no Tiny Core boot initramfs was found on the disk filesystems".to_owned()
        } else {
            format!(
                "could not inspect all disk filesystems for a Tiny Core initramfs: {}",
                scan_errors.join("; ")
            )
        };
        return TestOutcome {
            status: TestStatus::Error,
            stdout: String::new(),
            stderr: partitions_stderr(&reason, &objects),
            exit_code: Some(1),
            diagnostics: reason,
            observation: json!({}),
            evidence: Vec::new(),
        };
    }
    if candidates.len() > MAX_CPIO_CANDIDATES {
        let reason = format!(
            "found {} possible Tiny Core initramfs files, exceeding the safe inspection limit of {MAX_CPIO_CANDIDATES}",
            candidates.len()
        );
        return TestOutcome {
            status: TestStatus::Error,
            stdout: String::new(),
            stderr: partitions_stderr(&reason, &objects),
            exit_code: Some(1),
            diagnostics: reason,
            observation: json!({}),
            evidence: Vec::new(),
        };
    }

    let mut detected = Vec::new();
    let mut candidate_errors = Vec::new();
    for candidate in candidates {
        if candidate.size_bytes > MAX_INITRD_COMPRESSED_BYTES {
            candidate_errors.push(format!(
                "{}:{} is {} bytes, over the {} byte inspection limit",
                candidate.device, candidate.path, candidate.size_bytes, MAX_INITRD_COMPRESSED_BYTES
            ));
            continue;
        }
        match inspect_candidate(invocation, &disk, &candidate, started).await {
            Ok(Some(image)) => detected.push((candidate, image)),
            Ok(None) => {}
            Err(error) => {
                if started.elapsed() >= invocation.timeout {
                    return timed_out_outcome(
                        "timed out while reading a Tiny Core boot initramfs",
                        "Tiny Core initramfs inspection exceeded the configured timeout",
                        &objects,
                    );
                }
                candidate_errors.push(format!("{}:{}: {error}", candidate.device, candidate.path))
            }
        }
    }

    if detected.is_empty() {
        let reason = if candidate_errors.is_empty() {
            "candidate initramfs files were found, but none contained Tiny Core Linux markers"
                .to_owned()
        } else {
            format!(
                "no Tiny Core Linux initramfs could be verified: {}",
                candidate_errors.join("; ")
            )
        };
        return TestOutcome {
            status: TestStatus::Error,
            stdout: String::new(),
            stderr: partitions_stderr(&reason, &objects),
            exit_code: Some(1),
            diagnostics: reason,
            observation: json!({}),
            evidence: Vec::new(),
        };
    }
    if detected.len() > 1 {
        let paths = detected
            .iter()
            .map(|(candidate, _)| format!("{}:{}", candidate.device, candidate.path))
            .collect::<Vec<_>>()
            .join(", ");
        let reason = format!(
            "multiple Tiny Core initramfs files were found ({paths}); the active boot image is ambiguous"
        );
        return TestOutcome {
            status: TestStatus::Error,
            stdout: String::new(),
            stderr: partitions_stderr(&reason, &objects),
            exit_code: Some(1),
            diagnostics: reason,
            observation: json!({}),
            evidence: Vec::new(),
        };
    }

    let (candidate, image) = detected.pop().expect("one detected Tiny Core image");
    let (usage, usage_problem, usage_timed_out) = collect_usage(invocation, &disk, started).await;
    let filesystem_usage = usage
        .as_ref()
        .and_then(|rows| rows.iter().find(|row| row["device"] == candidate.device));
    let filesystem = objects
        .iter()
        .find(|object| object["type"] == "filesystem" && object["name"] == candidate.device);
    let partition = objects
        .iter()
        .find(|object| object["type"] == "partition" && object["name"] == candidate.device);
    let mut stderr = labeled_stderr("virt-filesystems", &filesystems_output.stderr);
    for error in scan_errors.iter().chain(candidate_errors.iter()) {
        append_stderr(&mut stderr, error);
    }
    let observation = json!({
        "distribution": "Tiny Core Linux",
        "version": image.version,
        "architecture": image.architecture,
        "disk_path": disk.path,
        "disk_name": disk.path.file_name().and_then(|name| name.to_str()).unwrap_or_default(),
        "disk_format": disk.format,
        "boot_image": {
            "partition": candidate.device,
            "path": candidate.path,
            "compressed_size_bytes": candidate.size_bytes,
            "uncompressed_cpio_size_bytes": image.expanded_size,
        },
        "boot_storage_partition": {
            "device": candidate.device,
            "filesystem_type": filesystem.map(|item| item["filesystem_type"].clone()).unwrap_or(Value::Null),
            "label": filesystem.map(|item| item["label"].clone()).unwrap_or(Value::Null),
            "uuid": filesystem.map(|item| item["uuid"].clone()).unwrap_or(Value::Null),
            "filesystem_size_bytes": filesystem.map(|item| item["size_bytes"].clone()).unwrap_or(Value::Null),
            "partition_size_bytes": partition
                .and_then(|item| item["size_bytes"].as_u64())
                .map_or_else(|| filesystem.map(|item| item["size_bytes"].clone()).unwrap_or(Value::Null), |size| json!(size)),
            "partition_type": partition.map(|item| item["mbr_type"].clone()).unwrap_or(Value::Null),
            "usage": filesystem_usage.cloned().unwrap_or(Value::Null),
        },
        "users": image.users,
        "groups": image.groups,
        "inspection_notes": {
            "users": if image.passwd_file_found { Value::Null } else { json!("/etc/passwd was not present in the base initramfs") },
            "groups": if image.group_file_found { Value::Null } else { json!("/etc/group was not present in the base initramfs") },
            "filesystem_usage": usage_problem,
            "scope": "information comes from the detected base Tiny Core initramfs; additional boot-time initramfs overlays and persistent backup data are not merged",
        }
    });
    let mut outcome = success_outcome(observation, stderr);
    if usage_timed_out {
        outcome.status = TestStatus::TimedOut;
        outcome.exit_code = None;
        outcome.diagnostics =
            "Tiny Core filesystem usage inspection exceeded the configured timeout".into();
    }
    outcome
}

#[derive(Debug, Clone)]
struct InitrdCandidate {
    device: String,
    path: String,
    size_bytes: u64,
}

#[derive(Debug)]
struct TinyCoreImageInfo {
    expanded_size: usize,
    version: Option<String>,
    architecture: Option<String>,
    users: Vec<Value>,
    groups: Vec<Value>,
    passwd_file_found: bool,
    group_file_found: bool,
}

async fn find_initrd_candidates(
    invocation: &TestInvocation,
    disk: &ResolvedDisk,
    objects: &[Value],
    started: Instant,
) -> (Vec<InitrdCandidate>, Vec<String>, bool) {
    let mut candidates = Vec::new();
    let mut errors = Vec::new();
    for filesystem in objects
        .iter()
        .filter(|object| object["type"] == "filesystem")
    {
        let Some(device) = filesystem["name"].as_str() else {
            continue;
        };
        let output = run_tool(
            invocation,
            "virt-ls",
            vec![
                format!("--format={}", disk.format),
                "-lR".into(),
                "--csv".into(),
                "-a".into(),
                disk.path.to_string_lossy().into_owned(),
                "-m".into(),
                format!("{device}:/"),
                "/".into(),
            ],
            started,
        )
        .await;
        let output = match output {
            Ok(output) if output.timed_out => return (candidates, errors, true),
            Ok(output) if output.exit_code == Some(0) => output,
            Ok(output) => {
                errors.push(format!(
                    "virt-ls could not inspect {device}: {}",
                    output.stderr.trim()
                ));
                continue;
            }
            Err(error) => {
                errors.push(error);
                continue;
            }
        };
        if output.stdout_truncated {
            errors.push(format!(
                "virt-ls output for {device} was truncated; Tiny Core boot files may be missing from the scan"
            ));
            return (Vec::new(), errors, false);
        }
        let parsed = match parse_initrd_listing(device, &output.stdout) {
            Ok(candidates) => candidates,
            Err(error) => {
                errors.push(format!("cannot parse virt-ls output for {device}: {error}"));
                continue;
            }
        };
        for candidate in parsed {
            if !candidates.iter().any(|existing: &InitrdCandidate| {
                existing.device == candidate.device && existing.path == candidate.path
            }) {
                candidates.push(candidate);
            }
        }
    }
    (candidates, errors, false)
}

fn parse_initrd_listing(device: &str, listing: &str) -> Result<Vec<InitrdCandidate>, String> {
    parse_csv(listing)?
        .into_iter()
        .filter_map(|fields| {
            if fields.len() < 4 {
                return Some(Err(
                    "recursive long listing row has fewer than four fields".into()
                ));
            }
            if fields[0] != "-" {
                return None;
            }
            let size_bytes = match fields[2].parse::<u64>() {
                Ok(size) => size,
                Err(_) => return Some(Err("file size is not a valid byte count".into())),
            };
            let path = &fields[3];
            if !safe_guest_path(path) || !is_tinycore_initrd_name(path) {
                return None;
            }
            Some(Ok(InitrdCandidate {
                device: device.to_owned(),
                path: path.to_owned(),
                size_bytes,
            }))
        })
        .collect()
}

fn safe_guest_path(path: &str) -> bool {
    let path = Path::new(path);
    path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::RootDir | Component::Normal(_)))
}

fn is_tinycore_initrd_name(path: &str) -> bool {
    let Some(name) = Path::new(path).file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let name = name.to_ascii_lowercase();
    let Some(stem) = name.strip_suffix(".gz") else {
        return false;
    };
    stem == "rootfs"
        || stem.starts_with("core")
        || stem.starts_with("tinycore")
        || stem.starts_with("microcore")
}

async fn inspect_candidate(
    invocation: &TestInvocation,
    disk: &ResolvedDisk,
    candidate: &InitrdCandidate,
    started: Instant,
) -> Result<Option<TinyCoreImageInfo>, String> {
    let temporary = TemporaryDirectory::create(&invocation.artifacts_dir)?;
    let output = run_tool(
        invocation,
        "virt-copy-out",
        vec![
            format!("--format={}", disk.format),
            "-a".into(),
            disk.path.to_string_lossy().into_owned(),
            "-m".into(),
            format!("{}:/", candidate.device),
            candidate.path.clone(),
            temporary.path.to_string_lossy().into_owned(),
        ],
        started,
    )
    .await
    .map_err(|error| format!("cannot read initramfs with libguestfs: {error}"))?;
    if output.timed_out {
        return Err("copying the boot image timed out".into());
    }
    if output.exit_code != Some(0) {
        return Err(format!(
            "virt-copy-out exited with {:?}: {}",
            output.exit_code,
            output.stderr.trim()
        ));
    }
    let copied_path = copied_file_path(&temporary.path, &candidate.path).ok_or_else(|| {
        format!(
            "virt-copy-out succeeded but did not create the expected file for {}",
            candidate.path
        )
    })?;
    let metadata = fs::metadata(&copied_path)
        .map_err(|error| format!("cannot inspect copied initramfs: {error}"))?;
    if metadata.len() > MAX_INITRD_COMPRESSED_BYTES {
        return Err(format!(
            "copied initramfs exceeds the {} byte compressed image limit",
            MAX_INITRD_COMPRESSED_BYTES
        ));
    }
    let compressed =
        fs::read(&copied_path).map_err(|error| format!("cannot read copied initramfs: {error}"))?;
    let mut expanded = Vec::new();
    GzDecoder::new(compressed.as_slice())
        .take(MAX_INITRD_EXPANDED_BYTES + 1)
        .read_to_end(&mut expanded)
        .map_err(|error| format!("initramfs is not a readable gzip stream: {error}"))?;
    if expanded.len() as u64 > MAX_INITRD_EXPANDED_BYTES {
        return Err(format!(
            "decompressed initramfs exceeds the {} byte safety limit",
            MAX_INITRD_EXPANDED_BYTES
        ));
    }
    let archive = parse_newc_archive(&expanded)?;
    if !archive.is_tinycore {
        return Ok(None);
    }
    Ok(Some(TinyCoreImageInfo {
        expanded_size: expanded.len(),
        version: detect_version(&archive.files)
            .or_else(|| detect_version_from_name(&candidate.path)),
        architecture: detect_architecture(&archive.files)
            .or_else(|| detect_architecture_from_name(&candidate.path)),
        users: archive
            .files
            .get("etc/passwd")
            .map(|bytes| parse_passwd(&String::from_utf8_lossy(bytes)))
            .unwrap_or_default(),
        groups: archive
            .files
            .get("etc/group")
            .map(|bytes| parse_group(&String::from_utf8_lossy(bytes)))
            .unwrap_or_default(),
        passwd_file_found: archive.files.contains_key("etc/passwd"),
        group_file_found: archive.files.contains_key("etc/group"),
    }))
}

fn copied_file_path(directory: &Path, guest_path: &str) -> Option<PathBuf> {
    let relative = guest_path.trim_start_matches('/');
    let nested = directory.join(relative);
    if nested.is_file() {
        return Some(nested);
    }
    let basename = Path::new(guest_path).file_name()?;
    let flat = directory.join(basename);
    flat.is_file().then_some(flat)
}

struct CpioArchive {
    files: HashMap<String, Vec<u8>>,
    is_tinycore: bool,
}

fn parse_newc_archive(bytes: &[u8]) -> Result<CpioArchive, String> {
    let mut offset = 0usize;
    let mut files = HashMap::new();
    let mut paths = HashSet::new();
    let mut saw_archive_entry = false;
    while offset < bytes.len() {
        let header_end = offset
            .checked_add(110)
            .filter(|end| *end <= bytes.len())
            .ok_or("truncated CPIO header")?;
        let magic = &bytes[offset..offset + 6];
        if magic != b"070701" && magic != b"070702" {
            return Err("Tiny Core initramfs is not a supported newc CPIO archive".into());
        }
        let field = |index: usize| -> Result<u32, String> {
            let start = offset + 6 + index * 8;
            let text = std::str::from_utf8(&bytes[start..start + 8])
                .map_err(|_| "CPIO header contains a non-ASCII field")?;
            u32::from_str_radix(text, 16)
                .map_err(|_| "CPIO header contains invalid hexadecimal data".into())
        };
        let mode = field(1)?;
        let file_size = usize::try_from(field(6)?).map_err(|_| "CPIO file size is too large")?;
        let name_size = usize::try_from(field(11)?).map_err(|_| "CPIO name size is too large")?;
        let name_end = header_end
            .checked_add(name_size)
            .filter(|end| *end <= bytes.len())
            .ok_or("truncated CPIO file name")?;
        if name_size == 0 || bytes[name_end - 1] != 0 {
            return Err("CPIO file name is not NUL terminated".into());
        }
        let name = std::str::from_utf8(&bytes[header_end..name_end - 1])
            .map_err(|_| "CPIO file name is not UTF-8")?
            .trim_start_matches("./")
            .trim_start_matches('/')
            .to_owned();
        let data_start = align4(name_end);
        let data_end = data_start
            .checked_add(file_size)
            .filter(|end| *end <= bytes.len())
            .ok_or("truncated CPIO file contents")?;
        if name == "TRAILER!!!" {
            break;
        }
        saw_archive_entry = true;
        paths.insert(name.clone());
        if is_regular_file(mode) && is_interesting_file(&name) && file_size <= 2 * 1024 * 1024 {
            files.insert(name, bytes[data_start..data_end].to_vec());
        }
        offset = align4(data_end);
    }
    if !saw_archive_entry {
        return Err("CPIO archive did not contain any entries".into());
    }
    let issue = files
        .get("etc/issue")
        .map(|value| String::from_utf8_lossy(value).to_ascii_lowercase())
        .unwrap_or_default();
    let release_text = ["etc/tc-release", "etc/os-release", "etc/version"]
        .iter()
        .filter_map(|path| files.get(*path))
        .map(|value| String::from_utf8_lossy(value).to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("\n");
    let has_core_init = paths.contains("etc/init.d/tc-config");
    let has_tc_tools =
        paths.contains("usr/bin/tce-load") || paths.contains("usr/local/bin/tce-load");
    let textual_marker = issue.contains("tiny core")
        || issue.contains("tinycore")
        || release_text.contains("tinycore")
        || release_text.contains("tiny core");
    Ok(CpioArchive {
        files,
        is_tinycore: (has_core_init && has_tc_tools) || (has_core_init && textual_marker),
    })
}

fn is_interesting_file(path: &str) -> bool {
    matches!(
        path,
        "etc/passwd"
            | "etc/group"
            | "etc/issue"
            | "etc/tc-release"
            | "etc/os-release"
            | "etc/version"
            | "etc/tce-version"
            | "etc/hostname"
    )
}

fn is_regular_file(mode: u32) -> bool {
    mode & 0o170000 == 0o100000
}

fn align4(value: usize) -> usize {
    value.saturating_add(3) & !3
}

fn detect_version(files: &HashMap<String, Vec<u8>>) -> Option<String> {
    for path in ["etc/tc-release", "etc/version", "etc/tce-version"] {
        if let Some(bytes) = files.get(path) {
            let value = String::from_utf8_lossy(bytes).trim().to_owned();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    let release = files
        .get("etc/os-release")
        .or_else(|| files.get("etc/issue"))
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())?;
    let regex =
        regex::Regex::new(r"(?i)(?:tiny\s*core|tinycore)[^0-9]{0,24}([0-9]+(?:\.[0-9]+){0,3})")
            .ok()?;
    regex
        .captures(&release)
        .and_then(|capture| capture.get(1))
        .map(|version| version.as_str().to_owned())
}

fn detect_version_from_name(path: &str) -> Option<String> {
    let name = Path::new(path).file_name()?.to_str()?;
    let regex =
        regex::Regex::new(r"(?i)^(?:tinycore|microcore|core)[-_]?([0-9]+(?:\.[0-9]+){0,3})\.gz$")
            .ok()?;
    regex
        .captures(name)
        .and_then(|capture| capture.get(1))
        .map(|version| version.as_str().to_owned())
}

fn detect_architecture(files: &HashMap<String, Vec<u8>>) -> Option<String> {
    let issue = files
        .get("etc/issue")
        .map(|bytes| String::from_utf8_lossy(bytes).to_ascii_lowercase())
        .unwrap_or_default();
    let release = files
        .get("etc/os-release")
        .map(|bytes| String::from_utf8_lossy(bytes).to_ascii_lowercase())
        .unwrap_or_default();
    let text = format!("{issue}\n{release}");
    if text.contains("pure64") || text.contains("x86_64") || text.contains("amd64") {
        Some("x86_64".into())
    } else if text.contains("arm64") || text.contains("aarch64") {
        Some("aarch64".into())
    } else if text.contains("armv7") || text.contains("armhf") {
        Some("armv7".into())
    } else if text.contains("i686") || text.contains("x86") {
        Some("x86".into())
    } else {
        None
    }
}

fn detect_architecture_from_name(path: &str) -> Option<String> {
    let name = Path::new(path).file_name()?.to_str()?.to_ascii_lowercase();
    if name.contains("pure64") || name.contains("core64") {
        Some("x86_64".into())
    } else if name.contains("arm64") || name.contains("aarch64") {
        Some("aarch64".into())
    } else {
        None
    }
}

fn partitions_stderr(reason: &str, objects: &[Value]) -> String {
    let summary = partition_summary(objects);
    format!(
        "{reason}. Partitions found:\n{}",
        serde_yaml::to_string(&summary)
            .unwrap_or_else(|_| "- unable to format partition details\n".into())
    )
}

fn timed_out_outcome(reason: &str, diagnostics: &str, objects: &[Value]) -> TestOutcome {
    TestOutcome {
        status: TestStatus::TimedOut,
        stdout: String::new(),
        stderr: partitions_stderr(reason, objects),
        exit_code: None,
        diagnostics: diagnostics.into(),
        observation: json!({}),
        evidence: Vec::new(),
    }
}

struct TemporaryDirectory {
    path: PathBuf,
}

impl TemporaryDirectory {
    fn create(parent: &Path) -> Result<Self, String> {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "cannot create test artifact directory {}: {error}",
                parent.display()
            )
        })?;
        let path = parent.join(format!("tinycore-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir(&path).map_err(|error| {
            format!(
                "cannot create temporary directory {}: {error}",
                path.display()
            )
        })?;
        Ok(Self { path })
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
