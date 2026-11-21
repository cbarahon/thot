use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;
use std::path::PathBuf;
use std::str::FromStr;
use std::error::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlpineTask {
    pub protocol_version: u32,
    pub task_id: String,
    pub run_id: String,
    pub submission_id: String,
    pub working_dir: String,
    pub argv: Vec<String>,
    pub limits: ExecutionLimits,
    pub timeout: Duration,
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
pub struct AlpineResult {
    pub protocol_version: u32,
    pub task_id: String,
    pub status: TestStatus,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub diagnostics: String,
    pub started_at: String,
    pub finished_at: String,
    pub duration_ms: u128,
    pub error: Option<String>,
}

pub const ALPINE_PROTOCOL_VERSION: u32 = 2;

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
        let task = AlpineTask {
            protocol_version: ALPINE_PROTOCOL_VERSION,
            task_id: "task123".to_string(),
            run_id: "run456".to_string(),
            submission_id: "sub789".to_string(),
            working_dir: "/tmp".to_string(),
            argv: vec!["./myprogram".to_string(), "arg1".to_string(), "arg2".to_string()],
            limits: ExecutionLimits {
                output_limits: StreamOutputLimits {
                    stdout: 1024,
                    stderr: 512,
                },
                cpu_time_limit: 60,
                memory_limit: 1024 * 1024 * 1024,
            },
            timeout: Duration::from_secs(60),
            identity: RunScopedIdentity {
                user_id: "user1".to_string(),
                group_id: "group1".to_string(),
            },
        };
        let encoded = encode_frame(&serde_json::to_string(&task).unwrap()).unwrap();
        let decoded: AlpineTask = serde_json::from_str(&decode_frame(&encoded).unwrap()).unwrap();
        assert_eq!(task, decoded);
    }
}