//! Purpose: Runs external test processes with timeouts, output limits, and exit-status capture.
use std::{
    path::Path,
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::{Child, Command},
    time::timeout,
};

const DEFAULT_OUTPUT_LIMIT: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct ExternalProcessResult {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub elapsed: Duration,
    pub timed_out: bool,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

pub async fn run_external_process(
    executable: &Path,
    args: &[String],
    cwd: &Path,
    env: &[(String, String)],
    deadline: Duration,
    output_limit: usize,
) -> Result<ExternalProcessResult, String> {
    let started = Instant::now();
    let mut command = Command::new(executable);
    command
        .args(args)
        .current_dir(cwd)
        .envs(env.iter().cloned())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command
        .spawn()
        .map_err(|e| format!("cannot start external test: {e}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or("could not capture external stdout")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("could not capture external stderr")?;

    let stdout_reader = tokio::spawn(read_capped(stdout, output_limit.max(1)));
    let stderr_reader = tokio::spawn(read_capped(stderr, output_limit.max(1)));
    let (status, timed_out) = match timeout(deadline, child.wait()).await {
        Ok(status) => (
            Some(status.map_err(|e| format!("cannot wait for external test: {e}"))?),
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
        .map_err(|e| format!("cannot collect external stdout: {e}"))?;
    let stderr = stderr_reader
        .await
        .map_err(|e| format!("cannot collect external stderr: {e}"))?;
    let mut stderr_text = String::from_utf8_lossy(&stderr.0).into_owned();
    if timed_out {
        if !stderr_text.is_empty() && !stderr_text.ends_with('\n') {
            stderr_text.push('\n');
        }
        stderr_text.push_str(&format!(
            "external test timed out after {:.3} seconds",
            deadline.as_secs_f64()
        ));
    }
    Ok(ExternalProcessResult {
        exit_code: status.and_then(|status| status.code()),
        stdout: String::from_utf8_lossy(&stdout.0).into_owned(),
        stderr: stderr_text,
        elapsed: started.elapsed(),
        timed_out,
        stdout_truncated: stdout.1,
        stderr_truncated: stderr.1,
    })
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

async fn read_capped<R: AsyncRead + Unpin>(mut reader: R, limit: usize) -> (Vec<u8>, bool) {
    let mut captured = Vec::with_capacity(limit.min(8192));
    let mut truncated = false;
    let mut buffer = [0u8; 8192];
    loop {
        match reader.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let keep = limit.saturating_sub(captured.len()).min(n);
                captured.extend_from_slice(&buffer[..keep]);
                truncated |= keep < n;
            }
        }
    }
    if truncated {
        captured.extend_from_slice(b"\n[output truncated]");
    }
    (captured, truncated)
}

pub fn default_output_limit() -> usize {
    DEFAULT_OUTPUT_LIMIT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn captures_exit_code_stdout_and_stderr() {
        let result = run_external_process(
            Path::new("sh"),
            &[
                "-c".into(),
                "printf hello; printf warning >&2; exit 7".into(),
            ],
            Path::new("."),
            &[],
            Duration::from_secs(2),
            128,
        )
        .await
        .unwrap();
        assert_eq!(result.exit_code, Some(7));
        assert_eq!(result.stdout, "hello");
        assert_eq!(result.stderr, "warning");
        assert!(!result.timed_out);
    }

    #[tokio::test]
    async fn bounds_output_and_reports_timeout() {
        let bounded = run_external_process(
            Path::new("sh"),
            &["-c".into(), "printf 123456789".into()],
            Path::new("."),
            &[],
            Duration::from_secs(2),
            4,
        )
        .await
        .unwrap();
        assert!(bounded.stdout_truncated);
        assert!(bounded.stdout.starts_with("1234"));

        let timed = run_external_process(
            Path::new("sh"),
            &[
                "-c".into(),
                "printf partial; printf warning >&2; sleep 2".into(),
            ],
            Path::new("."),
            &[],
            Duration::from_millis(30),
            128,
        )
        .await
        .unwrap();
        assert!(timed.timed_out);
        assert_eq!(timed.exit_code, None);
        assert!(timed.stdout.starts_with("partial"));
        assert!(timed.stderr.starts_with("warning"));
        assert!(timed.stderr.contains("timed out"));
    }
}
