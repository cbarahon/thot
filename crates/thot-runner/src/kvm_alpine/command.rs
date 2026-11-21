use std::path::Path;
use std::process::{Command, Child, Stdio};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

struct BoundedOutput {
    data: Vec<u8>,
    truncated: bool,
}

async fn read_capped(mut reader: impl AsyncReadExt, limit: usize) -> Result<BoundedOutput, String> {
    let mut data = vec![0; limit];
    let len = reader.read(&mut data).await.map_err(|e| format!("read failed: {e}"))?;
    data.truncate(len);
    Ok(BoundedOutput {
        data,
        truncated: len == limit,
    })
}

async fn run_host_command(
    executable: &Path,
    argv: &[&str],
    cwd: &Path,
    deadline: Duration,
    output_limit: usize,
) -> Result<(Option<i32>, BoundedOutput, Duration, bool, bool), String> {
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
        .map_err(|e| format!("cannot start host command: {e}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or("could not capture host command stdout")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("could not capture host command stderr")?;

    let stdout_reader = tokio::spawn(read_capped(stdout, output_limit));
    let stderr_reader = tokio::spawn(read_capped(stderr, output_limit));
    let (status, timed_out) = match timeout(deadline, child.wait()).await {
        Ok(status) => (
            Some(status.map_err(|e| format!("cannot wait for host command: {e}"))?),
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
        .map_err(|e| format!("cannot collect host command stdout: {e}"))?;
    let stderr = stderr_reader
        .await
        .map_err(|e| format!("cannot collect host command stderr: {e}"))?;
    Ok((status.and_then(|status| status.code()), stdout, started.elapsed(), timed_out, stdout.truncated || stderr.truncated))
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