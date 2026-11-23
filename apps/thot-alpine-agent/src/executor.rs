use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::{Component, Path, PathBuf},
    process::Stdio,
};
use thot_core::sandbox_contracts::PlatformCapability;
use thot_core::sandbox_protocol::{
    ExecuteTaskRequest, ExecuteTaskResult, ExecutionStatus, InstallPackageRequest,
    InstallPackageResult, ModuleLaunch, StageFileChunkRequest, StageFileChunkResult, SyncRequest,
    SyncResult,
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
        || (task.argv.is_empty() && task.module.is_none())
        || task.timeout == 0
    {
        return Err("task has missing identity, argv, or timeout".into());
    }
    let (program, args) = match &task.module {
        Some(module) => match validate_module_launch(module, &task.argv) {
            Ok(command) => command,
            Err((status, diagnostic)) => {
                return Ok(early_result(&task, status, &diagnostic));
            }
        },
        None => (PathBuf::from(&task.argv[0]), task.argv[1..].to_vec()),
    };
    let cwd = task.working_dir.canonicalize()?;
    if !cwd.starts_with("/work") || !cwd.is_dir() {
        return Err("working directory is outside /work".into());
    }
    let started = Instant::now();
    let mut command = Command::new(program);
    command
        .args(&args)
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

fn validate_module_launch(
    module: &ModuleLaunch,
    module_args: &[String],
) -> Result<(PathBuf, Vec<String>), (ExecutionStatus, String)> {
    validate_module_launch_at(module, module_args, Path::new("/work/.thot/modules"))
}

fn validate_module_launch_at(
    module: &ModuleLaunch,
    module_args: &[String],
    module_root: &Path,
) -> Result<(PathBuf, Vec<String>), (ExecutionStatus, String)> {
    if module.module_id.is_empty() || module.version.is_empty() {
        return Err((
            ExecutionStatus::PlatformError,
            "module identity is incomplete".into(),
        ));
    }
    for required in &module.capabilities {
        if !capabilities()
            .iter()
            .any(|available| available.id == required.id && available.version == required.version)
        {
            return Err((
                ExecutionStatus::Unsupported,
                format!(
                    "guest agent lacks capability {}@{}",
                    required.id, required.version
                ),
            ));
        }
    }
    let path = &module.package_path;
    if !path.starts_with(module_root) || path == module_root {
        return Err((
            ExecutionStatus::PlatformError,
            "module path is outside the private module staging area".into(),
        ));
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| (ExecutionStatus::PlatformError, error.to_string()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > 32 * 1024 * 1024
    {
        return Err((
            ExecutionStatus::PlatformError,
            "module package is not a bounded regular file".into(),
        ));
    }
    let actual =
        hash_file(path).map_err(|error| (ExecutionStatus::PlatformError, error.to_string()))?;
    if actual != module.package_sha256 {
        return Err((
            ExecutionStatus::PlatformError,
            "module package SHA-256 does not match its manifest".into(),
        ));
    }
    let canonical = path
        .canonicalize()
        .map_err(|error| (ExecutionStatus::PlatformError, error.to_string()))?;
    if !canonical.starts_with(module_root) {
        return Err((
            ExecutionStatus::PlatformError,
            "module path resolves outside its staging area".into(),
        ));
    }
    let mut args = vec![canonical.to_string_lossy().into_owned()];
    args.extend_from_slice(module_args);
    Ok((PathBuf::from("/bin/sh"), args))
}

fn early_result(
    task: &ExecuteTaskRequest,
    status: ExecutionStatus,
    diagnostic: &str,
) -> ExecuteTaskResult {
    ExecuteTaskResult {
        task_id: task.task_id.clone(),
        run_id: task.run_id.clone(),
        student_unit_id: task.student_unit_id.clone(),
        exit_code: None,
        signal: None,
        stdout: String::new(),
        stderr: String::new(),
        diagnostics: diagnostic.into(),
        elapsed_ms: 0,
        status,
        timed_out: false,
        resource_exceeded: false,
        platform_error: status == ExecutionStatus::PlatformError,
        cancelled: false,
        interrupted: false,
        truncated: false,
    }
}

pub fn capabilities() -> Vec<PlatformCapability> {
    let mut available = vec![
        PlatformCapability {
            id: "process.argv".into(),
            version: 1,
        },
        PlatformCapability {
            id: "files.stage".into(),
            version: 1,
        },
        PlatformCapability {
            id: "files.sha256".into(),
            version: 1,
        },
        PlatformCapability {
            id: "module.posix-shell".into(),
            version: 1,
        },
    ];
    if ["/usr/bin/guestfish", "/usr/sbin/guestfish"]
        .iter()
        .any(|path| Path::new(path).is_file())
    {
        available.push(PlatformCapability {
            id: "disk.inspect".into(),
            version: 1,
        });
    }
    use std::os::unix::fs::FileTypeExt;
    if fs::metadata("/dev/kvm").is_ok_and(|metadata| metadata.file_type().is_char_device()) {
        available.push(PlatformCapability {
            id: "nested.kvm".into(),
            version: 1,
        });
    }
    available
}

fn hash_file(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hash.finalize()))
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
        const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024 * 1024;
        const MAX_CHUNK_BYTES: usize = 256 * 1024;
        if chunk.task_id.is_empty()
            || chunk.total_size > MAX_FILE_BYTES
            || chunk.bytes.len() > MAX_CHUNK_BYTES
            || (chunk.bytes.is_empty() && !chunk.final_chunk)
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
        let digest = hash_file(&transfer.temporary)?;
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
    use sha2::Digest;
    use thot_core::sandbox_contracts::PlatformCapability;
    use tokio::io::BufReader;

    #[tokio::test]
    async fn capped_reader_drains_and_marks_truncation() {
        let input = BufReader::new(&b"abcdefgh"[..]);
        let (text, truncated) = read_capped(input, 3).await.unwrap();
        assert_eq!(text, "abc");
        assert!(truncated);
    }

    #[test]
    fn module_launch_verifies_digest_and_guest_capabilities() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("thot-module-test-{}-{unique}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let package = root.join("entrypoint.sh");
        fs::write(&package, b"#!/bin/sh\nprintf module-ok\\n\n").unwrap();
        let digest = format!("{:x}", Sha256::digest(fs::read(&package).unwrap()));
        let module = ModuleLaunch {
            module_id: "thot.test.module".into(),
            version: "1.0.0".into(),
            package_sha256: digest,
            package_path: package.clone(),
            capabilities: vec![PlatformCapability {
                id: "module.posix-shell".into(),
                version: 1,
            }],
        };
        assert!(validate_module_launch_at(&module, &["arg".into()], &root).is_ok());

        let mut bad_digest = module.clone();
        bad_digest.package_sha256 = "0".repeat(64);
        assert!(validate_module_launch_at(&bad_digest, &[], &root).is_err());

        let mut missing_capability = module;
        missing_capability.capabilities.push(PlatformCapability {
            id: "disk.inspect".into(),
            version: 999,
        });
        assert!(matches!(
            validate_module_launch_at(&missing_capability, &[], &root),
            Err((ExecutionStatus::Unsupported, _))
        ));
        fs::remove_dir_all(root).unwrap();
    }
}
