use crate::sandbox_contracts::PlatformCapability;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionStatus {
    Completed,
    TimedOut,
    ResourceExceeded,
    PlatformError,
    Cancelled,
    Interrupted,
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentRequest {
    Handshake(HandshakeRequest),
    StageFileChunk(StageFileChunkRequest),
    InstallPackage(InstallPackageRequest),
    ExecuteTask(ExecuteTaskRequest),
    Sync(SyncRequest),
    Acknowledge(AcknowledgeRequest),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentResponse {
    Handshake(HandshakeResponse),
    InstallPackageResult(InstallPackageResult),
    StageFileChunkResult(StageFileChunkResult),
    ExecuteTaskResult(ExecuteTaskResult),
    SyncResult(SyncResult),
    AcknowledgeResult(AcknowledgeResult),
    ProtocolError(ProtocolErrorResponse),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeRequest {
    pub protocol_version: u32,
    pub run_id: String,
    pub student_unit_id: String,
    pub expected_peer_cid: String,
    pub nonce: String,
    pub secret: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeResponse {
    pub protocol_version: u32,
    pub supported_versions: Vec<u32>,
    pub nonce: String,
    pub secret: String,
    /// Versioned operations/runtime features implemented by this guest agent.
    #[serde(default)]
    pub capabilities: Vec<PlatformCapability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageFileChunkRequest {
    pub task_id: String,
    pub file_path: String,
    pub total_size: u64,
    pub expected_sha256: String,
    pub chunk_index: u64,
    pub final_chunk: bool,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageFileChunkResult {
    pub task_id: String,
    pub complete: bool,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallPackageRequest {
    pub task_id: String,
    pub package_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteTaskRequest {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub working_dir: PathBuf,
    pub argv: Vec<String>,
    pub limits: ExecutionLimits,
    pub timeout: u32,
    pub stdout_limit: usize,
    pub stderr_limit: usize,
    pub is_preparation: bool,
    #[serde(default)]
    pub module: Option<ModuleLaunch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleLaunch {
    pub module_id: String,
    pub version: String,
    pub package_sha256: String,
    /// Relative module package path under /work/.thot/modules/.
    pub package_path: PathBuf,
    pub capabilities: Vec<PlatformCapability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionLimits {
    pub cpu_time_limit: u64,
    pub memory_limit: u64,
    pub output_limits: StreamOutputLimits,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamOutputLimits {
    pub stdout: usize,
    pub stderr: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallPackageResult {
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
pub struct ExecuteTaskResult {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub diagnostics: String,
    pub elapsed_ms: u128,
    pub status: ExecutionStatus,
    pub timed_out: bool,
    pub resource_exceeded: bool,
    pub platform_error: bool,
    pub cancelled: bool,
    pub interrupted: bool,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRequest {
    pub task_id: String,
    pub files: Vec<(PathBuf, u64, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
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
pub struct AcknowledgeRequest {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeResult {
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

pub const ALPINE_PROTOCOL_VERSION: u32 = 2;
pub const MAX_FRAME_BYTES: usize = 1_048_576;

pub fn write_frame<W: Write, T: Serialize>(writer: &mut W, data: &T) -> Result<(), ProtocolError> {
    let frame = serde_json::to_vec(data).map_err(ProtocolError::InvalidJson)?;
    if frame.is_empty() || frame.len() > MAX_FRAME_BYTES {
        return Err(ProtocolError::OversizedFrame);
    }
    let frame_size = frame.len() as u32;
    writer.write_all(&frame_size.to_be_bytes())?;
    writer.write_all(&frame)?;
    Ok(())
}

pub fn read_frame<R: Read, T: DeserializeOwned>(reader: &mut R) -> Result<T, ProtocolError> {
    let mut buffer = [0; 4];
    reader.read_exact(&mut buffer)?;
    let frame_size = u32::from_be_bytes(buffer);
    if frame_size == 0 || frame_size as usize > MAX_FRAME_BYTES {
        return Err(ProtocolError::OversizedFrame);
    }
    let mut frame = vec![0; frame_size as usize];
    reader.read_exact(&mut frame)?;
    let data: T = serde_json::from_slice(&frame)?;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode() {
        let request = AgentRequest::Handshake(HandshakeRequest {
            protocol_version: ALPINE_PROTOCOL_VERSION,
            run_id: "run123".to_string(),
            student_unit_id: "unit456".to_string(),
            expected_peer_cid: "peer789".to_string(),
            nonce: "nonce000".to_string(),
            secret: "secret111".to_string(),
        });
        let mut encoded = Vec::new();
        write_frame(&mut encoded, &request).unwrap();
        let decoded: AgentRequest = read_frame(&mut std::io::Cursor::new(&encoded)).unwrap();
        assert_eq!(
            serde_json::to_value(request).unwrap(),
            serde_json::to_value(decoded).unwrap()
        );
    }

    #[test]
    fn test_frame_boundary() {
        let request = AgentRequest::Handshake(HandshakeRequest {
            protocol_version: ALPINE_PROTOCOL_VERSION,
            run_id: "run123".to_string(),
            student_unit_id: "unit456".to_string(),
            expected_peer_cid: "peer789".to_string(),
            nonce: "nonce000".to_string(),
            secret: "secret111".to_string(),
        });
        let mut encoded = Vec::new();
        write_frame(&mut encoded, &request).unwrap();
        let decoded: Result<AgentRequest, ProtocolError> =
            read_frame(&mut std::io::Cursor::new(&encoded[..encoded.len() - 1]));
        assert!(matches!(decoded, Err(ProtocolError::Io(_))));
    }

    #[test]
    fn test_invalid_json() {
        let encoded = vec![0, 0, 0, 5, 0x68, 0x65, 0x6c, 0x6c, 0x6f];
        let decoded: Result<AgentRequest, ProtocolError> =
            read_frame(&mut std::io::Cursor::new(&encoded));
        assert!(matches!(decoded, Err(ProtocolError::InvalidJson(_))));
    }

    #[test]
    fn test_oversized_frame() {
        let request = "x".repeat(MAX_FRAME_BYTES + 1);
        let encoded = write_frame(&mut std::io::Cursor::new(vec![]), &request).unwrap_err();
        assert!(matches!(encoded, ProtocolError::OversizedFrame));
    }
}

#[derive(Debug)]
pub enum ProtocolError {
    InvalidFrameSize,
    InvalidJson(serde_json::Error),
    OversizedFrame,
    UnsupportedVersion(u32),
    MismatchedIdentity,
    ReplayedSession,
    Io(std::io::Error),
}

impl From<std::io::Error> for ProtocolError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for ProtocolError {
    fn from(error: serde_json::Error) -> Self {
        Self::InvalidJson(error)
    }
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidFrameSize => formatter.write_str("invalid frame size"),
            Self::InvalidJson(error) => write!(formatter, "invalid JSON: {error}"),
            Self::OversizedFrame => formatter.write_str("frame exceeds maximum size"),
            Self::UnsupportedVersion(version) => write!(formatter, "unsupported version {version}"),
            Self::MismatchedIdentity => formatter.write_str("message identity mismatch"),
            Self::ReplayedSession => formatter.write_str("replayed session"),
            Self::Io(error) => write!(formatter, "frame I/O error: {error}"),
        }
    }
}

impl std::error::Error for ProtocolError {}
