//! Versioned task/result contract between the generic engine and execution agents.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf};

pub const AGENT_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTask {
    pub protocol_version: u32,
    pub task_id: String,
    pub test_reference: String,
    pub test_name: String,
    #[serde(default)]
    pub parameters: BTreeMap<String, String>,
    pub timeout_seconds: u64,
    pub run_id: String,
    pub submission_id: String,
    /// Read-only path to the submitted deliverable, visible to the agent service.
    pub deliverable_path: PathBuf,
    /// The only project output directory the agent may modify for this task.
    pub test_directory: PathBuf,
    /// VM storage path owned by this run and submission, on the VirtualBox host.
    pub vm_storage_path: PathBuf,
    /// Engine-owned context returned by the previous task for this submission.
    #[serde(default)]
    pub context: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResult {
    pub protocol_version: u32,
    pub task_id: String,
    pub status: crate::test_results::TestStatus,
    #[serde(default)]
    pub stdout: String,
    #[serde(default)]
    pub stderr: String,
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub diagnostics: String,
    #[serde(default)]
    pub observation: Value,
    #[serde(default)]
    pub context: Value,
    #[serde(default)]
    pub evidence: Vec<String>,
    pub started_at: String,
    pub finished_at: String,
    pub duration_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCapabilities {
    pub protocol_version: u32,
    pub agent_id: String,
    pub agent_version: String,
    pub tests: Vec<String>,
    #[serde(default)]
    pub host_version: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTaskStatus {
    pub task_id: String,
    pub state: String,
    #[serde(default)]
    pub result: Option<AgentResult>,
    #[serde(default)]
    pub error: Option<String>,
}
