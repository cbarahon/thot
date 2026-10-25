use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

fn execute_test(test_path: &Path, artifact_dir: &Path) -> Result<(), String> {
    let test_name = test_path.file_stem().unwrap().to_str().unwrap();
    let artifact_path = artifact_dir.join(test_name);

    if !test_path.is_file() {
        return Err(format!("Path is not a file: {}", test_path.display()));
    }

    if test_path.is_symlink() {
        return Err(format!("Path is a symlink: {}", test_path.display()));
    }

    if test_path.starts_with("..") {
        return Err(format!("Path escapes the project root: {}", test_path.display()));
    }

    let mut command = Command::new("thot-runner")
        .arg("--timeout")
        .arg("60") // Set timeout to 60 seconds
        .arg("--output")
        .arg(&artifact_path)
        .arg(test_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn process: {}", e))?;

    let output = command.wait_with_output().map_err(|e| format!("Failed to wait for process: {}", e))?;

    if output.status.success() {
        Ok(())
    } else if output.status.code().unwrap() == 124 {
        Err(format!("Test timed out: {}", test_path.display()))
    } else {
        Err(format!("Test failed with exit code {}: {}", output.status.code().unwrap(), test_path.display()))
    }
}

fn main() {
    let project_root = Path::new("/home/carlos/worktrees/thot/crates/thot-engine");
    let artifact_dir = project_root.join("artifacts");

    if !artifact_dir.exists() {
        fs::create_dir_all(&artifact_dir).expect("Failed to create artifact directory");
    }

    let test_dir = project_root.join("src/tests/extern");
    if test_dir.exists() {
        for entry in fs::read_dir(&test_dir).expect("Failed to read test directory") {
            let entry = entry.expect("Failed to read directory entry");
            let path = entry.path();
            if path.is_file() {
                if let Err(e) = execute_test(&path, &artifact_dir) {
                    eprintln!("Error executing test {}: {}", path.display(), e);
                }
            }
        }
    }
}
