//! Purpose: Connects to a guest over SSH and reports host, memory, and filesystem information.
use ssh2::{CheckResult, KnownHostFileKind, KnownHostKeyFormat, Session};
use std::{
    io::Read,
    net::{TcpStream, ToSocketAddrs},
    time::Duration,
};
use thot_core::test_results::TestStatus;

use crate::test_execution::{TestInvocation, TestOutcome};

pub async fn run(invocation: &TestInvocation) -> TestOutcome {
    let params = &invocation.parameters;
    let host = params
        .get("host")
        .cloned()
        .unwrap_or_else(|| "127.0.0.1".into());
    let port = match params
        .get("port")
        .map(String::as_str)
        .unwrap_or("2222")
        .parse::<u16>()
    {
        Ok(port) if port > 0 => port,
        _ => return TestOutcome::error("ssh port must be a valid TCP port"),
    };
    let user = match params.get("user") {
        Some(user) if !user.is_empty() => user.clone(),
        _ => return TestOutcome::error("ssh test requires a user parameter"),
    };
    let password = match params.get("pass") {
        Some(password) => password.clone(),
        None => return TestOutcome::error("ssh test requires a pass parameter"),
    };
    let timeout = invocation.timeout.min(invocation.runtime.ssh_timeout);
    let output_limit = invocation.runtime.output_limit;
    let known_hosts = invocation.report_dir.join("known_hosts");
    let work = tokio::task::spawn_blocking(move || {
        collect_host_info(
            &host,
            port,
            &user,
            &password,
            timeout,
            output_limit,
            &known_hosts,
        )
    })
    .await;
    match work {
        Ok(Ok((stdout, stderr, observation, truncated))) => TestOutcome {
            status: TestStatus::Passed,
            stdout,
            stderr,
            exit_code: Some(0),
            diagnostics: if truncated {
                "SSH output was truncated at the configured output limit".into()
            } else {
                String::new()
            },
            observation,
            evidence: Vec::new(),
        },
        Ok(Err(error)) => TestOutcome {
            status: TestStatus::Failed,
            stdout: error.stdout,
            stderr: error.stderr,
            exit_code: error.exit_code,
            diagnostics: error.diagnostics,
            observation: serde_json::json!({}),
            evidence: Vec::new(),
        },
        Err(error) => TestOutcome::error(format!("SSH worker failed: {error}")),
    }
}

