use std::{env, process, path::PathBuf};
use thot_core::agent_protocol::{AgentTask, AgentResult};
use executor::execute_task;
use vsock::{connect_to_vsock, VsockAddr};

mod executor;
mod vsock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlpineRequest {
    pub task: AgentTask,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlpineResponse {
    pub result: AgentResult,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 4 {
        eprintln!("Usage: {} <host_cid> <port> <token>", args[0]);
        process::exit(1);
    }

    let host_cid = args[1].parse::<u32>()?;
    let port = args[2].parse::<u16>()?;
    let token = args[3].clone();

    let addr = VsockAddr::new(host_cid, port);
    let mut connection = connect_to_vsock(host_cid, port)?;

    let handshake_request = AlpineRequest {
        task: AgentTask::Handshake(thot_core::agent_protocol::HandshakeRequest {
            protocol_version: 1,
            run_id: "".to_string(),
            student_unit_id: "".to_string(),
            expected_peer_cid: "".to_string(),
            nonce: "".to_string(),
            secret: token,
        }),
    };
    connection.write_all(&serde_json::to_vec(&handshake_request)?)?;

    let mut buffer = vec![0; 1024];
    let bytes_read = connection.read(&mut buffer)?;
    if bytes_read == 0 {
        eprintln!("Connection lost");
        process::exit(1);
    }

    let handshake_response: AlpineResponse = serde_json::from_slice(&buffer[..bytes_read])?;
    if let AlpineResponse { result } = handshake_response {
        if let AgentResult::Handshake(thot_core::agent_protocol::HandshakeResponse {
            protocol_version,
            ..
        }) = result
        {
            if protocol_version != 1 {
                eprintln!("Version mismatch");
                process::exit(1);
            }
        } else {
            eprintln!("Invalid handshake response");
            process::exit(1);
        }
    } else {
        eprintln!("Invalid handshake response");
        process::exit(1);
    }

    loop {
        let mut buffer = vec![0; 1024];
        let bytes_read = connection.read(&mut buffer)?;
        if bytes_read == 0 {
            eprintln!("Connection lost");
            process::exit(1);
        }

        let task_request: AlpineRequest = serde_json::from_slice(&buffer[..bytes_read])?;
        if let AlpineRequest { task } = task_request {
            let result = execute_task(task).await?;
            let task_result_response = AlpineResponse {
                result: AgentResult::TaskResult(result),
            };
            connection.write_all(&serde_json::to_vec(&task_result_response)?)?;
        } else {
            eprintln!("Invalid task request");
            process::exit(1);
        }
    }
}