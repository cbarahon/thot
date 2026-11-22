//! Purpose: Exports host-side process and VirtualBox adapters used by the engine.

pub mod external;
pub mod kvm_alpine;
pub mod vbox;

use std::path::Path;
use std::sync::Arc;
use std::task::Poll;
use std::future::Future;
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderError {
    pub code: u32,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HostCapabilities {
    pub platform: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TemplateHandle {
    pub owner_uuid: Uuid,
    pub domain_uuid: Uuid,
    pub name: String,
    pub identity: String,
    pub provenance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BaselineHandle {
    pub owner_uuid: Uuid,
    pub domain_uuid: Uuid,
    pub name: String,
    pub identity: String,
    pub provenance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StudentEnvironmentHandle {
    pub owner_uuid: Uuid,
    pub domain_uuid: Uuid,
    pub name: String,
    pub identity: String,
    pub provenance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StagedFile {
    pub guest_path: String,
    pub bytes: Vec<u8>,
    pub sha256: String,
    pub readonly: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecutionLimits {
    pub cpu: u32,
    pub memory: u64,
}

pub trait SandboxProvider: Send + Sync {
    type TaskResult;

    fn preflight(&self, project: &Project) -> Result<HostCapabilities, ProviderError>;

    fn ensure_template(&self, project: &Project) -> Result<TemplateHandle, ProviderError>;

    fn prepare_baseline(&self, project: &Project, template: &TemplateHandle, project_root: &Path) -> Result<BaselineHandle, ProviderError>;

    fn create_environment(&self, project: &Project, run_id: &str, student_id: &str, baseline: &BaselineHandle, files: &[StagedFile]) -> Result<StudentEnvironmentHandle, ProviderError>;

    fn execute_task(&self, environment: &StudentEnvironmentHandle, task: &ExecuteTaskRequest) -> Result<Self::TaskResult, ProviderError>;

    fn cancel_environment(&self, environment: &StudentEnvironmentHandle) -> Result<(), ProviderError>;

    fn destroy_environment(&self, environment: &StudentEnvironmentHandle) -> Result<(), ProviderError>;

    fn reconcile_resources(&self) -> Result<(), ProviderError>;
}

pub async fn sha256_digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}