//! Shared, versioned contracts for sandbox providers, test modules and run evidence.
//!
//! These types are not the legacy VM activity schema.  They describe the target
//! provider contract while the current `thot.dev/v2` smoke project remains
//! readable by `sandbox_project::SandboxProject`.

use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::collections::BTreeMap;

pub const SANDBOX_CONTRACT_VERSION: u32 = 1;
pub const TEST_MODULE_MANIFEST_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderKind {
    KvmDebian,
    KvmAlpine,
    Docker,
    Host,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceIdentity {
    pub provider: ProviderKind,
    /// Opaque provider identity. It is never a project-supplied libvirt name or path.
    pub resource_id: String,
    /// Digest of the provider-owned generation manifest.
    pub generation_sha256: String,
    /// Opaque ownership token recorded in the provider registry.
    pub ownership_id: String,
}

impl ResourceIdentity {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.resource_id.is_empty() || self.ownership_id.is_empty() {
            return Err(ContractError::Invalid("resource identity is empty".into()));
        }
        validate_sha256(&self.generation_sha256)
    }
}

macro_rules! resource_handle {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        pub struct $name(ResourceIdentity);

        impl $name {
            pub fn new(identity: ResourceIdentity) -> Result<Self, ContractError> {
                identity.validate()?;
                Ok(Self(identity))
            }

            pub fn identity(&self) -> &ResourceIdentity {
                &self.0
            }
        }
    };
}

