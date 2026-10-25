use std::process::{Command, Stdio};
use std::time::Duration;
use tokio::process::CommandExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

const DEFAULT_OUTPUT_LIMIT: usize = 1 * 1024 * 1024; // 1 MiB

#[derive(Debug)]
pub struct ExternalProcessResult {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub elapsed: Duration,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

pub async fn run_external_process(
    executable: &str,
    args: &[&str],
    cwd: Option<&str>,
    env: Option<&[(String, String)]>,
    timeout: Option<Duration>,
    stdout_limit: Option<usize>,
    stderr_limit: Option<usize>,
) -> ExternalProcessResult {
    let mut command = Command::new(executable);
    command.args(args);

    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }

    if let Some(env) = env {
        for (key, value) in env {
            command.env(key, value);
        }
    }

    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start external process");

    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();

    let stdout_limit = stdout_limit.unwrap_or(DEFAULT_OUTPUT_LIMIT);
    let stderr_limit = stderr_limit.unwrap_or(DEFAULT_OUTPUT_LIMIT);

    let mut stdout_buffer = Vec::with_capacity(stdout_limit);
    let mut stderr_buffer = Vec::with_capacity(stderr_limit);

    let mut stdout_truncated = false;
    let mut stderr_truncated = false;

    tokio::select! {
        result = timeout(timeout.unwrap_or(Duration::MAX), async move {
            let mut buf = [0; 1024];
            loop {
                match stdout.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        if stdout_buffer.len() + n > stdout_limit {
                            stdout_truncated = true;
                            break;
                        }
                        stdout_buffer.extend_from_slice(&buf[..n]);
                    }
                    Err(_) => break,
                }
            }

            loop {
                match stderr.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        if stderr_buffer.len() + n > stderr_limit {
                            stderr_truncated = true;
                            break;
                        }
                        stderr_buffer.extend_from_slice(&buf[..n]);
                    }
                    Err(_) => break,
                }
            }

            child.wait().await.expect("Failed to wait for external process")
        }) => {
            match result {
                Ok(status) => ExternalProcessResult {
                    exit_code: status.code().unwrap_or(-1),
                    stdout: stdout_buffer,
                    stderr: stderr_buffer,
                    elapsed: status.elapsed(),
                    stdout_truncated,
                    stderr_truncated,
                },
                Err(_) => ExternalProcessResult {
                    exit_code: -1,
                    stdout: stdout_buffer,
                    stderr: stderr_buffer,
                    elapsed: Duration::ZERO,
                    stdout_truncated,
                    stderr_truncated,
                },
            }
        }
    }
}
