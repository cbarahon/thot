//! Purpose: Executes configured tests in order with prerequisites, repeats, and result persistence.
use chrono::Utc;
use serde_json::{Value, json};
use std::{collections::HashMap, path::Path, time::Instant};
use thot_core::{VMActivityProject, test_results::TestStatus};

use crate::{
    test_execution::{TestInvocation, TestOutcome},
    test_result_store, tests,
    vm_activity::{DeliveryInput, ProjectSnapshot, VmActivityRuntime},
};

pub async fn execute_project_tests(
    project: &VMActivityProject,
    snapshot: &ProjectSnapshot,
    runtime: &VmActivityRuntime,
    input: &DeliveryInput,
    run_id: &str,
    expedient_id: &str,
    report_dir: &Path,
    report_path: &Path,
    report: &mut Value,
) -> Result<(), String> {
    let mut prior: HashMap<String, Vec<bool>> = HashMap::new();
    let mut vm_context = HashMap::<String, String>::new();

    for definition in &project.tests {
        let base_parameters = definition.parameters.into_strings()?;
        let repeats = definition.repeat;
        let mut all_passed = true;
        for index in 0..repeats {
            let test_name = if repeats == 1 {
                definition.name.clone()
            } else {
                format!("{}_{:02}", definition.name, index + 1)
            };
            let unmet = definition.requirements.iter().find(|requirement| {
                prior.get(*requirement).is_none_or(|outcomes| {
                    outcomes.is_empty() || outcomes.iter().any(|passed| !passed)
                })
            });
            let now = Utc::now();
            let start = Instant::now();
            let mut parameters = base_parameters.clone();
            if !parameters.contains_key("vm_name") {
                if let Some(name) = vm_context.get("vm_name") {
                    parameters.insert("vm_name".into(), name.clone());
                }
            }
            let (record_dir, artifacts_dir) =
                test_result_store::paths_for_test(report_dir, &input.filename, &test_name)
                    .map_err(|e| format!("cannot create test record directory: {e}"))?;
            let outcome = if let Some(requirement) = unmet {
                TestOutcome::skipped(format!(
                    "required test {requirement:?} did not pass all repetitions"
                ))
            } else if let Some(reason) = input.validation_error.as_deref() {
                TestOutcome::skipped(format!(
                    "delivery was rejected before test execution: {reason}"
                ))
            } else {
                let invocation = TestInvocation {
                    test_name: test_name.clone(),
                    test_reference: definition.test_ref.clone(),
                    parameters,
                    timeout: std::time::Duration::from_secs(
                        definition.timeout.unwrap_or(project.timeout),
                    ),
                    project_root: snapshot.extracted_root.clone(),
                    project_archive_parent: snapshot.archive_parent.clone(),
                    delivery: input.clone(),
                    report_dir: report_dir.to_path_buf(),
                    artifacts_dir: artifacts_dir.clone(),
                    runtime: runtime.clone(),
                    run_id: run_id.into(),
                    expedient_id: expedient_id.into(),
                };
                tests::dispatch(&invocation).await
            };
            if let Some(name) = outcome.observation.get("vm_name").and_then(Value::as_str) {
                vm_context.insert("vm_name".into(), name.to_owned());
            }
            let passed = outcome.status == TestStatus::Passed;
            all_passed &= passed;
            let ended = Utc::now();
            let result_id = format!("result-{}", uuid::Uuid::new_v4().simple());
            let evidence = outcome
                .evidence
                .iter()
                .map(|item| {
                    Path::new(item)
                        .strip_prefix(report_dir)
                        .unwrap_or(Path::new(item))
                        .to_string_lossy()
                        .into_owned()
                })
                .collect::<Vec<_>>();
            let row = json!({
                "test_result_id": result_id,
                "test_id": test_name,
                "test_reference": definition.test_ref,
                "status": outcome.status,
                "start_time": now.to_rfc3339(),
                "end_time": ended.to_rfc3339(),
                "duration_ms": start.elapsed().as_millis(),
                "exit_code": outcome.exit_code,
                "stdout": outcome.stdout,
                "stderr": outcome.stderr,
                "diagnostics": outcome.diagnostics,
                "observation": outcome.observation,
                "evidence": evidence
            });
            test_result_store::persist_output(
                &record_dir,
                row["stdout"].as_str().unwrap_or(""),
                row["stderr"].as_str().unwrap_or(""),
            )
            .map_err(|e| format!("cannot persist test output: {e}"))?;
            report["test_results"]
                .as_array_mut()
                .ok_or("expedient has no test_results array")?
                .push(row);
            if let Some(list) = report["test_results"].as_array_mut() {
                if let Some(last) = list.last_mut() {
                    last["record_dir"] = json!(
                        record_dir
                            .strip_prefix(report_dir)
                            .unwrap_or(&record_dir)
                            .to_string_lossy()
                            .to_string()
                    );
                    last["artifacts_dir"] = json!(
                        artifacts_dir
                            .strip_prefix(report_dir)
                            .unwrap_or(&artifacts_dir)
                            .to_string_lossy()
                            .to_string()
                    );
                }
            }
            test_result_store::persist_report(report_path, report)
                .map_err(|e| format!("cannot persist expedient after test {test_name}: {e}"))?;
        }
        prior.insert(definition.name.clone(), vec![all_passed]);
    }
    Ok(())
}
