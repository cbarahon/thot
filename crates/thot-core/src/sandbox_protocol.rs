use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;
use std::path::PathBuf;
use std::str::FromStr;
use std::error::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlpineRequest {
    Handshake(HandshakeRequest),
    StageFiles(StageFilesRequest),
    ExecuteTask(ExecuteTaskRequest),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlpineResponse {
    Handshake(HandshakeResponse),
    TaskResult(TaskResult),
    ProtocolError(ProtocolErrorResponse),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeRequest {
    pub protocol_version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeResponse {
    pub protocol_version: u32,
    pub supported_versions: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageFilesRequest {
    pub task_id: String,
    pub files: Vec<(String, Vec<u8>)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteTaskRequest {
    pub task_id: String,
    pub run_id: String,
    pub submission_id: String,
    pub working_dir: String,
    pub argv: Vec<String>,
    pub limits: ExecutionLimits,
    pub timeout: u32,
    pub identity: RunScopedIdentity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionLimits {
    pub output_limits: StreamOutputLimits,
    pub cpu_time_limit: u64,
    pub memory_limit: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamOutputLimits {
    pub stdout: usize,
    pub stderr: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunScopedIdentity {
    pub user_id: String,
    pub group_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    pub task_id: String,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub diagnostics: String,
    pub elapsed_ms: u128,
    pub timed_out: bool,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolErrorResponse {
    pub error_message: String,
}

pub const ALPINE_PROTOCOL_VERSION: u32 = 1;

fn encode_frame(data: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let frame_size = data.len() as u32;
    if frame_size > 1_048_576 {
        return Err("Frame size exceeds maximum limit".into());
    }
    let mut frame = Vec::with_capacity(4 + data.len());
    frame.extend_from_slice(&frame_size.to_be_bytes());
    frame.extend_from_slice(data.as_bytes());
    Ok(frame)
}

fn decode_frame(frame: &[u8]) -> Result<String, Box<dyn Error>> {
    if frame.len() < 4 {
        return Err("Invalid frame size".into());
    }
    let frame_size = u32::from_be_bytes(frame[0..4].try_into().unwrap());
    if frame_size as usize + 4 > frame.len() {
        return Err("Frame size mismatch".into());
    }
    let data = std::str::from_utf8(&frame[4..4 + frame_size as usize])?;
    if !data.is_ascii() {
        return Err("Invalid JSON/UTF-8 encoding".into());
    }
    Ok(data.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode() {
        let request = AlpineRequest::Handshake(HandshakeRequest {
            protocol_version: ALPINE_PROTOCOL_VERSION,
        });
        let encoded = encode_frame(&serde_json::to_string(&request).unwrap()).unwrap();
        let decoded: AlpineRequest = serde_json::from_str(&decode_frame(&encoded).unwrap()).unwrap();
        assert_eq!(request, decoded);
    }
}