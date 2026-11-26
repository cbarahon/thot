use std::{env, fs, process};
use thot_core::sandbox_protocol::{
    ALPINE_PROTOCOL_VERSION, AcknowledgeResult, AgentRequest, AgentResponse, HandshakeResponse,
    read_frame, write_frame,
};

mod executor;
mod vsock;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.get(1).is_some_and(|arg| arg == "--version") {
        println!(
            "{} protocol={}",
            env!("CARGO_PKG_VERSION"),
            ALPINE_PROTOCOL_VERSION
        );
        return Ok(());
    }
    if args.len() != 6 {
        eprintln!(
            "Usage: {} <host-cid> <port> <token> <run-id> <student-unit-id>",
            args[0]
        );
        process::exit(64);
    }
    let host_cid: u32 = args[1].parse()?;
    let port: u32 = args[2].parse()?;
    let token_path = &args[3];
    let token_metadata = fs::metadata(token_path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if token_metadata.permissions().mode() & 0o077 != 0 {
            return Err("agent session secret must be readable only by its owner".into());
        }
    }
    let token = fs::read_to_string(token_path)?.trim().to_owned();
    let run_id = args[4].clone();
    let unit_id = args[5].clone();
    let mut stream = vsock::connect_to_vsock(host_cid, port)?;
    let mut staging = executor::StageStore::new();

    loop {
        let request: AgentRequest = read_frame(&mut stream)?;
        let response = match request {
            AgentRequest::Handshake(handshake) => {
                if handshake.protocol_version != ALPINE_PROTOCOL_VERSION
                    || handshake.run_id != run_id
                    || handshake.student_unit_id != unit_id
                    || handshake.expected_peer_cid != "2"
                    || !constant_time_eq(handshake.secret.as_bytes(), token.as_bytes())
                {
                    return Err("invalid or out-of-scope handshake".into());
                }
                AgentResponse::Handshake(HandshakeResponse {
                    protocol_version: ALPINE_PROTOCOL_VERSION,
                    supported_versions: vec![ALPINE_PROTOCOL_VERSION],
                    nonce: handshake.nonce,
                    secret: String::new(),
                    capabilities: executor::capabilities(),
                })
            }
            AgentRequest::ExecuteTaskWithProgress(task) => {
                let id = task.task_id.clone();
                let started = std::time::Instant::now();
                let execution = executor::execute_task(task);
                tokio::pin!(execution);
                loop {
                    tokio::select! {
                        result=&mut execution => break AgentResponse::ExecuteTaskResult(result?),
                        _=tokio::time::sleep(std::time::Duration::from_secs(5)) => {
                            write_frame(&mut stream,&AgentResponse::TaskProgress{task_id:id.clone(),elapsed_seconds:started.elapsed().as_secs()})?;
                        }
                    }
                }
            }
            AgentRequest::ExecuteTask(task) => {
                AgentResponse::ExecuteTaskResult(executor::execute_task(task).await?)
            }
            AgentRequest::InstallPackage(task) => {
                AgentResponse::InstallPackageResult(executor::install_package(task).await)
            }
            AgentRequest::Sync(task) => AgentResponse::SyncResult(executor::sync(task).await),
            AgentRequest::Acknowledge(task) => {
                AgentResponse::AcknowledgeResult(AcknowledgeResult {
                    task_id: task.task_id,
                    exit_code: Some(0),
                    stdout: String::new(),
                    stderr: String::new(),
                    diagnostics: String::new(),
                    elapsed_ms: 0,
                    timed_out: false,
                    truncated: false,
                })
            }
            AgentRequest::StageFileChunk(chunk) => {
                AgentResponse::StageFileChunkResult(staging.stage(chunk)?)
            }
        };
        write_frame(&mut stream, &response)?;
    }
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    let max = left.len().max(right.len());
    for index in 0..max {
        difference |= usize::from(*left.get(index).unwrap_or(&0) ^ *right.get(index).unwrap_or(&0));
    }
    difference == 0
}
