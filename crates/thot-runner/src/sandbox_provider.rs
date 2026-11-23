//! Engine-facing provider contract. Implementations own all sandbox resources
//! from template/baseline preparation through verified per-student cleanup.

use async_trait::async_trait;
use thot_core::sandbox_contracts::{
    HostCapabilities, PreparationResult, ProviderRunRequest, ProviderRunResult,
    SandboxPlatformRequest,
};

#[derive(Debug, thiserror::Error)]
pub enum SandboxProviderError {
    #[error("invalid provider request: {0}")]
    InvalidRequest(String),
    #[error("requested capability is unavailable: {0}")]
    Unsupported(String),
    #[error("provider infrastructure failure: {0}")]
    Infrastructure(String),
    #[error("provider state conflict: {0}")]
    Conflict(String),
}

/// A provider is responsible for creation, reuse, and deletion of its own
/// template, prepared baseline, student clone/container/workspace, and child
/// resources. The engine submits an entire student-unit attempt and never
/// manages hypervisor resource names or paths.
#[async_trait]
pub trait SandboxProvider: Send + Sync {
    fn kind(&self) -> thot_core::sandbox_contracts::ProviderKind;

    async fn preflight(
        &self,
        platform: &SandboxPlatformRequest,
    ) -> Result<HostCapabilities, SandboxProviderError>;

    async fn prepare(
        &self,
        platform: &SandboxPlatformRequest,
    ) -> Result<PreparationResult, SandboxProviderError>;

    /// The provider creates the environment, stages only the declared inputs,
    /// runs tasks sequentially, collects observations, then cleans up before
    /// returning. Infrastructure failure and cleanup status remain distinct.
    async fn run_deliverable(
        &self,
        request: ProviderRunRequest,
    ) -> Result<ProviderRunResult, SandboxProviderError>;

    async fn status(
        &self,
        run_id: &str,
        student_unit_id: &str,
    ) -> Result<Option<ProviderRunResult>, SandboxProviderError>;

    async fn cancel(&self, run_id: &str, student_unit_id: &str)
        -> Result<(), SandboxProviderError>;

    async fn reconcile(&self) -> Result<Vec<String>, SandboxProviderError>;
}
