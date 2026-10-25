use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn sanitize_path_component(component: &str) -> String {
    component.chars().filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-').collect()
}

fn create_artifact_directory(root: &Path, delivery_filename: &str, test_name: &str) -> io::Result<PathBuf> {
    let sanitized_delivery_filename = sanitize_path_component(delivery_filename);
    let sanitized_test_name = sanitize_path_component(test_name);
    let artifact_dir = root.join("test_records").join(&sanitized_delivery_filename).join(&sanitized_test_name).join("artifacts");
    fs::create_dir_all(&artifact_dir)?;
    Ok(artifact_dir)
}

fn save_json_report(root: &Path, delivery_filename: &str, test_name: &str, report: &serde_json::Value) -> io::Result<PathBuf> {
    let artifact_dir = create_artifact_directory(root, delivery_filename, test_name)?;
    let report_path = artifact_dir.join("report.json");
    let mut file = fs::File::create(&report_path)?;
    serde_json::to_writer_pretty(&mut file, report)?;
    Ok(report_path)
}

fn save_artifact(root: &Path, delivery_filename: &str, test_name: &str, artifact_name: &str, content: &[u8]) -> io::Result<PathBuf> {
    let artifact_dir = create_artifact_directory(root, delivery_filename, test_name)?;
    let artifact_path = artifact_dir.join(artifact_name);
    fs::write(&artifact_path, content)?;
    Ok(artifact_path)
}

fn save_stdout_stderr(root: &Path, delivery_filename: &str, test_name: &str, stdout: &[u8], stderr: &[u8]) -> io::Result<()> {
    let artifact_dir = create_artifact_directory(root, delivery_filename, test_name)?;
    fs::write(artifact_dir.join("stdout.txt"), stdout)?;
    fs::write(artifact_dir.join("stderr.txt"), stderr)?;
    Ok(())
}

fn save_evidence(root: &Path, delivery_filename: &str, test_name: &str, evidence_name: &str, evidence_path: &Path) -> io::Result<PathBuf> {
    let artifact_dir = create_artifact_directory(root, delivery_filename, test_name)?;
    let evidence_relative_path = evidence_path.strip_prefix(root).unwrap_or(evidence_path);
    let evidence_target_path = artifact_dir.join(evidence_relative_path);
    fs::create_dir_all(evidence_target_path.parent().unwrap())?;
    fs::rename(evidence_path, &evidence_target_path)?;
    Ok(evidence_target_path)
}

fn main() -> io::Result<()> {
    let root = Path::new("/path/to/artifact/root");
    let delivery_filename = "delivery123";
    let test_name = "test_case_1";
    let report = serde_json::json!({
        "status": "success",
        "duration": 1.23,
        "output": "Test passed"
    });
    let stdout = b"Test output\n";
    let stderr = b"Test error\n";
    let artifact_name = "example_artifact.txt";
    let artifact_content = b"Artifact content";
    let evidence_name = "example_evidence.txt";
    let evidence_path = Path::new("/path/to/evidence/example_evidence.txt");

    save_json_report(root, delivery_filename, test_name, &report)?;
    save_stdout_stderr(root, delivery_filename, test_name, stdout, stderr)?;
    save_artifact(root, delivery_filename, test_name, artifact_name, artifact_content)?;
    save_evidence(root, delivery_filename, test_name, evidence_name, evidence_path)?;

    Ok(())
}
