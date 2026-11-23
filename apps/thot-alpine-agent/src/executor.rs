use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::{self, Write},
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::{Component, Path, PathBuf},
    process::Stdio,
};
use thot_core::sandbox_protocol::{
    ExecuteTaskRequest, ExecuteTaskResult, ExecutionStatus, InstallPackageRequest,
    InstallPackageResult, StageFileChunkRequest, StageFileChunkResult, SyncRequest, SyncResult,
};
use tokio::{
    io::AsyncReadExt,
    process::{Child, Command},
    time::{Duration, Instant, timeout},
};

pub async fn execute_task(
    task: ExecuteTaskRequest,
) -> Result<ExecuteTaskResult, Box<dyn std::error::Error>> {
    if task.task_id.is_empty()
        || task.run_id.is_empty()
        || task.student_unit_id.is_empty()
        || task.argv.is_empty()
        || task.timeout == 0
    {
        return Err("task has missing identity, argv, or timeout".into());
    }
    let cwd = task.working_dir.canonicalize()?;
    if !cwd.starts_with("/work") || !cwd.is_dir() {
        return Err("working directory is outside /work".into());
    }
    let started = Instant::now();
    let mut command = Command::new(&task.argv[0]);
    command
        .args(&task.argv[1..])
        .current_dir(cwd)
        .env_clear()
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    {
        command.process_group(0);
        unsafe {
            command.as_std_mut().pre_exec(|| {
                if libc::setgroups(0, std::ptr::null()) != 0 {
                    return Err(io::Error::last_os_error());
                }
                if libc::setgid(1001) != 0 {
                    return Err(io::Error::last_os_error());
                }
                if libc::setuid(1001) != 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    let mut child = command.spawn()?;
    let stdout = child.stdout.take().ok_or("stdout pipe unavailable")?;
    let stderr = child.stderr.take().ok_or("stderr pipe unavailable")?;
    let out_limit = task.stdout_limit.min(task.limits.output_limits.stdout);
    let err_limit = task.stderr_limit.min(task.limits.output_limits.stderr);
    let out = tokio::spawn(read_capped(stdout, out_limit));
    let err = tokio::spawn(read_capped(stderr, err_limit));
    let (status, timed_out) =
        match timeout(Duration::from_secs(task.timeout as u64), child.wait()).await {
            Ok(result) => (Some(result?), false),
            Err(_) => {
                kill_process_group(&mut child).await;
                (None, true)
            }
        };
    let (stdout, stdout_truncated) = out.await??;
    let (stderr, stderr_truncated) = err.await??;
    let exit_code = status.and_then(|s| s.code());
    let signal = status.and_then(|s| {
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            s.signal()
        }
        #[cfg(not(unix))]
        {
            None
        }
    });
    let diagnostic = if timed_out {
        format!("command exceeded {} second timeout", task.timeout)
    } else {
        String::new()
    };
    Ok(ExecuteTaskResult {
        task_id: task.task_id,
        run_id: task.run_id,
        student_unit_id: task.student_unit_id,
        exit_code,
        signal,
        stdout,
        stderr,
        diagnostics: diagnostic,
        elapsed_ms: started.elapsed().as_millis(),
        status: if timed_out {
            ExecutionStatus::TimedOut
        } else {
            ExecutionStatus::Completed
        },
        timed_out,
        resource_exceeded: false,
        platform_error: false,
        cancelled: false,
        interrupted: false,
        truncated: stdout_truncated || stderr_truncated,
    })
}

async fn read_capped<R: AsyncReadExt + Unpin>(
    mut reader: R,
    cap: usize,
) -> io::Result<(String, bool)> {
    let mut stored = Vec::with_capacity(cap.min(64 * 1024));
    let mut buffer = [0u8; 8192];
    let mut truncated = false;
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let remaining = cap.saturating_sub(stored.len());
        let keep = read.min(remaining);
        stored.extend_from_slice(&buffer[..keep]);
        truncated |= keep < read;
    }
    Ok((String::from_utf8_lossy(&stored).into_owned(), truncated))
}

async fn kill_process_group(child: &mut Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
    let _ = child.kill().await;
    let _ = child.wait().await;
}

struct StagedFile {
    destination: PathBuf,
    temporary: PathBuf,
    total_size: u64,
    expected_sha256: String,
    next_chunk: u64,
    received: u64,
    file: std::fs::File,
}

pub struct StageStore {
    files: HashMap<String, StagedFile>,
}

impl StageStore {
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
        }
    }

    pub fn stage(&mut self, chunk: StageFileChunkRequest) -> io::Result<StageFileChunkResult> {
        const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;
        const MAX_CHUNK_BYTES: usize = 256 * 1024;
        if chunk.task_id.is_empty()
            || chunk.total_size > MAX_FILE_BYTES
            || chunk.bytes.len() > MAX_CHUNK_BYTES
            || chunk.expected_sha256.len() != 64
            || !chunk.expected_sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid file chunk limits or digest",
            ));
        }
        let root = fs::canonicalize("/work")?;
        let relative = chunk.file_path.strip_prefix("/work/").ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "file path must be below /work")
        })?;
        let relative_path = Path::new(relative);
        if relative_path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unsafe file path",
            ));
        }
        if chunk.chunk_index == 0 {
            if self.files.contains_key(&chunk.task_id) {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "duplicate file transfer",
                ));
            }
            let destination = root.join(relative_path);
            let parent = destination
                .parent()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "file has no parent"))?;
            fs::create_dir_all(parent)?;
            let canonical_parent = fs::canonicalize(parent)?;
            if !canonical_parent.starts_with(&root) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "file path escapes /work",
                ));
            }
            if fs::symlink_metadata(&destination).is_ok() {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "destination already exists",
                ));
            }
            let task_component: String = chunk
                .task_id
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
                .take(80)
                .collect();
            if task_component.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "invalid transfer task ID",
                ));
            }
            let temporary = parent.join(format!(".thot-{task_component}.part"));
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            self.files.insert(
                chunk.task_id.clone(),
                StagedFile {
                    destination,
                    temporary,
                    total_size: chunk.total_size,
                    expected_sha256: chunk.expected_sha256.clone(),
                    next_chunk: 0,
                    received: 0,
                    file,
                },
            );
        }
        let transfer = self.files.get_mut(&chunk.task_id).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "file transfer did not start at chunk zero",
            )
        })?;
        if chunk.chunk_index != transfer.next_chunk
            || chunk.total_size != transfer.total_size
            || chunk.expected_sha256 != transfer.expected_sha256
            || transfer.received + chunk.bytes.len() as u64 > transfer.total_size
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "out-of-order or inconsistent file chunk",
            ));
        }
        transfer.file.write_all(&chunk.bytes)?;
        transfer.received += chunk.bytes.len() as u64;
        transfer.next_chunk += 1;
        if !chunk.final_chunk {
            return Ok(StageFileChunkResult {
                task_id: chunk.task_id,
                complete: false,
                sha256: None,
            });
        }
        if transfer.received != transfer.total_size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "final chunk does not match declared size",
            ));
        }
        transfer.file.sync_all()?;
        let data = fs::read(&transfer.temporary)?;
        let digest = format!("{:x}", Sha256::digest(&data));
        if digest != transfer.expected_sha256 {
            let temporary = transfer.temporary.clone();
            self.files.remove(&chunk.task_id);
            let _ = fs::remove_file(temporary);
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "staged file SHA-256 mismatch",
            ));
        }
        let staged = self.files.remove(&chunk.task_id).expect("transfer exists");
        fs::rename(&staged.temporary, &staged.destination)?;
        fs::set_permissions(&staged.destination, fs::Permissions::from_mode(0o444))?;
        Ok(StageFileChunkResult {
            task_id: chunk.task_id,
            complete: true,
            sha256: Some(digest),
        })
    }
}

