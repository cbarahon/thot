use serde::{Deserialize, Serialize, deny_unknown_fields};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SandboxRunRequest {
    pub project_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Lifecycle {
    pub template_name: String,
    pub template_image: Image,
    pub template_vm: Vm,
    pub template_packages: Vec<String>,
    pub agent: Agent,
    pub project_clone: Clone,
    pub preparation: Preparation,
    pub student_clone: Clone,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Image {
    pub distribution: String,
    pub release: String,
    pub architecture: String,
    pub iso_flavor: String,
    pub iso_url: String,
    pub checksum_url: String,
    pub verify_checksum: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Vm {
    pub vcpu: u32,
    pub memory_mib: u64,
    pub disk_gib: u64,
    pub firmware: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Agent {
    pub install_from: String,
    pub version: String,
    pub listen: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Clone {
    pub name: String,
    pub linked: bool,
    pub source_snapshot: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Preparation {
    pub timeout_seconds: u64,
    pub packages: Vec<String>,
    pub scripts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub cpu: Option<u32>,
    pub memory: Option<u64>,
    pub vcpu: Option<u32>,
    pub memory_mib: Option<u64>,
    pub disk_gib: Option<u64>,
    pub preparation_timeout_seconds: Option<u64>,
    pub student_timeout_seconds: Option<u64>,
    pub stdout_bytes: Option<u64>,
    pub stderr_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct NetworkPolicy {
    pub inbound: Vec<String>,
    pub outbound: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct InputDeclaration {
    pub role: String,
    pub path: Option<PathBuf>,
    pub path_template: Option<String>,
    pub copy_to: Option<PathBuf>,
    pub read_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Test {
    pub id: String,
    pub action: String,
    pub argv: Vec<String>,
    pub requires: Vec<String>,
    pub timeout_seconds: Option<u64>,
    pub expect_exit_code: Option<u32>,
    pub expected_stdout: Option<String>,
    pub comparison: Option<String>,
    pub capture: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Evaluation {
    pub expressions: Vec<String>,
    pub calculation: String,
    pub scale: Option<f64>,
    pub rubric_criteria: Vec<RubricCriterion>,
    pub infrastructure_error_policy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RubricCriterion {
    pub name: String,
    pub description: Option<String>,
    pub checks: Vec<String>,
    pub calculation: String,
    pub parameters: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub enum AssessmentStatus {
    Passed,
    Failed,
    Skipped,
    Pending,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StudentsFile {
    pub schema_version: String,
    pub identity_field: String,
    pub students: Vec<Student>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Student {
    pub student_id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AssignmentsFile {
    pub schema_version: String,
    pub assignments: Vec<Assignment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Assignment {
    pub student_id: String,
    pub deliverable: String,
}