use super::kvm_alpine::KvmAlpineHost;
use crate::vm_activity::{DeliveryInput, VmActivityRuntime};
use std::path::PathBuf;
use thot_core::test_results::TestStatus;

pub struct KvmAlpineProvider {
    host: KvmAlpineHost,
}

impl KvmAlpineProvider {
    pub fn new(host: KvmAlpineHost) -> Self {
        KvmAlpineProvider { host }
    }
}

impl SandboxProvider for KvmAlpineProvider {
    fn create_run(&self, run_id: &str, project_root: &PathBuf, student_unit: &str) -> Result<(), Error> {
        self.host.create_run(run_id, project_root, student_unit)
    }

    fn destroy_run(&self, run_id: &str) -> Result<(), Error> {
        self.host.destroy_run(run_id)
    }

    fn execute_test(&self, test_invocation: TestInvocation) -> Result<TestOutcome, Error> {
        self.host.execute_test(test_invocation)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Provider error: {0}")]
    ProviderError(String),
    #[error("Transport error: {0}")]
    TransportError(String),
    #[error("Infrastructure error: {0}")]
    InfrastructureError(String),
}