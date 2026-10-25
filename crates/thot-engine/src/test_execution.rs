//! Purpose: Defines the invocation and outcome contracts shared by test platform adapters.
use std::{collections::BTreeMap, path::PathBuf, time::Duration};
use thot_core::test_results::TestStatus;

use crate::vm_activity::{DeliveryInput, VmActivityRuntime};

#[derive(Debug, Clone)]
pub struct TestInvocation {
    pub test_name: String,
    pub test_reference: String,
    pub parameters: BTreeMap<String, String>,
    pub timeout: Duration,
    pub project_root: PathBuf,
    pub project_archive_parent: PathBuf,
    pub delivery: DeliveryInput,
    pub report_dir: PathBuf,
    pub artifacts_dir: PathBuf,
    pub runtime: VmActivityRuntime,
    pub run_id: String,
    pub expedient_id: String,
}

#[derive(Debug, Clone)]
pub struct TestOutcome {
    pub status: TestStatus,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub diagnostics: String,
    pub observation: serde_json::Value,
    pub evidence: Vec<String>,
}

impl TestOutcome {
    pub fn error(message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            status: TestStatus::Error,
            stdout: String::new(),
            stderr: message.clone(),
            exit_code: None,
            diagnostics: message,
            observation: serde_json::json!({}),
            evidence: Vec::new(),
        }
    }

    pub fn skipped(reason: impl Into<String>) -> Self {
        let reason = reason.into();
        Self {
            status: TestStatus::Skipped,
            stdout: String::new(),
            stderr: String::new(),
            exit_code: None,
            diagnostics: reason,
            observation: serde_json::json!({}),
            evidence: Vec::new(),
        }
    }
}
