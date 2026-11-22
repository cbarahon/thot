use std::sync::Arc;
use tokio::sync::Mutex;

use crate::error::ProviderError;
use crate::kvm_alpine::modules::{Preflight, PrepareBaseline, CreateStudentEnvironment, DispatchTask, DestroyStudentEnvironment, ReconcileOwnedResources};
use crate::kvm_alpine::settings::KvmAlpineSettings;
use crate::kvm_alpine::provider::KvmAlpineHost;
use crate::kvm_alpine::ownership::OwnershipRegistry;
use crate::kvm_alpine::transport::Transport;
use crate::kvm_alpine::isolation::Isolation;

pub struct KvmAlpineProvider {
    settings: Arc<KvmAlpineSettings>,
    registry: OwnershipRegistry,
}

impl KvmAlpineProvider {
    pub fn new(settings: KvmAlpineSettings) -> Result<Self, ProviderError> {
        Ok(KvmAlpineProvider {
            settings: Arc::new(settings),
            registry: OwnershipRegistry::new(),
        })
    }

    pub async fn preflight(&self) -> Result<(), ProviderError> {
        let host = KvmAlpineHost::new(self.settings.clone())?;
        host.preflight().await
    }

    pub async fn prepare_baseline(&self) -> Result<(), ProviderError> {
        let host = KvmAlpineHost::new(self.settings.clone())?;
        host.prepare_baseline().await
    }

    pub async fn create_student_environment(&self) -> Result<(), ProviderError> {
        let host = KvmAlpineHost::new(self.settings.clone())?;
        host.create_student_environment().await
    }

    pub async fn dispatch_task(&self, task_id: String, run_id: String, student_unit_id: String, working_dir: std::path::PathBuf, argv: Vec<String>, limits: crate::kvm_alpine::execution::ExecutionLimits, timeout: u32, stdout_limit: usize, stderr_limit: usize, is_preparation: bool) -> Result<crate::kvm_alpine::execution::TaskResult, ProviderError> {
        let host = KvmAlpineHost::new(self.settings.clone())?;
        let transport = Transport::new();
        let isolation = Isolation::new();
        host.dispatch_task(task_id, run_id, student_unit_id, working_dir, argv, limits, timeout, stdout_limit, stderr_limit, is_preparation, transport, isolation).await
    }

    pub async fn destroy_student_environment(&self) -> Result<(), ProviderError> {
        let host = KvmAlpineHost::new(self.settings.clone())?;
        host.destroy_student_environment().await
    }

    pub async fn reconcile_owned_resources(&self) -> Result<(), ProviderError> {
        let host = KvmAlpineHost::new(self.settings.clone())?;
        host.reconcile_owned_resources().await
    }
}

// Re-export KvmAlpineProvider through kvm_alpine::mod
pub use KvmAlpineProvider;