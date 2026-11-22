use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tokio::task;

use crate::delivery_run_service::{RunRecord, RunExpedientSummary};
use crate::test_result_store::{persist_output, persist_report};
use crate::sandbox_config::{parse_configuration, PROJECT_CONFIGURATION};
use crate::sandbox_provider::{AgentTask, AgentResult, SandboxProvider, SandboxError};
use crate::test_results::{TestResult, TestStatus, TestOutcome};

#[derive(Debug, Serialize, Deserialize)]
pub struct SandboxRunRecord {
    pub run_id: String,
    pub project_id: String,
    pub created_at: String,
    pub status: String,
    pub observations: Vec<TestResult>,
    pub rubric_total: u32,
    pub diagnostics: Vec<String>,
    pub cleanup_state: String,
}

#[derive(Debug, Error)]
pub enum SandboxServiceError {
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    #[error("Conflict: {0}")]
    Conflict(String),
    #[error("Storage error: {0}")]
    StorageError(String),
    #[error("Provider error: {0}")]
    ProviderError(String),
    #[error("Cancellation: {0}")]
    Cancellation(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub schema_version: String,
    pub project_id: String,
    pub name: String,
    pub language: String,
    pub students_file: PathBuf,
    pub assignments_file: PathBuf,
    pub sandbox_platforms: Vec<SandboxPlatform>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxPlatform {
    pub id: String,
    pub kind: String,
    pub isolation_scope: String,
    pub lifecycle: Lifecycle,
    pub limits: Limits,
    pub network: NetworkPolicy,
    pub inputs: Vec<InputDeclaration>,
    pub tests: Vec<Test>,
    pub evaluation: Evaluation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lifecycle {
    pub template_name: String,
    pub template_image: TemplateImage,
    pub template_vm: TemplateVM,
    pub template_packages: Vec<String>,
    pub agent: Agent,
    pub project_clone: ProjectClone,
    pub preparation: Preparation,
    pub student_clone: StudentClone,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateImage {
    pub distribution: String,
    pub release: String,
    pub architecture: String,
    pub iso_flavor: String,
    pub iso_url: String,
    pub checksum_url: String,
    pub verify_checksum: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateVM {
    pub vcpu: u32,
    pub memory_mib: u64,
    pub disk_gib: u64,
    pub firmware: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub install_from: String,
    pub version: String,
    pub listen: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Clone {
    pub name: String,
    pub linked: bool,
    pub source_snapshot: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preparation {
    pub timeout_seconds: u64,
    pub packages: Vec<String>,
    pub scripts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputDeclaration {
    pub role: String,
    pub path_template: String,
    pub copy_to: String,
    pub read_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Test {
    pub test_id: String,
    pub input_ids: Vec<String>,
    pub expected_output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionLimits {
    pub cpu_seconds: u64,
    pub memory_mib: u64,
    pub disk_gib: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evaluation {
    pub rubric: Vec<RubricItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RubricItem {
    pub test_id: String,
    pub points: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxRunRequest {
    pub project_path: String,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRequest {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub working_dir: PathBuf,
    pub files: Vec<SyncFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncFile {
    pub file_path: String,
    pub file_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeRequest {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecutionStatus {
    Completed,
    TimedOut,
    ResourceExceeded,
    PlatformError,
    Cancelled,
    Interrupted,
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
    ExecuteTaskResult(ExecuteTaskResult),
    SyncResult(SyncResult),
    AcknowledgeResult(AcknowledgeResult),
    ProtocolError(ProtocolErrorResponse),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallPackageResult {
    pub task_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteTaskResult {
    pub task_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    pub task_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeResult {
    pub task_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolErrorResponse {
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResult {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: i32,
}

pub struct SandboxEngine {
    provider: Arc<dyn SandboxProvider>,
    cancellation_token: Arc<AtomicBool>,
}

impl SandboxEngine {
    pub fn new(provider: Arc<dyn SandboxProvider>) -> Self {
        Self {
            provider,
            cancellation_token: Arc::new(AtomicBool::new(false)),
        }
    }

    pub async fn run(&self, request: &SandboxRunRequest) -> Result<SandboxRunRecord, SandboxServiceError> {
        let project = parse_configuration(&request.project_path)?;
        let mut observations = Vec::new();
        let mut rubric_total = 0;

        for test in &project.tests {
            let task_id = format!("test_{}", test.test_id);
            let mut task = AgentTask::new(&task_id, &test.input_ids, &test.expected_output);

            match self.provider.execute_task(&task).await {
                Ok(result) => {
                    let observation = TestResult::new(&test.test_id, &result.stdout, &result.stderr, result.exit_code == 0);
                    observations.push(observation);
                    if result.exit_code == 0 {
                        rubric_total += test.points;
                    }
                }
                Err(err) => {
                    return Err(SandboxServiceError::ProviderError(err.to_string()));
                }
            }

            if self.cancellation_token.load(Ordering::SeqCst) {
                return Err(SandboxServiceError::Cancellation("Task cancelled by user".to_string()));
            }
        }

        let diagnostics = self.provider.get_diagnostics()?;
        let cleanup_state = self.provider.get_cleanup_state()?;

        Ok(SandboxRunRecord {
            run_id: request.project_path.clone(),
            project_id: project.project_id.clone(),
            created_at: SystemTime::now().to_rfc3339(),
            status: "Completed".to_string(),
            observations,
            rubric_total,
            diagnostics,
            cleanup_state,
        })
    }

    pub fn cancel(&self) {
        self.cancellation_token.store(true, Ordering::SeqCst);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolVersion {
    pub version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncFileChunk {
    pub file_path: String,
    pub file_hash: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncFileAck {
    pub file_path: String,
    pub file_hash: String,
    pub status: ExecutionStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncAck {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeError {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeComplete {
    pub task_id: String,
    pub run_id: String,
    pub student_unit_id: String,
    pub status: ExecutionStatus,
    pub result: Option<AgentResult>,
}

#[derive(Debug, Clone