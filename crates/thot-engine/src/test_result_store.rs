//! Purpose: Creates per-test artifact directories and atomically stores output and expedients.
use std::{
    fs, io,
    path::{Path, PathBuf},
};

fn safe_component(value: &str) -> String {
    let cleaned = value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    let cleaned = cleaned.trim_matches('.');
    if cleaned.is_empty() {
        "unnamed".into()
    } else {
        cleaned.chars().take(120).collect()
    }
}

pub fn paths_for_test(
    report_dir: &Path,
    delivery_filename: &str,
    test_name: &str,
) -> io::Result<(PathBuf, PathBuf)> {
    let record_dir = report_dir
        .join("test_records")
        .join(safe_component(delivery_filename))
        .join(safe_component(test_name));
    let artifacts = record_dir.join("artifacts");
    fs::create_dir_all(&artifacts)?;
    Ok((record_dir, artifacts))
}

pub fn persist_output(record_dir: &Path, stdout: &str, stderr: &str) -> io::Result<()> {
    atomic_write(&record_dir.join("stdout.txt"), stdout.as_bytes())?;
    atomic_write(&record_dir.join("stderr.txt"), stderr.as_bytes())
}

pub fn persist_report(path: &Path, report: &serde_json::Value) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(report).map_err(io::Error::other)?;
    atomic_write(path, &bytes)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("report path has no parent"))?;
    fs::create_dir_all(parent)?;
    let temp = path.with_extension(format!("tmp-{}", std::process::id()));
    fs::write(&temp, bytes)?;
    fs::rename(temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_paths_are_scoped_and_include_the_requested_layout() {
        let root = std::env::temp_dir().join(format!("thot-record-{}", uuid::Uuid::new_v4()));
        let (record, artifacts) =
            paths_for_test(&root, "student one.ova", "start_time_01").unwrap();
        assert!(artifacts.ends_with("test_records/student_one.ova/start_time_01/artifacts"));
        assert!(record.starts_with(&root));
        fs::remove_dir_all(root).unwrap();
    }
}
