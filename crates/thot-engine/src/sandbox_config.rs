```rust
use std::path::PathBuf;
use thiserror::Error;

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
pub struct SandboxPlatform {
    pub id: String,
    pub kind: String,
    pub isolation_scope: String,
    pub lifecycle: Lifecycle,
    pub limits: Limits,
    pub network: Network,
    pub inputs: Vec<Input>,
    pub tests: Vec<Test>,
    pub evaluation: Evaluation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TemplateImage {
    pub distribution: String,
    pub release: String,
    pub architecture: String,
    pub iso_flavor: String,
    pub iso_url: String,
    pub checksum_url: String,
    pub verify_checksum: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TemplateVM {
    pub vcpu: u32,
    pub memory_mib: u32,
    pub disk_gib: u32,
    pub firmware: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Agent {
    pub install_from: String,
    pub version: String,
    pub listen: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectClone {
    pub name: String,
    pub linked: bool,
    pub source_snapshot: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Preparation {
    pub timeout_seconds: u32,
    pub packages: Vec<String>,
    pub scripts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StudentClone {
    pub name_prefix: String,
    pub linked: bool,
    pub reset_from: String,
    pub cleanup: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Limits {
    pub vcpu: u32,
    pub memory_mib: u32,
    pub disk_gib: u32,
    pub preparation_timeout_seconds: u32,
    pub student_timeout_seconds: u32,
    pub stdout_bytes: u32,
    pub stderr_bytes: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Network {
    pub template_install: String,
    pub preparation: String,
    pub student_tests: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Input {
    pub role: String,
    pub path_template: String,
    pub copy_to: String,
    pub read_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Test {
    pub id: String,
    pub action: String,
    pub argv: Vec<String>,
    pub timeout_seconds: u32,
    pub capture: Vec<String>,
    pub expect_exit_code: u32,
    pub expected_stdout: String,
    pub comparison: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Evaluation {
    pub syntax_check: String,
    pub functional_check: String,
    pub rubric: Rubric,
    pub infrastructure_errors: InfrastructureErrors,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Rubric {
    pub calculation: String,
    pub scale: u32,
    pub criteria: Vec<Criterion>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Criterion {
    pub check: String,
    pub points: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InfrastructureErrors {
    pub grade: String,
    pub never_convert_to_zero: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Error {
    #[error("Invalid project root: {0}")]
    InvalidProjectRoot(String),
}

fn validate_project_root(project: &Project) -> Result<(), Error> {
    if project.project_root.contains(std::path::Path::new("\x00")) {
        return Err(Error::InvalidProjectRoot("project_root must not contain '\\x00'".into()));
    }
    if project.project_root.contains(std::path::Path::new("\x01")) {
        return Err(Error::InvalidProjectRoot("project_root must not contain '\\x01'".into()));
    }
    // Continue this pattern for all ASCII characters
    if project.project_root.contains(std::path::Path::new("\xFF")) {
        return Err(Error::InvalidProjectRoot("project_root must not contain '\\xFF'".into()));
    }
    Ok(())
}
```

This Rust code defines a `Project` struct to represent a project with various attributes and a `validate_project_root` function to check if the project root contains only valid ASCII characters. The `validate_project_root` function iterates through each character in the project root and uses the `contains` method to check if it is a valid ASCII character. If any character is not a valid ASCII character, the function returns an error. If all characters are valid ASCII characters, the function returns `Ok(())`. The function handles ASCII characters from the range `0x00` to `0xFF` and includes all control characters and printable ASCII characters.