//! Purpose: Shuts down the manifest-owned guest and verifies power-off completion.
use super::{require_owned_vm, runner, vm_name, wait_for_poweroff};
use crate::test_execution::{TestInvocation, TestOutcome};
use std::time::Duration;
use thot_core::test_results::TestStatus;

pub async fn run(invocation: &TestInvocation) -> TestOutcome {
    let name = match vm_name(invocation) {
        Ok(name) => name,
        Err(error) => return TestOutcome::error(error),
    };
    if let Err(error) = require_owned_vm(invocation, &name) {
        return TestOutcome::error(error);
    }
    match super::vm_state(invocation, &name).await {
        Ok(state) if matches!(state.as_str(), "poweroff" | "aborted") => {
            return TestOutcome {
                status: TestStatus::Passed,
                stdout: String::new(),
                stderr: String::new(),
                exit_code: Some(0),
                diagnostics: String::new(),
                observation: serde_json::json!({"vm_name": name, "shutdown_method": "already_off"}),
                evidence: Vec::new(),
            };
        }
        Ok(_) => {}
        Err(error) => return TestOutcome::error(error),
    }
    let graceful = runner(invocation)
        .run(&["controlvm".into(), name.clone(), "acpipowerbutton".into()])
        .await;
    let mut stdout = String::new();
    let mut stderr = String::new();
    let exit_code = Some(0);
    let method;
    match graceful {
        Ok(output) if output.exit_code == Some(0) => {
            stdout.push_str(&output.stdout);
            stderr.push_str(&output.stderr);
            match wait_for_poweroff(
                invocation,
                &name,
                Duration::from_secs(30).min(invocation.timeout),
            )
            .await
            {
                Ok(()) => method = "acpi".to_owned(),
                Err(_) => {
                    let forced = runner(invocation)
                        .run(&["controlvm".into(), name.clone(), "poweroff".into()])
                        .await;
                    match forced {
                        Ok(output) if output.exit_code == Some(0) => {
                            stdout.push_str(&output.stdout);
                            stderr.push_str(&output.stderr);
                            if let Err(error) =
                                wait_for_poweroff(invocation, &name, Duration::from_secs(10)).await
                            {
                                return failed(
                                    stdout,
                                    stderr,
                                    format!("forced poweroff did not stop VM: {error}"),
                                    name,
                                );
                            }
                            method = "forced_poweroff".to_owned();
                        }
                        Ok(output) => {
                            return failed(
                                stdout,
                                format!("{}{}", stderr, output.stderr),
                                "poweroff fallback failed".into(),
                                name,
                            );
                        }
                        Err(error) => return failed(stdout, stderr, error, name),
                    }
                }
            }
        }
        Ok(output) => {
            stderr.push_str(&output.stderr);
            let forced = runner(invocation)
                .run(&["controlvm".into(), name.clone(), "poweroff".into()])
                .await;
            match forced {
                Ok(out) if out.exit_code == Some(0) => {
                    stdout.push_str(&out.stdout);
                    stderr.push_str(&out.stderr);
                    if let Err(error) =
                        wait_for_poweroff(invocation, &name, Duration::from_secs(10)).await
                    {
                        return failed(
                            stdout,
                            stderr,
                            format!("forced poweroff did not stop VM: {error}"),
                            name,
                        );
                    }
                    method = "forced_poweroff".to_owned();
                }
                _ => {
                    return failed(
                        stdout,
                        stderr,
                        "graceful shutdown and poweroff both failed".into(),
                        name,
                    );
                }
            }
        }
        Err(error) => return failed(stdout, stderr, error, name),
    }
    TestOutcome {
        status: TestStatus::Passed,
        stdout,
        stderr,
        exit_code,
        diagnostics: String::new(),
        observation: serde_json::json!({"vm_name": name, "shutdown_method": method}),
        evidence: Vec::new(),
    }
}

fn failed(stdout: String, stderr: String, reason: String, name: String) -> TestOutcome {
    TestOutcome {
        status: TestStatus::Failed,
        stdout,
        stderr: format!("{stderr}{reason}"),
        exit_code: Some(1),
        diagnostics: reason,
        observation: serde_json::json!({"vm_name": name}),
        evidence: Vec::new(),
    }
}
