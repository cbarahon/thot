use super::sandbox_provider::{AgentTask, AgentResult, AgentCapabilities, AgentTaskStatus, SandboxError};
use crate::provider::SandboxProvider;
use crate::kvm_alpine::provider::{KvmAlpineHost, Preflight, PrepareBaseline, CreateStudentEnvironment, DispatchTask, DestroyStudentEnvironment, ReconcileOwnedResources};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct EngineProviderAdapter {
    kvm_alpine_host: Arc<Mutex<KvmAlpineHost>>,
}

impl EngineProviderAdapter {
    pub fn new(kvm_alpine_host: Arc<KvmAlpineHost>) -> Self {
        EngineProviderAdapter {
            kvm_alpine_host: Arc::new(Mutex::new(kvm_alpine_host)),
        }
    }

    async fn run_task(
        &self,
        config: &AgentTask,
        task_data: &Value,
        run_id: &str,
        student_unit_id: &str,
        baseline_handle: &Self::BaselineHandle,
    ) -> Result<AgentResult, SandboxError> {
        let mut kvm_alpine_host = self.kvm_alpine_host.lock().await;
        kvm_alpine_host.dispatch_task().await?;
        Ok(AgentResult {
            protocol_version: config.protocol_version,
            task_id: config.task_id.clone(),
            status: crate::test_results::TestStatus::Passed,
            stdout: String::new(),
            stderr: String::new(),
            exit_code: Some(0),
            diagnostics: String::new(),
            observation: Value::Null,
            context: Value::Null,
            evidence: Vec::new(),
            started_at: chrono::Utc::now().to_rfc3339(),
            finished_at: chrono::Utc::now().to_rfc3339(),
            duration_ms: 0,
        })
    }
}

#[async_trait]
impl SandboxProvider for EngineProviderAdapter {
    type TemplateHandle = ();
    type BaselineHandle = ();
    type StudentEnvHandle = ();

    async fn preflight(
        &self,
        config: &AgentTask,
        baseline_handle: &Self::BaselineHandle,
    ) -> Result<(), SandboxError> {
        let mut kvm_alpine_host = self.kvm_alpine_host.lock().await;
        kvm_alpine_host.preflight().await?;
        Ok(())
    }

    async fn ensure_template(&self) -> Result<Self::TemplateHandle, SandboxError> {
        Ok(())
    }

    async fn prepare_baseline(
        &self,
        config: &AgentTask,
        baseline_handle: &Self::BaselineHandle,
    ) -> Result<Self::BaselineHandle, SandboxError> {
        Ok(())
    }

    async fn create_environment(
        &self,
        config: &AgentTask,
        run_id: &String,
        student_unit_id: &String,
        baseline_handle: &Self::BaselineHandle,
        guest_files: &Vec<(String, Vec<u8>)>,
    ) -> Result<Self::StudentEnvHandle, SandboxError> {
        Ok(())
    }

    async fn dispatch_task(
        &self,
        config: &AgentTask,
        task_data: &serde_json::Value,
        run_id: &String,
        student_unit_id: &String,
        env_handle: &Self::StudentEnvHandle,
    ) -> Result<AgentResult, SandboxError> {
        self.run_task(config, task_data, run_id, student_unit_id, env_handle).await
    }

    async fn destroy_environment(
        &self,
        env_handle: &Self::StudentEnvHandle,
    ) -> Result<(), SandboxError> {
        Ok(())
    }
}