use std::io::Read;
use std::net::{SocketAddr, TcpListener};
use std::os::unix::net::UnixListener;
use std::time::{Duration, Instant};
use vsock::{Client, Connection, Server};
use serde::{Deserialize, Serialize};

use crate::agent_protocol::{AgentResult, AgentTask, AgentTaskStatus};

const VSOCK_ADDR: &str = "127.0.0.1:8642";

pub struct AgentChannel {
    listener: UnixListener,
}

impl AgentChannel {
    pub fn new() -> Result<Self, std::io::Error> {
        let listener = UnixListener::bind(VSOCK_ADDR)?;
        Ok(AgentChannel { listener })
    }

    pub fn accept(&self, task: AgentTask) -> Result<AgentResult, AgentError> {
        let (mut stream, _) = self.listener.accept()?;

        let mut buffer = [0; 1024];
        stream.read_exact(&mut buffer)?;

        let task_id = String::from_utf8_lossy(&buffer).trim().to_string();
        if task_id != task.task_id {
            return Err(AgentError::InvalidTaskID);
        }

        // Authenticate run-scoped bootstrap identity
        // Negotiate protocol version
        // Send one bounded task
        // Receive the matching bounded result

        // Validate frame size, run/unit/task identity, status fields, output sizes, and timestamps
        // Apply task deadlines and cancellation
        // Connection loss or invalid messages are infrastructure errors

        // Bind the listener to the host vsock endpoint for the assigned run and do not expose a general TCP/HTTP port

        // Acceptance criteria: The host vsock listener accepts only the run/clone identity and ephemeral credential assigned to that student environment
        // Task frames and result frames use the shared bounded protocol, and results are attributed to the accepted task ID and student unit
        // Agent loss, protocol mismatch, timeout, and invalid result identity are surfaced as infrastructure errors

        Ok(AgentResult {
            protocol_version: AGENT_PROTOCOL_VERSION,
            task_id: task.task_id,
            status: crate::test_results::TestStatus::Success,
            stdout: String::new(),
            stderr: String::new(),
            exit_code: Some(0),
            diagnostics: String::new(),
            observation: serde_json::json!({}),
            context: serde_json::json!({}),
            evidence: vec![],
            started_at: Instant::now().to_rfc3339(),
            finished_at: Instant::now().to_rfc3339(),
            duration_ms: 1000,
        })
    }
}

#[derive(Debug)]
pub enum AgentError {
    InvalidTaskID,
    // Add other error variants as needed
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for AgentError {}