resource_handle!(TemplateHandle);
resource_handle!(BaselineHandle);
resource_handle!(StudentEnvironmentHandle);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformCapability {
    pub id: String,
    pub version: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostCapabilities {
    pub provider: ProviderKind,
    pub provider_version: String,
    pub capabilities: Vec<PlatformCapability>,
    pub nested_virtualization: NestedVirtualization,
    pub resource_ceiling: ResourceCeiling,
}

impl HostCapabilities {
    /// Reject unavailable nesting and administrator resource ceilings before
    /// the provider performs any host-side work.
    pub fn validate_admission(
        &self,
        request: &SandboxPlatformRequest,
    ) -> Result<(), ContractError> {
        request.validate()?;
        if self.provider != request.provider {
            return Err(ContractError::Invalid(
                "selected provider does not match the capability report".into(),
            ));
        }
        if request.vcpu > self.resource_ceiling.vcpu
            || request.memory_mib > self.resource_ceiling.memory_mib
            || request.disk_bytes > self.resource_ceiling.disk_bytes
        {
            return Err(ContractError::Invalid(
                "requested resources exceed the administrator ceiling".into(),
            ));
        }
        if request.nested_virtualization_required
            && self.nested_virtualization != NestedVirtualization::Available
        {
            return Err(ContractError::Invalid(
                "nested virtualization is required but unavailable; no fallback is permitted"
                    .into(),
            ));
        }
        Ok(())
    }

    pub fn validate_module(&self, module: &TestModuleReference) -> Result<(), ContractError> {
        module.validate()?;
        for required in &module.capabilities {
            if !self.capabilities.iter().any(|available| {
                available.id == required.id && available.version == required.version
            }) {
                return Err(ContractError::Invalid(format!(
                    "provider lacks required capability {}@{}; no fallback is permitted",
                    required.id, required.version
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NestedVirtualization {
    NotRequested,
    Available,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceCeiling {
    pub vcpu: u32,
    pub memory_mib: u64,
    pub disk_bytes: u64,
    pub concurrent_environments: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxPlatformRequest {
    pub contract_version: u32,
    pub platform_id: String,
    pub provider: ProviderKind,
    pub isolation_scope: String,
    pub vcpu: u32,
    pub memory_mib: u64,
    pub disk_bytes: u64,
    pub nested_virtualization_required: bool,
    pub preparation: PreparationSpec,
    pub network_policy: String,
}

impl SandboxPlatformRequest {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.contract_version != SANDBOX_CONTRACT_VERSION
            || self.platform_id.is_empty()
            || self.isolation_scope.is_empty()
            || self.network_policy.is_empty()
            || self.vcpu == 0
            || self.memory_mib == 0
            || self.disk_bytes == 0
            || self.preparation.timeout_seconds == 0
        {
            return Err(ContractError::Invalid(
                "sandbox platform request is incomplete".into(),
            ));
        }
        for script in &self.preparation.scripts {
            script.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparationResult {
    pub execution: ExecutionOutcome,
    pub started_at: String,
    pub finished_at: String,
    pub duration_ms: u64,
    pub packages: Vec<String>,
    pub scripts: Vec<PreparationScriptResult>,
    pub diagnostics: Vec<Diagnostic>,
    pub baseline: Option<BaselineHandle>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparationScriptResult {
    pub path: String,
    pub sha256: String,
    pub execution: ExecutionOutcome,
    pub exit_code: Option<i32>,
    pub stdout: CapturedStream,
    pub stderr: CapturedStream,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderRunRequest {
    pub contract_version: u32,
    pub run_id: String,
    pub platform: SandboxPlatformRequest,
    pub student_unit_id: String,
    pub inputs: Vec<ArtifactReference>,
    pub tasks: Vec<SandboxTestTask>,
}

impl ProviderRunRequest {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.contract_version != SANDBOX_CONTRACT_VERSION
            || self.run_id.is_empty()
            || self.student_unit_id.is_empty()
            || self.tasks.is_empty()
        {
            return Err(ContractError::Invalid(
                "provider run request is incomplete".into(),
            ));
        }
        self.platform.validate()?;
        for input in &self.inputs {
            input.validate()?;
        }
        let mut task_ids = std::collections::HashSet::new();
        for task in &self.tasks {
            task.validate()?;
            if task.run_id != self.run_id
                || task.platform_id != self.platform.platform_id
                || task.student_unit_id != self.student_unit_id
                || !task_ids.insert(task.task_id.as_str())
            {
                return Err(ContractError::Invalid(
                    "provider tasks must be unique and scoped to the request".into(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderRunResult {
    pub contract_version: u32,
    pub run_id: String,
    pub platform_id: String,
    pub student_unit_id: String,
    pub observations: Vec<SandboxTestObservation>,
    pub cleanup: CleanupOutcome,
    pub diagnostics: Vec<Diagnostic>,
}

impl ProviderRunResult {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.contract_version != SANDBOX_CONTRACT_VERSION
            || self.run_id.is_empty()
            || self.platform_id.is_empty()
            || self.student_unit_id.is_empty()
        {
            return Err(ContractError::Invalid(
                "provider run result is incomplete".into(),
            ));
        }
        let mut task_ids = std::collections::HashSet::new();
        for observation in &self.observations {
            observation.validate()?;
            if observation.run_id != self.run_id
                || observation.platform_id != self.platform_id
                || observation.student_unit_id != self.student_unit_id
                || !task_ids.insert(observation.task_id.as_str())
            {
                return Err(ContractError::Invalid(
                    "provider observations must be unique and scoped to the result".into(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparationSpec {
    pub timeout_seconds: u32,
    pub network_policy: String,
    pub packages: Vec<String>,
    pub scripts: Vec<PreparationScript>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparationScript {
    /// Safe project-relative path to instructor-owned script content.
    pub path: String,
    pub sha256: String,
    pub timeout_seconds: u32,
}

impl PreparationScript {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.path.is_empty()
            || self.path.starts_with('/')
            || self
                .path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
            || self.timeout_seconds == 0
        {
            return Err(ContractError::Invalid(
                "preparation script path or timeout is invalid".into(),
            ));
        }
        validate_sha256(&self.sha256)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestModuleReference {
    pub module_id: String,
    /// Immutable publisher-selected module version, not a floating range.
    pub version: String,
    pub package_sha256: String,
    pub manifest_version: u32,
    pub capabilities: Vec<PlatformCapability>,
}

impl TestModuleReference {
    pub fn validate(&self) -> Result<(), ContractError> {
        if !valid_module_id(&self.module_id)
            || self.version.trim().is_empty()
            || self.version.contains('*')
            || self.version.starts_with('^')
            || self.version.starts_with('~')
            || self.manifest_version != TEST_MODULE_MANIFEST_VERSION
        {
            return Err(ContractError::Invalid(
                "test module identity or manifest version is invalid".into(),
            ));
        }
        validate_sha256(&self.package_sha256)?;
        validate_capabilities(&self.capabilities)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestModuleManifest {
    pub manifest_version: u32,
    pub module_id: String,
    pub version: String,
    pub package_sha256: String,
    pub capabilities: Vec<PlatformCapability>,
    #[serde(default)]
    pub assets: Vec<ModuleAsset>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleAsset {
    pub path: String,
    pub sha256: String,
    pub size_bytes: u64,
}

impl TestModuleManifest {
    pub fn reference(&self) -> Result<TestModuleReference, ContractError> {
        let reference = TestModuleReference {
            module_id: self.module_id.clone(),
            version: self.version.clone(),
            package_sha256: self.package_sha256.clone(),
            manifest_version: self.manifest_version,
            capabilities: self.capabilities.clone(),
        };
        reference.validate()?;
        let mut asset_paths = std::collections::HashSet::new();
        for asset in &self.assets {
            if asset.path.is_empty()
                || asset.path.starts_with('/')
                || asset
                    .path
                    .split('/')
                    .any(|part| part.is_empty() || part == "." || part == "..")
            {
                return Err(ContractError::Invalid("module asset path is unsafe".into()));
            }
            if !asset_paths.insert(asset.path.as_str()) {
                return Err(ContractError::Invalid(format!(
                    "duplicate module asset path `{}`",
                    asset.path
                )));
            }
            validate_sha256(&asset.sha256)?;
        }
        Ok(reference)
    }

    pub fn verify_package_bytes(&self, package: &[u8]) -> Result<(), ContractError> {
        self.reference()?;
        let actual = format!("{:x}", sha2::Sha256::digest(package));
        if actual != self.package_sha256 {
            return Err(ContractError::Invalid(
                "test module package digest does not match its manifest".into(),
            ));
        }
        Ok(())
    }

    pub fn verify_asset_bytes(&self, path: &str, bytes: &[u8]) -> Result<(), ContractError> {
        let asset = self
            .assets
            .iter()
            .find(|asset| asset.path == path)
            .ok_or_else(|| ContractError::Invalid(format!("unknown module asset `{path}`")))?;
        let actual = format!("{:x}", sha2::Sha256::digest(bytes));
        if actual != asset.sha256 || bytes.len() as u64 != asset.size_bytes {
            return Err(ContractError::Invalid(format!(
                "test module asset `{path}` digest or size does not match its manifest"
            )));
        }
        Ok(())
    }
}

pub fn parse_test_module_manifest(bytes: &[u8]) -> Result<TestModuleManifest, ContractError> {
    let manifest: TestModuleManifest = serde_json::from_slice(bytes)
        .map_err(|error| ContractError::Invalid(format!("module manifest JSON: {error}")))?;
    manifest.reference()?;
    Ok(manifest)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum TestOperation {
    Exec {
        argv: Vec<String>,
        cwd: String,
    },
    OvaInventory,
    OvaPartitionTable,
    OvaFilesystems,
    OvaInstalledSystems,
    OvaGrubBoot {
        entry_id: String,
        expected_os: String,
    },
    OvaSharedData {
        guest_os: String,
        probe_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxTestTask {
    pub contract_version: u32,
    pub task_id: String,
    pub attempt_id: String,
    pub run_id: String,
    pub platform_id: String,
    pub student_unit_id: String,
    pub test_id: String,
    pub module: Option<TestModuleReference>,
    pub operation: TestOperation,
    pub timeout_seconds: u32,
    pub stdout_limit_bytes: u64,
    pub stderr_limit_bytes: u64,
    pub artifact_limit_bytes: u64,
    #[serde(default)]
    pub parameters: BTreeMap<String, serde_json::Value>,
}

impl SandboxTestTask {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.contract_version != SANDBOX_CONTRACT_VERSION
            || self.task_id.is_empty()
            || self.attempt_id.is_empty()
            || self.run_id.is_empty()
            || self.platform_id.is_empty()
            || self.student_unit_id.is_empty()
            || self.test_id.is_empty()
            || self.timeout_seconds == 0
            || self.stdout_limit_bytes == 0
            || self.stderr_limit_bytes == 0
            || self.artifact_limit_bytes == 0
        {
            return Err(ContractError::Invalid(
                "sandbox test task is incomplete".into(),
            ));
        }
        if let Some(module) = &self.module {
            module.validate()?;
        }
        match &self.operation {
            TestOperation::Exec { argv, cwd } => {
                if argv.is_empty() || cwd.is_empty() || !cwd.starts_with('/') {
                    return Err(ContractError::Invalid(
                        "exec task requires argv and an absolute guest working directory".into(),
                    ));
                }
                if self.module.is_some() {
                    return Err(ContractError::Invalid(
                        "exec tasks cannot declare a portable test module".into(),
                    ));
                }
            }
            _ if self.module.is_none() => {
                return Err(ContractError::Invalid(
                    "typed OVA operations require a pinned test module".into(),
                ));
            }
            _ => {}
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionOutcome {
    Completed,
    TimedOut,
    ResourceExceeded,
    PlatformError,
    Cancelled,
    Interrupted,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssessmentOutcome {
    Passed,
    Failed,
    Skipped,
    Pending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupOutcome {
    Clean,
    Pending,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactReference {
    pub sha256: String,
    pub size_bytes: u64,
    pub media_type: String,
    pub relative_path: String,
    pub complete: bool,
}

impl ArtifactReference {
    pub fn validate(&self) -> Result<(), ContractError> {
        validate_sha256(&self.sha256)?;
        if self.relative_path.is_empty()
            || self.relative_path.starts_with('/')
            || self
                .relative_path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(ContractError::Invalid("artifact path is unsafe".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapturedStream {
    pub artifact: ArtifactReference,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Diagnostic {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub technical_detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxTestObservation {
    pub contract_version: u32,
    pub task_id: String,
    pub attempt_id: String,
    pub run_id: String,
    pub platform_id: String,
    pub student_unit_id: String,
    pub test_id: String,
    pub execution: ExecutionOutcome,
    pub started_at: String,
    pub finished_at: String,
    pub duration_ms: u64,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub stdout: CapturedStream,
    pub stderr: CapturedStream,
    #[serde(default)]
    pub observations: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
    #[serde(default)]
    pub artifacts: Vec<ArtifactReference>,
}

impl SandboxTestObservation {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.contract_version != SANDBOX_CONTRACT_VERSION
            || self.task_id.is_empty()
            || self.attempt_id.is_empty()
            || self.run_id.is_empty()
            || self.platform_id.is_empty()
            || self.student_unit_id.is_empty()
            || self.test_id.is_empty()
            || self.started_at.is_empty()
            || self.finished_at.is_empty()
        {
            return Err(ContractError::Invalid(
                "sandbox observation identity or status fields are inconsistent".into(),
            ));
        }
        self.stdout.artifact.validate()?;
        self.stderr.artifact.validate()?;
        for artifact in &self.artifacts {
            artifact.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryProvenance {
    pub source_kind: String,
    pub selector: String,
    pub provider_id: Option<String>,
    pub revision: Option<String>,
    pub name: String,
    pub mime_type: Option<String>,
    pub source_checksum: Option<String>,
    pub source_checksum_algorithm: Option<String>,
    pub size_bytes: u64,
    pub created_at: Option<String>,
    pub modified_at: Option<String>,
    pub acquired_at: String,
    pub sha256: String,
}

impl DeliveryProvenance {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.source_kind.is_empty()
            || self.selector.is_empty()
            || self.name.is_empty()
            || self.acquired_at.is_empty()
        {
            return Err(ContractError::Invalid(
                "delivery provenance is incomplete".into(),
            ));
        }
        if self.source_checksum.is_some() != self.source_checksum_algorithm.is_some() {
            return Err(ContractError::Invalid(
                "source checksum and its algorithm must be recorded together".into(),
            ));
        }
        validate_sha256(&self.sha256)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunSnapshot {
    pub schema_version: String,
    pub run_id: String,
    pub project_id: String,
    pub created_at: String,
    pub configuration_sha256: String,
    pub status: RunStatus,
    pub current_revision: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Created,
    Running,
    Interrupted,
    Failed,
    Complete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunJournalEvent {
    pub sequence: u64,
    pub event_id: String,
    pub occurred_at: String,
    pub event_type: String,
    pub previous_sha256: String,
    pub details: serde_json::Value,
    pub event_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunRevision {
    pub revision_id: String,
    pub parent_revision: Option<String>,
    pub created_at: String,
    pub operation: String,
    pub result_paths: Vec<String>,
    pub configuration_sha256: String,
    pub evidence_manifest_sha256: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ContractError {
    #[error("invalid sandbox contract: {0}")]
    Invalid(String),
}

pub fn validate_sha256(value: &str) -> Result<(), ContractError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ContractError::Invalid(
            "expected a 64-character SHA-256 digest".into(),
        ));
    }
    Ok(())
}

fn valid_module_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn validate_capabilities(capabilities: &[PlatformCapability]) -> Result<(), ContractError> {
    let mut ids = std::collections::HashSet::new();
    if capabilities.iter().any(|capability| {
        capability.id.trim().is_empty()
            || capability.version == 0
            || !ids.insert((capability.id.as_str(), capability.version))
    }) {
        return Err(ContractError::Invalid(
            "module capabilities must have unique non-empty IDs and positive versions".into(),
        ));
    }
    Ok(())
}
