use async_trait::async_trait;
use std::path::PathBuf;
use serde::{Serialize, Deserialize};
use thiserror::Error;
use thot_runner::{SandboxProvider, ProviderError as SandboxError};
use thot_core::sandbox_protocol::{ExecutionStatus, AgentRequest, AgentResponse, HandshakeRequest, HandshakeResponse, StageFileChunkRequest, InstallPackageRequest, ExecuteTaskRequest};

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
    pub status: ExecutionStatus,
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

#[async_trait]
pub trait SandboxProvider {
    type TemplateHandle;
    type BaselineHandle;
    type StudentEnvHandle;

    async fn preflight(
        &self,
        config: &AgentTask,
        baseline_handle: &Self::BaselineHandle,
    ) -> Result<(), SandboxError>;

    async fn ensure_template(&self) -> Result<Self::TemplateHandle, SandboxError>;

    async fn prepare_baseline(
        &self,
        config: &AgentTask,
        baseline_handle: &Self::BaselineHandle,
    ) -> Result<Self::BaselineHandle, SandboxError>;

    async fn create_environment(
        &self,
        config: &AgentTask,
        run_id: &String,
        student_unit_id: &String,
        baseline_handle: &Self::BaselineHandle,
        guest_files: &Vec<(String, Vec<u8>)>,
    ) -> Result<Self::StudentEnvHandle, SandboxError>;

    async fn dispatch_task(
        &self,
        config: &AgentTask,
        task_data: &serde_json::Value,
        run_id: &String,
        student_unit_id: &String,
        env_handle: &Self::StudentEnvHandle,
    ) -> Result<AgentResult, SandboxError>;

    async fn destroy_environment(
        &self,
        env_handle: &Self::StudentEnvHandle,
    ) -> Result<(), SandboxError>;

    async fn reconcile_resources(&self) -> Result<(), SandboxError>;
}