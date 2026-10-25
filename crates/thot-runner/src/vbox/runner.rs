//! Purpose: Invokes VBoxManage with configured profile, timeouts, and bounded output capture.
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use tokio::{io::AsyncRead, process::Command, time::timeout};

#[derive(Clone, Debug)]
pub struct OutputLimits {
    pub stdout_bytes: usize,
    pub stderr_bytes: usize,
}

impl Default for OutputLimits {
    fn default() -> Self {
        Self {
            stdout_bytes: 1024 * 1024,
            stderr_bytes: 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug)]
pub struct VBoxCommandOutput {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration: Duration,
}

#[derive(Clone, Debug)]
pub struct LocalVirtualBoxRunner {
    vbox_manage_path: PathBuf,
    vbox_user_home: PathBuf,
    timeout: Duration,
    storage_root: PathBuf,
    output_limits: OutputLimits,
}

impl LocalVirtualBoxRunner {
    pub fn new(
        vbox_manage_path: PathBuf,
        vbox_user_home: PathBuf,
        timeout: Duration,
        storage_root: PathBuf,
        output_limits: OutputLimits,
    ) -> Self {
        Self {
            vbox_manage_path,
            vbox_user_home,
            timeout,
            storage_root,
            output_limits,
        }
    }

    pub fn storage_root(&self) -> &std::path::Path {
        &self.storage_root
    }

    /// Run a fixed VBoxManage operation represented as executable arguments.
    /// Callers must select the operation from the adapter's allow-list.
    pub async fn run(&self, args: &[String]) -> Result<VBoxCommandOutput, String> {
        let start = Instant::now();
        let mut command = Command::new(&self.vbox_manage_path);
        command
            .args(args)
            .env("VBOX_USER_HOME", &self.vbox_user_home)
            .current_dir(&self.vbox_user_home)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        let mut child = command
            .spawn()
            .map_err(|e| format!("could not run VBoxManage: {e}"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or("VBoxManage stdout was not captured")?;
        let stderr = child
            .stderr
            .take()
            .ok_or("VBoxManage stderr was not captured")?;
        let (stdout, stderr, status) = timeout(self.timeout, async {
            let out = read_bounded(stdout, self.output_limits.stdout_bytes);
            let err = read_bounded(stderr, self.output_limits.stderr_bytes);
            let wait = child.wait();
            tokio::join!(out, err, wait)
        })
        .await
        .map_err(|_| {
            format!(
                "VBoxManage timed out after {} seconds",
                self.timeout.as_secs()
            )
        })?;
        let status = status.map_err(|e| format!("could not wait for VBoxManage: {e}"))?;
        Ok(VBoxCommandOutput {
            exit_code: status.code(),
            stdout,
            stderr,
            duration: start.elapsed(),
        })
    }
}

async fn read_bounded<R: AsyncRead + Unpin>(mut reader: R, limit: usize) -> String {
    use tokio::io::AsyncReadExt;
    let mut captured = Vec::with_capacity(limit.min(8192));
    let mut buffer = [0u8; 8192];
    let mut truncated = false;
    loop {
        match reader.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let keep = (limit.saturating_sub(captured.len())).min(n);
                captured.extend_from_slice(&buffer[..keep]);
                if keep < n {
                    truncated = true;
                }
            }
        }
    }
    let mut value = String::from_utf8_lossy(&captured).into_owned();
    if truncated {
        value.push_str("\n[output truncated]");
    }
    value
}
