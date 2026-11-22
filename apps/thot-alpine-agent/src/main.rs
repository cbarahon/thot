use std::{env, process, path::PathBuf};
use thot_core::agent_protocol::{AgentTask, AgentResult};
use executor::execute_task;
use vsock::{connect, VsockAddr};

mod executor;
mod vsock;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 4 {
        eprintln!("Usage: {} <host_cid> <port> <token>", args[0]);
        process::exit(1);
    }

    let host_cid = args[1].parse::<u64>().expect("Invalid host CID");
    let port = args[2].parse::<u32>().expect("Invalid port");
    let token = args[3].clone();

    let addr = VsockAddr::new(host_cid, port);
    let mut connection = connect(addr).expect("Failed to connect to host");

    let handshake_request = AlpineRequest::Handshake(HandshakeRequest {
        protocol_version: 1,
    });
    connection.write_all(&serde_json::to_vec(&handshake_request).expect("Failed to serialize handshake request")).expect("Failed to send handshake request"));

    let mut buffer = vec![0; 1024];
    let bytes_read = connection.read(&mut buffer).expect("Failed to read from host");
    if bytes_read == 0 {
        eprintln!("Connection lost");
        process::exit(1);
    }

    let handshake_response: AlpineResponse = serde_json::from_slice(&buffer[..bytes_read]).expect("Failed to deserialize handshake response");
    if let AlpineResponse::Handshake(HandshakeResponse { protocol_version, .. }) = handshake_response {
        if protocol_version != 1 {
            eprintln!("Version mismatch");
            process::exit(1);
        }
    } else {
        eprintln!("Invalid handshake response");
        process::exit(1);
    }

    loop {
        let mut buffer = vec![0; 1024];
        let bytes_read = connection.read(&mut buffer).expect("Failed to read from host");
        if bytes_read == 0 {
            eprintln!("Connection lost");
            process::exit(1);
        }

        let task_request: AlpineRequest = serde_json::from_slice(&buffer[..bytes_read]).expect("Failed to deserialize task request");
        if let AlpineRequest::ExecuteTask(task_request) = task_request {
            let result = execute_task(task_request).expect("Failed to execute task");
            let task_result_response = AlpineResponse::TaskResult(result);
            connection.write_all(&serde_json::to_vec(&task_result_response).expect("Failed to serialize task result response")).expect("Failed to send task result response");
        } else {
            eprintln!("Invalid task request");
            process::exit(1);
        }
    }
}