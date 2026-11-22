use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::os::unix::net::UnixListener;
use std::time::{Duration, Instant};
use vsock::{Client, Connection, Server};
use serde::{Deserialize, Serialize};
use tokio::task;
use tokio::time::sleep;
use sha2::{Sha256, Digest};
use hmac::{Hmac, Mac};
use base64;

use crate::agent_protocol::{AgentRequest, AgentResponse, AgentTask, AgentTaskStatus};
use crate::sandbox_protocol::{ExecutionLimits, HandshakeRequest, HandshakeResponse, StageFileChunkRequest};

use crate::error::ProviderError;

const VSOCK_ADDR: u32 = 12345; // Placeholder for the host vsock port
const TOKEN_LENGTH: usize = 32; // Placeholder for the token length
const PROTOCOL_VERSION: u32 = 1; // Placeholder for the protocol version
const FRAME_CAP: usize = 1024 * 1024; // Placeholder for the frame cap

pub struct AgentChannel {
    listener: Server,
}

impl AgentChannel {
    pub fn bind(port: u32) -> Result<AgentChannel, ProviderError> {
        let listener = Server::bind_host_port(port, 0)?;
        Ok(AgentChannel { listener })
    }

    pub async fn accept_session(expected_peer_cid: u32, run_id: &str, student_id: &str, one_use_secret: &[u8], timeout: Duration) -> Result<AgentSession, ProviderError> {
        let mut client = self.listener.accept().await?;

        let peer_cid = client.peer_cid();
        if peer_cid != expected_peer_cid {
            return Err(ProviderError::SessionMismatch);
        }

        let mut received_version = [0; std::mem::size_of::<u32>()];
        client.read_exact(&mut received_version).await?;
        let received_version = u32::from_le_bytes(received_version);

        if received_version != PROTOCOL_VERSION {
            return Err(ProviderError::UnsupportedProtocolVersion);
        }

        let mut received_run_id = String::new();
        client.read_line(&mut received_run_id).await?;
        if received_run_id.trim() != run_id {
            return Err(ProviderError::RunMismatch);
        }

        let mut received_student_id = String::new();
        client.read_line(&mut received_student_id).await?;
        if received_student_id.trim() != student_id {
            return Err(ProviderError::StudentMismatch);
        }

        let mut received_secret = [0; TOKEN_LENGTH];
        client.read_exact(&mut received_secret).await?;
        if !hmac::assert_matches!(Hmac::<Sha256>::new_from_slice(one_use_secret), Ok(hmac)) || !hmac.verify_slice(&received_secret) {
            return Err(ProviderError::SecretMismatch);
        }

        let session = AgentSession { client, expected_peer_cid, run_id: run_id.to_string(), student_id: student_id.to_string(), one_use_secret: one_use_secret.to_vec(), timeout };
        Ok(session)
    }
}

pub struct AgentSession {
    client: Client,
    expected_peer_cid: u32,
    run_id: String,
    student_id: String,
    one_use_secret: Vec<u8>,
    timeout: Duration,
}

impl AgentSession {
    pub async fn send_request(&mut self, request: AgentRequest) -> Result<(), ProviderError> {
        let frame = serde_json::to_string(&request).map_err(|_| ProviderError::SerializationFailed)?;
        let frame_bytes = frame.as_bytes();
        if frame_bytes.len() > FRAME_CAP {
            return Err(ProviderError::FrameTooLarge);
        }
        self.client.write_all(frame_bytes).await?;
        Ok(())
    }

    pub async fn read_response(&mut self) -> Result<AgentResponse, ProviderError> {
        let mut response_buffer = String::new();
        self.client.read_line(&mut response_buffer).await?;
        let response: AgentResponse = serde_json::from_str(&response_buffer).map_err(|_| ProviderError::DeserializationFailed)?;
        Ok(response)
    }
}

#[derive(Debug)]
pub enum ProviderError {
    SessionMismatch,
    UnsupportedProtocolVersion,
    RunMismatch,
    StudentMismatch,
    SecretMismatch,
    SerializationFailed,
    DeserializationFailed,
    FrameTooLarge,
    ConnectionFailed,
    Timeout,
    InfrastructureFailure,
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for ProviderError {}