pub async fn install_package(task: InstallPackageRequest) -> InstallPackageResult {
    let start = Instant::now();
    if task.package_name.is_empty()
        || !task
            .package_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    {
        return package_error(task.task_id, "invalid package name", start);
    }
    let output = Command::new("apk")
        .args(["add", "--no-progress", &task.package_name])
        .output()
        .await;
    match output {
        Ok(o) => InstallPackageResult {
            task_id: task.task_id,
            exit_code: o.status.code(),
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
            diagnostics: String::new(),
            elapsed_ms: start.elapsed().as_millis(),
            timed_out: false,
            truncated: false,
        },
        Err(e) => package_error(task.task_id, &e.to_string(), start),
    }
}

fn package_error(task_id: String, message: &str, start: Instant) -> InstallPackageResult {
    InstallPackageResult {
        task_id,
        exit_code: None,
        stdout: String::new(),
        stderr: String::new(),
        diagnostics: message.to_string(),
        elapsed_ms: start.elapsed().as_millis(),
        timed_out: false,
        truncated: false,
    }
}

pub async fn sync(task: SyncRequest) -> SyncResult {
    let start = Instant::now();
    match Command::new("sync").status().await {
        Ok(status) => SyncResult {
            task_id: task.task_id,
            exit_code: status.code(),
            stdout: String::new(),
            stderr: String::new(),
            diagnostics: String::new(),
            elapsed_ms: start.elapsed().as_millis(),
            timed_out: false,
            truncated: false,
        },
        Err(error) => SyncResult {
            task_id: task.task_id,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            diagnostics: error.to_string(),
            elapsed_ms: start.elapsed().as_millis(),
            timed_out: false,
            truncated: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;
    #[tokio::test]
    async fn capped_reader_drains_and_marks_truncation() {
        let input = BufReader::new(&b"abcdefgh"[..]);
        let (text, truncated) = read_capped(input, 3).await.unwrap();
        assert_eq!(text, "abc");
        assert!(truncated);
    }
}
