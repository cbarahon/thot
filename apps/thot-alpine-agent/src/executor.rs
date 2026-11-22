use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, CommandExt};
use tokio::time::timeout;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("invalid task version")]
    InvalidVersion,
    #[error("malformed path")]
    MalformedPath,
    #[error("empty argv")]
    EmptyArgv,
    #[error("expired deadline")]
    ExpiredDeadline,
    #[error("invalid limits")]
    InvalidLimits,
    #[error("infrastructure error: {0}")]
    InfrastructureError(String),
    #[error("protocol error: {0}")]
    ProtocolError(String),
}

pub struct SandboxTaskResult {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub elapsed: Duration,
    pub timed_out: bool,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub diagnostics: Option<String>,
}

pub async fn execute_task(task: ExecuteTaskRequest) -> Result<SandboxTaskResult, AgentError> {
    if task.protocol_version != 1 {
        return Err(AgentError::InvalidVersion);
    }

    let cwd = task.working_dir.parse::<PathBuf>().map_err(|_| AgentError::MalformedPath)?;
    if !cwd.starts_with("/work") {
        return Err(AgentError::MalformedPath);
    }

    if task.argv.is_empty() {
        return Err(AgentError::EmptyArgv);
    }

    if task.timeout <= 0 {
        return Err(AgentError::ExpiredDeadline);
    }

    if task.output_limits.stdout_limit <= 0 || task.output_limits.stderr_limit <= 0 {
        return Err(AgentError::InvalidLimits);
    }

    let mut command = Command::new(&task.argv[0]);
    command
        .args(&task.argv[1..])
        .current_dir(&cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command
        .spawn()
        .map_err(|e| AgentError::InfrastructureError(e.to_string()))?;

    let stdout = child
        .stdout
        .take()
        .ok_or(AgentError::InfrastructureError("could not capture stdout".to_string()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or(AgentError::InfrastructureError("could not capture stderr".to_string()))?;

    let stdout_reader = tokio::spawn(read_capped(stdout, task.output_limits.stdout_limit as usize));
    let stderr_reader = tokio::spawn(read_capped(stderr, task.output_limits.stderr_limit as usize));
    let (status, timed_out) = match timeout(Duration::from_secs(task.timeout as u64), child.wait()).await {
        Ok(status) => (
            Some(status.map_err(|e| AgentError::InfrastructureError(e.to_string()))?),
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
        .map_err(|e| AgentError::InfrastructureError(e.to_string()))?;
    let stderr = stderr_reader
        .await
        .map_err(|e| AgentError::InfrastructureError(e.to_string()))?;
    let mut stderr_text = String::from_utf8_lossy(&stderr.0).into_owned();
    if timed_out {
        if !stderr_text.is_empty() && !stderr_text.ends_with('\n') {
            stderr_text.push('\n');
        }
        stderr_text.push_str(&format!(
            "guest command timed out after {:.3} seconds",
            task.timeout as f64
        ));
    }

    Ok(SandboxTaskResult {
        exit_code: status.and_then(|status| status.code()),
        stdout: String::from_utf8_lossy(&stdout.0).into_owned(),
        stderr: stderr_text,
        elapsed: Duration::from_secs(task.timeout as u64),
        timed_out,
        stdout_truncated: stdout.1,
        stderr_truncated: stderr.1,
        diagnostics: None,
    })
}

async fn read_capped(mut reader: impl AsyncReadExt + Unpin, limit: usize) -> Result<(Vec<u8>, bool), AgentError> {
    let mut buffer = vec![0; limit];
    let n = reader.read(&mut buffer).await.map_err(|e| AgentError::InfrastructureError(e.to_string()))?;
    buffer.truncate(n);
    Ok((buffer, n < limit))
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteTaskRequest {
    pub protocol_version: u32,
    pub working_dir: String,
    pub argv: Vec<String>,
    pub timeout: u32,
    pub output_limits: OutputLimits,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputLimits {
    pub stdout_limit: u64,
    pub stderr_limit: u64,
}