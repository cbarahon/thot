use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Debug, Clone)]
pub struct TestInvocation {
    pub test_reference: String,
    pub repeat_index: usize,
    pub merged_params: String,
    pub effective_timeout: u64,
    pub project_root: PathBuf,
    pub project_archive_parent: PathBuf,
    pub delivery_ova_path: PathBuf,
    pub per_delivery_report_dir: PathBuf,
    pub per_test_artifacts_dir: PathBuf,
    pub vm_activity_runtime: VmActivityRuntime,
}

#[derive(Debug, Clone)]
pub struct TestOutcome {
    pub status: TestStatus,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub diagnostics: Vec<String>,
    pub structured_observation: Option<String>,
    pub evidence_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy)]
pub enum TestStatus {
    Passed,
    Failed,
    Skipped,
    TimedOut,
}

impl TestInvocation {
    pub fn repeat_display_name(&self) -> String {
        if self.repeat_index == 0 {
            self.test_reference.clone()
        } else {
            format!("{}_{}", self.test_reference, self.repeat_index + 1)
        }
    }
}
