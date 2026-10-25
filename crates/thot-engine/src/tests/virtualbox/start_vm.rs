//! Purpose: Starts the manifest-owned VirtualBox guest in headless mode.
use super::{output_outcome, require_owned_vm, runner, vm_name};
use crate::test_execution::{TestInvocation, TestOutcome};

pub async fn run(invocation: &TestInvocation) -> TestOutcome {
    let name = match vm_name(invocation) {
        Ok(name) => name,
        Err(error) => return TestOutcome::error(error),
    };
    if let Err(error) = require_owned_vm(invocation, &name) {
        return TestOutcome::error(error);
    }
    let result = runner(invocation)
        .run(&[
            "startvm".into(),
            name.clone(),
            "--type".into(),
            "headless".into(),
        ])
        .await;
    let mut outcome = output_outcome(result, "VBoxManage startvm");
    outcome.observation["vm_name"] = serde_json::json!(name);
    outcome
}
