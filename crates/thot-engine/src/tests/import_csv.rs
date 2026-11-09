//! Purpose: Marks a test result for per-student CSV substitution after shared delivery execution.
use serde_json::json;
use thot_core::test_results::TestStatus;

use crate::test_execution::{TestInvocation, TestOutcome};

pub async fn run(_invocation: &TestInvocation) -> TestOutcome {
    // Delivery tests are shared across students assigned the same file. The engine replaces this
    // placeholder separately in each student expedient using the CSV row for that student's ID.
    TestOutcome {
        status: TestStatus::Passed,
        stdout: String::new(),
        stderr: String::new(),
        exit_code: Some(0),
        diagnostics: "CSV result will be resolved for each student expedient".into(),
        observation: json!({}),
        evidence: Vec::new(),
    }
}
