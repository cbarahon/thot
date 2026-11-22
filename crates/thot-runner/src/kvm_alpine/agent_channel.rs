use std::io::Read;
use std::net::{SocketAddr, TcpListener};
use std::os::unix::net::UnixListener;
use std::time::{Duration, Instant};
use vsock::{Client, Connection, Server};
use serde::{Deserialize, Serialize};

use crate::agent_protocol::{AgentResult, AgentTask, AgentTaskStatus};

const VSOCK_ADDR: u32 = 12345; // Placeholder for the host vsock port
const TOKEN_LENGTH: usize = 32; // Placeholder for the token length
const PROTOCOL_VERSION: u32 = 1; // Placeholder for the protocol version

pub struct AgentChannel {
    listener: Server,
}

impl AgentChannel {
    pub fn new() -> Result<Self, std::io::Error> {
        let listener = Server::bind_host_port(VSOCK_ADDR, 0)?;
        Ok(AgentChannel { listener })
    }

    pub fn accept(&self, task: AgentTask) -> Result<AgentResult, AgentError> {
        let mut client = self.listener.accept()?;

        // Generate a fresh per-environment token
        let token = generate_token(TOKEN_LENGTH);

        // Send the token to the guest
        client.send_all(token.as_bytes())?;

        // Receive the token from the guest
        let mut received_token = vec![0; TOKEN_LENGTH];
        client.recv_exact(&mut received_token)?;

        if received_token != token {
            return Err(AgentError::InvalidToken);
        }

        // Negotiate protocol version
        let mut received_version = [0; std::mem::size_of::<u32>()];
        client.recv_exact(&mut received_version)?;
        let received_version = u32::from_le_bytes(received_version);

        if received_version != PROTOCOL_VERSION {
            return Err(AgentError::UnsupportedProtocolVersion);
        }

        // Send one bounded task
        client.send_all(&task.to_vec())?;

        // Receive the matching bounded result
        let mut result_buffer = Vec::new();
        client.read_to_end(&mut result_buffer)?;

        let result = AgentResult::from_vec(&result_buffer)?;

        // Validate frame size, run/unit/task identity, status fields, output sizes, and timestamps
        // Apply task deadlines and cancellation
        // Connection loss or invalid messages are infrastructure errors

        Ok(result)
    }
}

fn generate_token(length: usize) -> String {
    use rand::RngCore;
    use rand::rngs::OsRng;

    let mut token = vec![0u8; length];
    OsRng.fill_bytes(&mut token);
    base64::encode(token)
}

#[derive(Debug)]
pub enum AgentError {
    InvalidTaskID,
    InvalidToken,
    UnsupportedProtocolVersion,
    // Add other error variants as needed
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for AgentError {}