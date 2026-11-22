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
pub struct SandboxRunRequest {
    project_path: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RunRecord {
    run_id: String,
    project_id: String,
    created_at: String,
    delivery_directory: String,
    status: String,
    expedient_ids: Vec<String>,
    expedients: Vec<RunExpedientSummary>,
    error: Option<String>,
    warnings: Vec<String>,
}

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("Invalid request: {0}")]
    InvalidRequest(String),
    #[error("Conflict: {0}")]
    Conflict(String),
    #[error("Storage error: {0}")]
    StorageError(String),
    #[error("Provider error: {0}")]
    ProviderError(String),
}

#[derive(Debug)]
pub struct SandboxRunService {
    artifact_root: PathBuf,
    test_result_store: Arc<Mutex<TestResultStore>>,
    cancellation_signal: Arc<AtomicBool>,
    provider: Arc<dyn SandboxProvider>,
    run_store: Arc<Mutex<RunStore>>,
}

impl SandboxRunService {
    pub fn new(
        artifact_root: PathBuf,
        test_result_store: Arc<Mutex<TestResultStore>>,
        provider: Arc<dyn SandboxProvider>,
    ) -> Self {
        Self {
            artifact_root,
            test_result_store,
            cancellation_signal: Arc::new(AtomicBool::new(false)),
            provider,
            run_store: Arc::new(Mutex::new(RunStore::new())),
        }
    }

    pub async fn connect(&self, provider: Arc<dyn SandboxProvider>, artifact_root: PathBuf) -> Result<(), ServiceError> {
        self.provider = provider;
        self.artifact_root = artifact_root;
        self.run_store.lock().await.load_artifacts(&self.artifact_root).await?;
        self.reconcile_kvm_resources().await?;
        Ok(())
    }

    pub async fn start_run(&self, request: &SandboxRunRequest) -> Result<RunRecord, ServiceError> {
        let config_path = request.project_path.join(PROJECT_CONFIGURATION);
        let config_bytes = std::fs::read(&config_path).map_err(|e| ServiceError::StorageError(e.to_string()))?;
        let project = parse_configuration(&config_bytes).map_err(|e| ServiceError::ValidationError(e))?;
        let run_id = uuid::Uuid::new_v4().to_string();
        let run_record = RunRecord {
            run_id: run_id.clone(),
            project_id: project.project_id.clone(),
            created_at: SystemTime::now().duration_since(UNIX_EPOCH).map_or_else(|_| "".to_string(), |d| d.as_secs().to_string()),
            delivery_directory: request.project_path.to_string_lossy().to_string(),
            status: "queued".to_string(),
            expedient_ids: vec![],
            expedients: vec![],
            error: None,
            warnings: vec![],
        };
        self.run_store.lock().await.add_run(&run_id, &run_record).await?;
        task::spawn(async move {
            if !self.cancellation_signal.load(Ordering::SeqCst) {
                match self.execute_run(&run_id, &project).await {
                    Ok(_) => {}
                    Err(e) => {
                        self.run_store.lock().await.update_run_status(&run_id, "failed").await?;
                        self.run_store.lock().await.update_run_error(&run_id, e.to_string()).await?;
                    }
                }
            }
        });
        Ok(run_record)
    }

    async fn execute_run(&self, run_id: &str, project: &Project) -> Result<(), ServiceError> {
        let baseline_handle = self.provider.ensure_template().await?;
        let baseline_handle = self.provider.prepare_baseline(project, &baseline_handle).await?;
        for student_unit_id in project.student_units.iter() {
            let guest_files = self.collect_guest_files(project, student_unit_id)?;
            let env_handle = self.provider.create_environment(project, run_id, student_unit_id, &baseline_handle, &guest_files).await?;
            let result = self.provider.dispatch_task(project, student_unit_id, &env_handle).await?;
            self.persist_test_result(run_id, student_unit_id, &result).await?;
        }
        self.run_store.lock().await.update_run_status(run_id, "completed").await?;
        Ok(())
    }

    fn collect_guest_files(&self, project: &Project, student_unit_id: &str) -> Result<Vec<(String, Vec<u8>)>, ServiceError> {
        let mut guest_files = Vec::new();
        for input in project.inputs.iter() {
            match input.role.as_str() {
                "student-deliverable" => {
                    let path = project.input_path(input, student_unit_id)?;
                    let content = std::fs::read(&path).map_err(|e| ServiceError::StorageError(e.to_string()))?;
                    guest_files.push((input.path_template.clone(), content));
                }
                _ => {}
            }
        }
        Ok(guest_files)
    }

    async fn persist_test_result(&self, run_id: &str, student_unit_id: &str, result: &AgentResult) -> Result<(), ServiceError> {
        let test_result = TestResult {
            student_unit_id: student_unit_id.to_string(),
            status: TestStatus::from(result.status),
            stdout: result.stdout.clone(),
            stderr: result.stderr.clone(),
            exit_code: result.exit_code,
            duration_ms: result.duration_ms,
        };
        let mut test_results = self.test_result_store.lock().await;
        test_results.add_test_result(run_id, student_unit_id, &test_result).await?;
        Ok(())
    }

    async fn reconcile_kvm_resources(&self) -> Result<(), ServiceError> {
        // Implement reconciliation logic for KVM resources
        Ok(())
    }
}

struct RunStore {
    runs: HashMap<String, RunRecord>,
}

impl RunStore {
    fn new() -> Self {
        Self {
            runs: HashMap::new(),
        }
    }

    async fn load_artifacts(&self, artifact_root: &Path) -> Result<(), ServiceError> {
        // Implement logic to load existing run artifacts from the artifact root
        Ok(())
    }

    async fn add_run(&self, run_id: &str, run_record: &RunRecord) -> Result<(), ServiceError> {
        self.runs.insert(run_id.to_string(), run_record.clone());
        Ok(())
    }

    async fn update_run_status(&self, run_id: &str, status: &str) -> Result<(), ServiceError> {
        if let Some(run_record) = self.runs.get_mut(run_id) {
            run_record.status = status.to_string();
        }
        Ok(())
    }

    async fn update_run_error(&self, run_id: &str, error: String) -> Result<(), ServiceError> {
        if let Some(run_record) = self.runs.get_mut(run_id) {
            run_record.error = Some(error);
        }
        Ok(())
    }
}

struct Project {
    project_id: String,
    student_units: Vec<String>,
    inputs: Vec<Input>,
    tests: Vec<Test>,
}

struct Input {
    role: String,
    path_template: String,
    copy_to: String,
    read_only: bool,
}

struct Test {
    test_id: String,
    input_ids: Vec<String>,
    expected_output: String,
}

impl Input {
    fn input_path(&self, input: &Input, student_unit_id: &str) -> Result<PathBuf, ServiceError> {
        let path = self.path_template.replace("{student_id}", student_unit_id);
        Ok(PathBuf::from(&path))
    }
}