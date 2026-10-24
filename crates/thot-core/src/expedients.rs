use std::time::SystemTime;

#[derive(Debug, Clone)]
pub struct DeliveryExpedient {
    pub version: u32,
    pub source_provenance: String,
    pub project_snapshot_hash: String,
    pub execution_environment_versions: Vec<String>,
    pub vm_observations: Vec<String>,
    pub result_ids: Vec<String>,
    pub artifact_references: Vec<String>,
    pub cleanup_manifest_status: String,
    pub timestamps: Vec<SystemTime>,
    pub orchestration_status: String,
}
