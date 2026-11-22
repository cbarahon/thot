use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

#[derive(Debug)]
pub(crate) struct HostCommandOutput {
    pub(crate) exit_code: Option<i32>,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
    pub(crate) elapsed: Duration,
    pub(crate) timed_out: bool,
    pub(crate) stdout_truncated: bool,
    pub(crate) stderr_truncated: bool,
}

#[cfg(unix)]
pub(crate) async fn run_host_command(
    executable: &Path,
    argv: &[OsString],
    cwd: &Path,
    deadline: Duration,
    stdout_limit: usize,
    stderr_limit: usize,
) -> Result<HostCommandOutput, ProviderError> {
    let started = Instant::now();
    let mut command = Command::new(executable);
    command
        .args(argv)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command
        .spawn()
        .map_err(|e| ProviderError::new(format!("cannot start host command: {e}")))?;

    let stdout = child
        .stdout
        .take()
        .ok_or(ProviderError::new("could not capture host command stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or(ProviderError::new("could not capture host command stderr"))?;

    let stdout_reader = tokio::spawn(read_capped(stdout, stdout_limit));
    let stderr_reader = tokio::spawn(read_capped(stderr, stderr_limit));
    let (status, timed_out) = match timeout(deadline, child.wait()).await {
        Ok(status) => (
            Some(status.map_err(|e| ProviderError::new(format!("cannot wait for host command: {e}")))?),
            false,
        ),
        Err(_) => {
            terminate_child(&mut child).await;
            let _ = child.wait().await;
            (None, true)
        }
    };
    let stdout = stdout_reader
        .await
        .map_err(|e| ProviderError::new(format!("cannot collect host command stdout: {e}")))?;
    let stderr = stderr_reader
        .await
        .map_err(|e| ProviderError::new(format!("cannot collect host command stderr: {e}")))?;

    Ok(HostCommandOutput {
        exit_code: status.and_then(|status| status.code()),
        stdout: stdout.data,
        stderr: stderr.data,
        elapsed: started.elapsed(),
        timed_out,
        stdout_truncated: stdout.truncated,
        stderr_truncated: stderr.truncated,
    })
}

#[cfg(unix)]
async fn terminate_child(child: &mut Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        // The test starts in its own process group so timeout also stops child tools it launched.
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
    let _ = child.kill().await;
}

#[cfg(unix)]
async fn read_capped(mut reader: impl AsyncReadExt, limit: usize) -> Result<BoundedOutput, String> {
    let mut data = vec![0; limit];
    let len = reader.read(&mut data).await.map_err(|e| format!("read failed: {e}"))?;
    data.truncate(len);
    Ok(BoundedOutput {
        data,
        truncated: len == limit,
    })
}

#[cfg(unix)]
pub(crate) struct BoundedOutput {
    data: Vec<u8>,
    truncated: bool,
}

#[cfg(unix)]
#[derive(Debug)]
pub(crate) struct ProviderError {
    message: String,
}

#[cfg(unix)]
impl ProviderError {
    pub(crate) fn new(message: String) -> Self {
        ProviderError { message }
    }
}