fn collect_host_info(
    host: &str,
    port: u16,
    user: &str,
    password: &str,
    timeout: Duration,
    output_limit: usize,
    known_hosts_path: &std::path::Path,
) -> Result<(String, String, serde_json::Value, bool), SshFailure> {
    let address = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("cannot resolve SSH host: {e}"))?
        .next()
        .ok_or("SSH host resolved to no addresses")?;
    let tcp = TcpStream::connect_timeout(&address, timeout)
        .map_err(|e| format!("SSH connection failed: {e}"))?;
    tcp.set_read_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    tcp.set_write_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    let mut session = Session::new().map_err(|e| format!("cannot create SSH session: {e}"))?;
    session.set_timeout(timeout.as_millis().min(u32::MAX as u128) as u32);
    session.set_tcp_stream(tcp);
    session
        .handshake()
        .map_err(|e| format!("SSH handshake failed: {e}"))?;
    verify_host_key(&session, host, port, known_hosts_path)?;
    session
        .userauth_password(user, password)
        .map_err(|e| format!("SSH authentication failed: {e}"))?;
    if !session.authenticated() {
        return Err("SSH authentication was rejected".into());
    }

    let mut channel = session
        .channel_session()
        .map_err(|e| format!("cannot open SSH channel: {e}"))?;
    channel.exec("set -e; LC_ALL=C; printf 'hostname: '; hostname; printf 'distribution: '; (. /etc/os-release 2>/dev/null && printf '%s' \"$PRETTY_NAME\") || uname -srmo; printf '\\n'; printf 'kernel: '; uname -r; printf 'memory_bytes: '; awk '/MemTotal/ {print $2 * 1024}' /proc/meminfo; printf 'df_output_begin\\n'; df -B1; printf '\\ndf_output_end\\n'")
        .map_err(|e| format!("cannot run host information commands: {e}"))?;
    let (stdout, out_truncated) = read_stream_limited(&mut channel, output_limit)
        .map_err(|e| format!("cannot read SSH output: {e}"))?;
    let (stderr, err_truncated) = read_stream_limited(channel.stderr(), output_limit)
        .map_err(|e| format!("cannot read SSH stderr: {e}"))?;
    channel
        .wait_close()
        .map_err(|e| format!("cannot close SSH channel: {e}"))?;
    let exit_code = channel
        .exit_status()
        .map_err(|e| format!("cannot read SSH command status: {e}"))?;
    if exit_code != 0 {
        return Err(SshFailure {
            stdout,
            stderr,
            exit_code: Some(exit_code),
            diagnostics: format!("remote host information command exited {exit_code}"),
        });
    }
    let df_output = stdout
        .split_once("df_output_begin\n")
        .and_then(|(_, tail)| {
            tail.split_once("\ndf_output_end")
                .map(|(body, _)| body.trim().to_owned())
        })
        .unwrap_or_default();
    let observation = serde_json::json!({
        "hostname": stdout.lines().find_map(|line| line.strip_prefix("hostname: ")).unwrap_or("").trim(),
        "distribution": stdout.lines().find_map(|line| line.strip_prefix("distribution: ")).unwrap_or("").trim(),
        "kernel": stdout.lines().find_map(|line| line.strip_prefix("kernel: ")).unwrap_or("").trim(),
        "memory_bytes": stdout.lines().find_map(|line| line.strip_prefix("memory_bytes: ")).and_then(|v| v.trim().parse::<u64>().ok()),
        "df_output": df_output,
        "output_truncated": out_truncated || err_truncated,
    });
    let yaml_stdout = serde_yaml::to_string(&observation)
        .map_err(|e| format!("cannot serialize SSH observation as YAML: {e}"))?;
    session
        .disconnect(None, "Thot test completed", None)
        .map_err(|e| format!("cannot close SSH session: {e}"))?;
    Ok((
        yaml_stdout,
        stderr,
        observation,
        out_truncated || err_truncated,
    ))
}

#[derive(Debug)]
struct SshFailure {
    stdout: String,
    stderr: String,
    exit_code: Option<i32>,
    diagnostics: String,
}

impl From<String> for SshFailure {
    fn from(diagnostics: String) -> Self {
        Self {
            stdout: String::new(),
            stderr: diagnostics.clone(),
            exit_code: None,
            diagnostics,
        }
    }
}

impl From<&str> for SshFailure {
    fn from(diagnostics: &str) -> Self {
        diagnostics.to_owned().into()
    }
}

fn read_stream_limited(reader: impl Read, limit: usize) -> std::io::Result<(String, bool)> {
    let mut reader = reader.take(limit as u64 + 1);
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    let truncated = bytes.len() > limit;
    bytes.truncate(limit);
    Ok((String::from_utf8_lossy(&bytes).into_owned(), truncated))
}

fn verify_host_key(
    session: &Session,
    host: &str,
    port: u16,
    path: &std::path::Path,
) -> Result<(), String> {
    let (key, key_type) = session
        .host_key()
        .ok_or("SSH server did not provide a host key")?;
    let mut known = session
        .known_hosts()
        .map_err(|e| format!("cannot initialize known-hosts store: {e}"))?;
    if path.exists() {
        known
            .read_file(path, KnownHostFileKind::OpenSSH)
            .map_err(|e| format!("cannot read per-delivery known-hosts file: {e}"))?;
    }
    match known.check_port(host, port, key) {
        CheckResult::Match => Ok(()),
        CheckResult::Mismatch => Err("SSH host key changed for this delivery endpoint".into()),
        CheckResult::Failure => Err("SSH host key could not be checked".into()),
        CheckResult::NotFound => {
            let host_port = format!("[{host}]:{port}");
            known
                .add(
                    &host_port,
                    key,
                    "Thot first-use pin",
                    KnownHostKeyFormat::from(key_type),
                )
                .map_err(|e| format!("cannot pin SSH host key: {e}"))?;
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("cannot create known-hosts directory: {e}"))?;
            }
            known
                .write_file(path, KnownHostFileKind::OpenSSH)
                .map_err(|e| format!("cannot persist SSH host key: {e}"))
        }
    }
}
