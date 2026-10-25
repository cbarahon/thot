//! Purpose: Cleans up only the VM resources recorded in the delivery manifest.
use crate::{
    test_execution::{TestInvocation, TestOutcome},
    vm_activity::cleanup_from_manifest,
};
use thot_core::test_results::TestStatus;

pub async fn run(invocation: &TestInvocation) -> TestOutcome {
    let cleanup = cleanup_from_manifest(
        &invocation.runtime,
        &invocation.report_dir,
        &invocation.delivery.delivery_id,
    )
    .await;
    let passed = cleanup["status"].as_str() == Some("passed");
    let output = serde_json::to_string_pretty(&cleanup).unwrap_or_else(|_| "{}".into());
    TestOutcome {
        status: if passed {
            TestStatus::Passed
        } else {
            TestStatus::Failed
        },
        stdout: output,
        stderr: cleanup["details"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default(),
        exit_code: Some(if passed { 0 } else { 1 }),
        diagnostics: if passed {
            String::new()
        } else {
            "manifest-scoped cleanup failed".into()
        },
        observation: cleanup,
        evidence: vec!["cleanup-manifest.json".into()],
    }
}
