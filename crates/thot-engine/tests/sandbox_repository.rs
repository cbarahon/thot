use serde_json::json;
use sha2::Digest;
use std::{fs, path::PathBuf};
use thot_core::sandbox_contracts::DeliveryProvenance;
use thot_engine::sandbox_repository::{
    ProjectRunRepository, export_run_archive, import_run_archive,
};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("thot-run-store-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn snapshot_files() -> Vec<(String, Vec<u8>)> {
    vec![
        (
            "configuration.thot.yaml".into(),
            b"schema_version: thot.dev/v2\n".to_vec(),
        ),
        ("students.yaml".into(), b"students: []\n".to_vec()),
        ("assignments.yaml".into(), b"assignments: []\n".to_vec()),
    ]
}

#[test]
fn run_package_persists_input_evidence_and_verifies_revision() {
    let scratch = Scratch::new();
    let source = scratch.0.join("submission.ova");
    fs::write(&source, b"smoke-deliverable").unwrap();
    let digest = format!("{:x}", sha2::Sha256::digest(b"smoke-deliverable"));
    let mut repository =
        ProjectRunRepository::begin(&scratch.0, "run-1", "project-1", &snapshot_files()).unwrap();
    repository.start().unwrap();
    let persisted = repository
        .persist_input(
            "student-1",
            &source,
            DeliveryProvenance {
                source_kind: "local".into(),
                selector: "submission.ova".into(),
                provider_id: None,
                revision: None,
                name: "submission.ova".into(),
                mime_type: None,
                source_checksum: None,
                source_checksum_algorithm: None,
                size_bytes: b"smoke-deliverable".len() as u64,
                created_at: None,
                modified_at: None,
                acquired_at: "2026-11-23T00:00:00Z".into(),
                sha256: digest,
            },
        )
        .unwrap();
    assert!(repository.run_dir().join(&persisted).is_file());
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let source_metadata = fs::metadata(&source).unwrap();
        let persisted_metadata = fs::metadata(repository.run_dir().join(&persisted)).unwrap();
        assert_eq!(source_metadata.dev(), persisted_metadata.dev());
        assert_eq!(source_metadata.ino(), persisted_metadata.ino());
        assert_eq!(persisted_metadata.permissions().mode() & 0o222, 0);
    }

    let output = repository
        .put_artifact(b"captured stdout", "text/plain", true)
        .unwrap();
    assert_eq!(
        fs::read(repository.run_dir().join(output.relative_path)).unwrap(),
        b"captured stdout"
    );
    let result_path = "reports/student-1.json";
    let result_file = repository.run_dir().join(result_path);
    fs::create_dir_all(result_file.parent().unwrap()).unwrap();
    fs::write(
        &result_file,
        serde_json::to_vec(&json!({"status": "completed"})).unwrap(),
    )
    .unwrap();
    let revision = repository
        .commit_revision("initial", vec![result_path.into()])
        .unwrap();
    assert_eq!(revision.configuration_sha256.len(), 64);
    assert_eq!(
        ProjectRunRepository::verify_run(repository.run_dir())
            .unwrap()
            .revision_id,
        Some(revision.revision_id)
    );
}

#[test]
fn directory_deliverable_is_persisted_as_a_safe_read_only_tree() {
    let scratch = Scratch::new();
    let source = scratch.0.join("student1");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("sum.sh"), b"echo 42\n").unwrap();
    let digest = format!(
        "{:x}",
        sha2::Sha256::digest([b"sum.sh\0".as_slice(), b"echo 42\n"].concat())
    );
    let mut repository =
        ProjectRunRepository::begin(&scratch.0, "run-dir", "project-1", &snapshot_files()).unwrap();
    repository.start().unwrap();
    let persisted = repository
        .persist_input(
            "student-1",
            &source,
            DeliveryProvenance {
                source_kind: "local".into(),
                selector: "student1".into(),
                provider_id: None,
                revision: None,
                name: "student1".into(),
                mime_type: None,
                source_checksum: None,
                source_checksum_algorithm: None,
                size_bytes: 8,
                created_at: None,
                modified_at: None,
                acquired_at: "2026-11-25T00:00:00Z".into(),
                sha256: digest,
            },
        )
        .unwrap();
    let persisted = repository.run_dir().join(persisted);
    assert_eq!(fs::read(persisted.join("sum.sh")).unwrap(), b"echo 42\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&persisted).unwrap().permissions().mode() & 0o222,
            0
        );
        assert_eq!(
            fs::metadata(persisted.join("sum.sh"))
                .unwrap()
                .permissions()
                .mode()
                & 0o222,
            0
        );
    }
}

#[test]
fn repository_rejects_a_second_writer_and_marks_unfinished_run_interrupted() {
    let scratch = Scratch::new();
    let first =
        ProjectRunRepository::begin(&scratch.0, "run-active", "project-1", &snapshot_files())
            .unwrap();
    assert!(
        ProjectRunRepository::begin(&scratch.0, "run-collision", "project-1", &snapshot_files(),)
            .is_err()
    );
    drop(first);

    let _next = ProjectRunRepository::begin(&scratch.0, "run-next", "project-1", &snapshot_files())
        .unwrap();
    let old_run: serde_json::Value =
        serde_json::from_slice(&fs::read(scratch.0.join("results/run-active/run.json")).unwrap())
            .unwrap();
    assert_eq!(old_run["status"], "interrupted");
}

#[test]
fn completed_run_archive_round_trips_and_is_verified_on_import() {
    let scratch = Scratch::new();
    let mut repository =
        ProjectRunRepository::begin(&scratch.0, "portable-run", "project-1", &snapshot_files())
            .unwrap();
    repository.start().unwrap();
    let result = repository.run_dir().join("student-1.json");
    fs::write(&result, br#"{"student_id":"student-1","tests":[]}"#).unwrap();
    repository
        .commit_revision("initial", vec!["student-1.json".into()])
        .unwrap();
    let run_dir = repository.run_dir().to_path_buf();
    drop(repository);

    let archive = scratch.0.join("portable-run.zip");
    export_run_archive(&run_dir, &archive).unwrap();
    let imported_dir = scratch.0.join("imported-run");
    let verification = import_run_archive(&archive, &imported_dir).unwrap();
    assert_eq!(
        verification.status,
        thot_core::sandbox_contracts::RunStatus::Complete
    );
    assert!(imported_dir.join("student-1.json").is_file());
}

#[test]
fn run_archive_import_rejects_traversal_and_compressed_members() {
    use std::io::Write;
    use zip::{ZipWriter, write::FileOptions};

    let scratch = Scratch::new();
    for (name, compression) in [
        ("traversal.zip", zip::CompressionMethod::Stored),
        ("compressed.zip", zip::CompressionMethod::Deflated),
    ] {
        let archive_path = scratch.0.join(name);
        let mut archive = ZipWriter::new(fs::File::create(&archive_path).unwrap());
        let member = if name == "traversal.zip" {
            "../outside.txt"
        } else {
            "run.json"
        };
        archive
            .start_file(
                member,
                FileOptions::default().compression_method(compression),
            )
            .unwrap();
        archive.write_all(b"not a valid committed run").unwrap();
        archive.finish().unwrap();
        let destination = scratch.0.join(format!("extract-{name}"));
        assert!(import_run_archive(&archive_path, &destination).is_err());
        assert!(!destination.exists());
    }
    assert!(!scratch.0.join("outside.txt").exists());
}
