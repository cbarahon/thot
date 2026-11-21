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

    let capabilities = AgentCapabilities {
        protocol_version: AGENT_PROTOCOL_VERSION,
        agent_id: "alpine-agent".to_string(),
        agent_version: "1.0.0".to_string(),
        tests: vec!["test1".to_string(), "test2".to_string()],
        host_version: None,
        status: "ready".to_string(),
    };

    let capabilities_msg = serde_json::to_string(&capabilities).expect("Failed to serialize capabilities");
    connection.write_all(capabilities_msg.as_bytes()).expect("Failed to send capabilities");

    loop {
        let mut buffer = vec![0; 1024];
        let bytes_read = connection.read(&mut buffer).expect("Failed to read from host");
        if bytes_read == 0 {
            eprintln!("Connection lost");
            process::exit(1);
        }

        let task_msg = String::from_utf8_lossy(&buffer[..bytes_read]).to_string();
        let task: AgentTask = serde_json::from_str(&task_msg).expect("Failed to deserialize task");

        let result = execute_task(&task).expect("Failed to execute task");
        let result_msg = serde_json::to_string(&result).expect("Failed to serialize result");
        connection.write_all(result_msg.as_bytes()).expect("Failed to send result");
    }
}