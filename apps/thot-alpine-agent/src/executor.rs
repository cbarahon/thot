use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
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

pub async fn execute_task(task: Task) -> Result<SandboxTaskResult, AgentError> {
    if task.version != 1 {
        return Err(AgentError::InvalidVersion);
    }

    let cwd = task.workspace.join(task.cwd);
    if !cwd.starts_with(&task.workspace) {
        return Err(AgentError::MalformedPath);
    }

    if task.argv.is_empty() {
        return Err(AgentError::EmptyArgv);
    }

    if task.deadline <= Duration::from_secs(0) {
        return Err(AgentError::ExpiredDeadline);
    }

    if task.stdout_limit <= 0 || task.stderr_limit <= 0 {
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

    let stdout_reader = tokio::spawn(read_capped(stdout, task.stdout_limit as usize));
    let stderr_reader = tokio::spawn(read_capped(stderr, task.stderr_limit as usize));
    let (status, timed_out) = match timeout(task.deadline, child.wait()).await {
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
            task.deadline.as_secs_f64()
        ));
    }

    Ok(SandboxTaskResult {
        exit_code: status.and_then(|status| status.code()),
        stdout: String::from_utf8_lossy(&stdout.0).into_owned(),
        stderr: stderr_text,
        elapsed: task.deadline,
        timed_out,
        stdout_truncated: stdout.1,
        stderr_truncated: stderr.1,
        diagnostics: None,
    })
}

async fn terminate_child(child: &mut Command) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        // The test starts in its own process group so timeout also stops child tools it launched.
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
    let _ = child.kill().await;
}

struct Task {
    version: u32,
    workspace: PathBuf,
    cwd: PathBuf,
    argv: Vec<String>,
    deadline: Duration,
    stdout_limit: u64,
    stderr_limit: u64,
}