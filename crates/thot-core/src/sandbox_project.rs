//! Versioned, professor-authored project configuration for sandbox evaluation.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxProject {
    pub schema_version: String,
    pub project_id: String,
    pub name: String,
    #[serde(default = "default_language")]
    pub language: String,
    pub students_file: String,
    pub assignments_file: String,
    #[serde(default)]
    pub delivery_agent: Option<DeliveryAgentConfig>,
    #[serde(default)]
    pub delivery_folder: Option<String>,
    #[serde(default)]
    pub delivery_limits: DeliveryLimits,
    pub sandbox_platforms: Vec<SandboxPlatform>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryLimits {
    #[serde(default = "default_delivery_max_bytes")]
    pub max_bytes: u64,
    #[serde(default = "default_delivery_max_files")]
    pub max_files: usize,
}

impl Default for DeliveryLimits {
    fn default() -> Self {
        Self {
            max_bytes: default_delivery_max_bytes(),
            max_files: default_delivery_max_files(),
        }
    }
}

fn default_delivery_max_bytes() -> u64 {
    128 * 1024 * 1024
}

fn default_delivery_max_files() -> usize {
    10_000
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum DeliveryAgentConfig {
    Local {
        #[serde(default)]
        path: Option<String>,
    },
    Gdrive {
        folder_url: String,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxPlatform {
    pub id: String,
    pub kind: String,
    pub isolation_scope: String,
    pub lifecycle: SandboxLifecycle,
    pub limits: SandboxLimits,
    pub network: SandboxNetwork,
    pub inputs: Vec<SandboxInput>,
    pub tests: Vec<SandboxTest>,
    pub evaluation: SandboxEvaluation,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxLifecycle {
    pub template_name: String,
    pub template_image: SandboxImage,
    pub template_vm: SandboxVm,
    #[serde(default)]
    pub template_packages: Vec<String>,
    pub agent: SandboxAgent,
    pub project_clone: SandboxProjectClone,
    pub preparation: SandboxPreparation,
    pub student_clone: SandboxStudentClone,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxImage {
    pub distribution: String,
    pub release: String,
    pub architecture: String,
    pub image_url: String,
    pub sha512: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxVm {
    pub vcpu: u32,
    pub memory_mib: u32,
    pub disk_gib: u32,
    pub firmware: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxAgent {
    pub install_from: String,
    pub version: String,
    pub listen: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxProjectClone {
    pub name: String,
    pub linked: bool,
    pub source_snapshot: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxPreparation {
    pub timeout_seconds: u32,
    #[serde(default)]
    pub packages: Vec<String>,
    #[serde(default)]
    pub scripts: Vec<SandboxPreparationScript>,
}

/// Instructor-authored script stored as a project-relative file. The engine
/// calculates its SHA-256 before creating/reusing a prepared baseline.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxPreparationScript {
    pub path: String,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub timeout_seconds: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxStudentClone {
    pub name_prefix: String,
    pub linked: bool,
    pub reset_from: String,
    pub cleanup: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxLimits {
    pub vcpu: u32,
    pub memory_mib: u32,
    pub disk_gib: u32,
    pub preparation_timeout_seconds: u32,
    pub student_timeout_seconds: u32,
    pub stdout_bytes: usize,
    pub stderr_bytes: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxNetwork {
    pub template_install: String,
    pub preparation: String,
    pub student_tests: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxInput {
    pub role: String,
    #[serde(default)]
    pub path_template: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    pub copy_to: String,
    #[serde(default = "default_true")]
    pub read_only: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxTest {
    pub id: String,
    pub action: String,
    #[serde(default)]
    pub requires: Vec<String>,
    pub argv: Vec<String>,
    pub timeout_seconds: u32,
    pub expect_exit_code: i32,
    #[serde(default)]
    pub expected_stdout: Option<String>,
    #[serde(default)]
    pub capture: Vec<String>,
    #[serde(default)]
    pub comparison: Option<String>,
    #[serde(default)]
    pub module: Option<SandboxTestModule>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxTestModule {
    /// Executable contract; P3 currently implements only `posix-shell-script-v1`.
    pub runtime: String,
    /// Project-relative JSON module manifest.
    pub manifest: String,
    /// Project-relative POSIX shell script whose bytes match package_sha256.
    pub package: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SandboxEvaluation {
    #[serde(flatten)]
    pub checks: BTreeMap<String, SandboxCheck>,
    #[serde(default)]
    pub rubric: Option<SandboxRubric>,
    /// Independent grade items for schema v3 projects. Each item may be
    /// complete while another remains pending because of infrastructure.
    #[serde(default)]
    pub items: Vec<SandboxEvaluationItem>,
    pub infrastructure_errors: SandboxInfrastructurePolicy,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxEvaluationItem {
    pub id: String,
    pub title: String,
    pub calculation: String,
    pub scale: u32,
    pub criteria: Vec<SandboxCriterion>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxCheck {
    pub tests: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxRubric {
    pub calculation: String,
    pub scale: u32,
    pub criteria: Vec<SandboxCriterion>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxCriterion {
    pub check: String,
    pub points: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxInfrastructurePolicy {
    pub grade: String,
    pub never_convert_to_zero: bool,
}

impl SandboxProject {
    pub fn validate(&self) -> Result<(), String> {
        if !matches!(
            self.schema_version.as_str(),
            "thot.dev/v2" | "thot.dev/v3" | "thot.dev/v4"
        ) {
            return Err(format!(
                "unsupported sandbox schema version `{}`",
                self.schema_version
            ));
        }
        if !valid_id(&self.project_id) || self.name.trim().is_empty() {
            return Err("project_id must be a safe identifier and name must not be empty".into());
        }
        for path in [&self.students_file, &self.assignments_file] {
            if !safe_project_path(path) {
                return Err(format!(
                    "roster/assignment file must be a safe project-relative path: `{path}`"
                ));
            }
        }
        if let Some(path) = &self.delivery_folder
            && (path.trim().is_empty() || path.contains('\0'))
        {
            return Err("delivery_folder must be a non-empty filesystem path".into());
        }
        if self.delivery_limits.max_bytes == 0
            || self.delivery_limits.max_bytes > 64 * 1024 * 1024 * 1024
            || self.delivery_limits.max_files == 0
            || self.delivery_limits.max_files > 100_000
        {
            return Err(
                "delivery limits must be within the supported 64 GiB / 100,000 file ceilings"
                    .into(),
            );
        }
        if let Some(DeliveryAgentConfig::Local { path: Some(path) }) = &self.delivery_agent
            && (path.trim().is_empty() || path.contains('\0'))
        {
            return Err("local delivery agent path must be a non-empty filesystem path".into());
        }
        if let Some(DeliveryAgentConfig::Gdrive { folder_url }) = &self.delivery_agent
            && (!folder_url.starts_with("https://drive.google.com/drive/folders/")
                || folder_url.len() <= "https://drive.google.com/drive/folders/".len())
        {
            return Err("Google Drive delivery agent requires a Drive folder URL".into());
        }
        if self.sandbox_platforms.len() != 1 {
            return Err("the local CLI currently requires exactly one sandbox_platform".into());
        }
        for platform in &self.sandbox_platforms {
            if !valid_id(&platform.id) || platform.kind != "kvm-alpine" {
                return Err(format!(
                    "unsupported or invalid sandbox platform `{}`",
                    platform.id
                ));
            }
            if platform.isolation_scope != "kvm-guest" {
                return Err(format!(
                    "platform `{}` must use kvm-guest isolation",
                    platform.id
                ));
            }
            if platform
                .inputs
                .iter()
                .filter(|input| input.role == "student-deliverable")
                .count()
                != 1
            {
                return Err(format!(
                    "platform `{}` must declare exactly one student-deliverable input",
                    platform.id
                ));
            }
            let lifecycle = &platform.lifecycle;
            for name in [
                &lifecycle.template_name,
                &lifecycle.project_clone.name,
                &lifecycle.student_clone.name_prefix,
            ] {
                if !valid_domain_name(name) {
                    return Err(format!("invalid libvirt resource name `{name}`"));
                }
            }
            if lifecycle.template_image.image_url.get(..8) != Some("https://")
                || !lifecycle
                    .template_image
                    .image_url
                    .starts_with("https://dl-cdn.alpinelinux.org/alpine/v")
                || lifecycle.template_image.sha512.len() != 128
                || !lifecycle
                    .template_image
                    .sha512
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(
                    "template image requires an HTTPS URL and a 128-digit SHA-512 digest".into(),
                );
            }
            if lifecycle.template_image.distribution != "Alpine Linux"
                || lifecycle.template_image.architecture != "x86_64"
                || lifecycle.agent.install_from != "pinned-static-binary"
                || lifecycle.agent.listen != "guest-initiated-only"
                || !lifecycle.template_packages.is_empty()
                || lifecycle.template_vm.firmware != "bios"
                || !lifecycle.project_clone.linked
                || lifecycle.project_clone.source_snapshot != "prepared-baseline"
                || !lifecycle.student_clone.linked
                || lifecycle.student_clone.reset_from != "prepared-baseline"
                || lifecycle.student_clone.cleanup != "always"
                || lifecycle.project_clone.name == lifecycle.template_name
                || lifecycle.agent.version.trim().is_empty()
            {
                return Err(format!(
                    "platform `{}` has unsupported image, agent, or clone settings",
                    platform.id
                ));
            }
            let limits = &platform.limits;
            if limits.vcpu == 0
                || limits.vcpu > 8
                || limits.memory_mib < 256
                || limits.memory_mib > 16384
                || limits.disk_gib == 0
                || limits.disk_gib > 100
                || limits.student_timeout_seconds == 0
                || limits.student_timeout_seconds > 3600
                || limits.preparation_timeout_seconds == 0
                || limits.preparation_timeout_seconds > 3600
                || limits.stdout_bytes == 0
                || limits.stdout_bytes > 16 * 1024 * 1024
                || limits.stderr_bytes == 0
                || limits.stderr_bytes > 16 * 1024 * 1024
            {
                return Err(format!(
                    "platform `{}` has limits outside the supported host policy",
                    platform.id
                ));
            }
            if lifecycle.preparation.timeout_seconds > limits.preparation_timeout_seconds
                || lifecycle.preparation.timeout_seconds == 0
                || lifecycle.preparation.packages.iter().any(|package| {
                    package.is_empty()
                        || !package.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric()
                                || matches!(byte, b'-' | b'_' | b'.' | b'+')
                        })
                })
            {
                return Err(format!(
                    "platform `{}` preparation settings are invalid",
                    platform.id
                ));
            }
            let mut preparation_paths = std::collections::HashSet::new();
            for script in &lifecycle.preparation.scripts {
                if !safe_project_path(&script.path)
                    || !preparation_paths.insert(script.path.as_str())
                    || script.timeout_seconds.is_some_and(|timeout| {
                        timeout == 0 || timeout > lifecycle.preparation.timeout_seconds
                    })
                    || script.sha256.as_ref().is_some_and(|digest| {
                        digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
                    })
                {
                    return Err(format!(
                        "platform `{}` has an unsafe or duplicate preparation script",
                        platform.id
                    ));
                }
            }
            if limits.disk_gib != lifecycle.template_vm.disk_gib
                || limits.vcpu != lifecycle.template_vm.vcpu
                || limits.memory_mib != lifecycle.template_vm.memory_mib
            {
                return Err(
                    "template_vm and platform resource limits must match for this provider".into(),
                );
            }
            if platform.network.student_tests != "deny" {
                return Err("kvm-alpine student_tests network policy must be `deny`".into());
            }
            let mut test_ids = std::collections::HashSet::new();
            for test in &platform.tests {
                if !valid_id(&test.id)
                    || !test_ids.insert(test.id.as_str())
                    || test.argv.is_empty()
                    || !matches!(test.action.as_str(), "exec" | "module")
                    || (test.action == "exec" && test.module.is_some())
                    || (test.action == "module" && test.module.is_none())
                    || test.timeout_seconds == 0
                    || test.timeout_seconds > limits.student_timeout_seconds
                {
                    return Err(format!(
                        "test `{}` has an invalid ID, argv, or timeout",
                        test.id
                    ));
                }
                if let Some(module) = &test.module
                    && (!matches!(
                        module.runtime.as_str(),
                        "posix-shell-script-v1" | "linux-x86_64-elf-v1"
                    ) || !safe_project_path(&module.manifest)
                        || !safe_project_path(&module.package))
                {
                    return Err(format!(
                        "test `{}` module manifest/package must stay inside the project",
                        test.id
                    ));
                }
                if test.requires.iter().any(|dependency| {
                    !test_ids.contains(dependency.as_str()) || dependency == &test.id
                }) {
                    return Err(format!(
                        "test `{}` must depend only on earlier tests",
                        test.id
                    ));
                }
            }
            for input in &platform.inputs {
                let student_delivery = input.role == "student-deliverable";
                if (!student_delivery && input.path_template.is_some() == input.path.is_some())
                    || (student_delivery && input.path.is_some())
                    || !matches!(
                        input.role.as_str(),
                        "student-deliverable" | "instructor-tests"
                    )
                    || !input.copy_to.starts_with("/work/")
                    || input.copy_to.split('/').any(|part| part == "..")
                {
                    return Err(format!(
                        "input `{}` must declare a safe /work destination and a source only when it is not a student deliverable",
                        input.role
                    ));
                }
                if let Some(path) = &input.path
                    && !safe_project_path(path)
                {
                    return Err(format!(
                        "input `{}` path must stay inside the project",
                        input.role
                    ));
                }
                if let Some(template) = &input.path_template {
                    let expanded = template.replace("{student_id}", "student");
                    if !safe_project_path(&expanded)
                        || expanded.contains('{')
                        || expanded.contains('}')
                    {
                        return Err(format!(
                            "input `{}` has an unsafe path template",
                            input.role
                        ));
                    }
                }
            }
            // v4 is an explicit new document version, normalized into the
            // existing v3 assessment model. This initial compatibility slice
            // keeps the v3 execution contract intact while the concise v4
            // authoring form is introduced by the next schema adapter.
            let is_v2 = self.schema_version == "thot.dev/v2";
            let is_v4 = self.schema_version == "thot.dev/v4";
            let invalid_assessment_shape = if is_v2 {
                platform.evaluation.rubric.is_none() || !platform.evaluation.items.is_empty()
            } else if is_v4 {
                // v4's compact authoring adapter supports either one rubric or
                // independent items during migration, but never both at once.
                platform.evaluation.rubric.is_some() == !platform.evaluation.items.is_empty()
            } else {
                platform.evaluation.rubric.is_some() || platform.evaluation.items.is_empty()
            };
            if invalid_assessment_shape
                || platform.evaluation.infrastructure_errors.grade != "pending"
                || !platform
                    .evaluation
                    .infrastructure_errors
                    .never_convert_to_zero
            {
                return Err(format!(
                    "platform `{}` rubric or infrastructure policy is invalid",
                    platform.id
                ));
            }
            let rubrics: Vec<(&str, u32, &str, &[SandboxCriterion])> =
                if let Some(rubric) = &platform.evaluation.rubric {
                    vec![(
                        "default",
                        rubric.scale,
                        &rubric.calculation,
                        &rubric.criteria,
                    )]
                } else {
                    let mut item_ids = std::collections::HashSet::new();
                    for item in &platform.evaluation.items {
                        if !valid_id(&item.id)
                            || !item_ids.insert(item.id.as_str())
                            || item.title.trim().is_empty()
                        {
                            return Err(format!(
                                "platform `{}` has an invalid or duplicate evaluation item",
                                platform.id
                            ));
                        }
                    }
                    platform
                        .evaluation
                        .items
                        .iter()
                        .map(|item| {
                            (
                                item.id.as_str(),
                                item.scale,
                                item.calculation.as_str(),
                                item.criteria.as_slice(),
                            )
                        })
                        .collect()
                };
            for (rubric_name, scale, calculation, rubric_criteria) in rubrics {
                let points: u32 = rubric_criteria
                    .iter()
                    .map(|criterion| criterion.points)
                    .sum();
                if scale == 0 || points != scale || calculation != "weighted_sum" {
                    return Err(format!(
                        "evaluation `{rubric_name}` must use weighted_sum and criteria totaling its nonzero scale"
                    ));
                }
                for criterion in rubric_criteria {
                    let Some(check) = platform.evaluation.checks.get(&criterion.check) else {
                        return Err(format!(
                            "rubric references undefined check `{}`",
                            criterion.check
                        ));
                    };
                    if check.tests.is_empty()
                        || check
                            .tests
                            .iter()
                            .any(|test_id| !test_ids.contains(test_id.as_str()))
                    {
                        return Err(format!(
                            "check `{}` must reference configured test IDs",
                            criterion.check
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn valid_domain_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 55
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn safe_project_path(value: &str) -> bool {
    let path = std::path::Path::new(value);
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
}

fn default_language() -> String {
    "en".into()
}
fn default_true() -> bool {
    true
}
