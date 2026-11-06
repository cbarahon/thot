//! Purpose: Executes configured tests in order with prerequisites, repeats, and result persistence.
use chrono::Utc;
use regex::Regex;
use serde_json::{Value, json};
use serde_json_path::JsonPath;
use std::{collections::HashMap, path::Path, time::Instant};
use thot_core::{VMActivityProject, test_results::TestStatus};

use crate::{
    test_execution::{TestInvocation, TestOutcome},
    test_result_store, tests,
    vm_activity::{self, DeliveryInput, ProjectSnapshot, VmActivityRuntime},
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
    let mut vm_isolation_error: Option<String> = None;

    for definition in &project.tests {
        let mut base_parameters = definition.parameters.into_strings()?;
        let filter_grep = base_parameters
            .remove("filter_grep")
            .map(|encoded| crate::project_config::parse_filter_grep(&encoded))
            .transpose()?;
        let filter_jsonpath = base_parameters
            .remove("filter_jsonpath")
            .map(|encoded| crate::project_config::parse_filter_jsonpath(&encoded))
            .transpose()?;
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
            let mut outcome = if let Some(requirement) = unmet {
                TestOutcome::skipped(format!(
                    "required test {requirement:?} did not pass all repetitions"
                ))
            } else if let Some(reason) = input.validation_error.as_deref() {
                TestOutcome::skipped(format!(
                    "delivery was rejected before test execution: {reason}"
                ))
            } else if let Some(reason) = vm_isolation_error.as_deref() {
                TestOutcome::skipped(format!(
                    "VM tests were stopped because host-isolation setup failed: {reason}"
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
                    prior_test_results: report["test_results"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default(),
                };
                tests::dispatch(&invocation).await
            };
            if let Some(name) = outcome.observation.get("vm_name").and_then(Value::as_str) {
                vm_context.insert("vm_name".into(), name.to_owned());
            }
            if definition.test_ref == "VirtualBox/import_ova"
                && outcome.status == TestStatus::Passed
            {
                if let Some(uuid) = outcome.observation.get("vm_uuid").and_then(Value::as_str) {
                    if let Err(error) = vm_activity::disable_guest_features(runtime, uuid).await {
                        vm_isolation_error = Some(error);
                        report["orchestration_error"] = json!(format!(
                            "imported VM host-isolation setup failed: {}",
                            vm_isolation_error.as_deref().unwrap_or_default()
                        ));
                    }
                }
            }
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
            let raw_stdout = outcome.stdout.clone();
            let raw_stderr = outcome.stderr.clone();
            test_result_store::persist_output(&record_dir, &raw_stdout, &raw_stderr)
                .map_err(|e| format!("cannot persist test output: {e}"))?;
            if outcome.status != TestStatus::Skipped {
                if let Err(error) =
                    filter_stdout(&mut outcome, filter_grep.as_ref(), filter_jsonpath.as_ref())
                {
                    outcome.status = TestStatus::Error;
                    outcome.stdout.clear();
                    if !outcome.stderr.is_empty() && !outcome.stderr.ends_with('\n') {
                        outcome.stderr.push('\n');
                    }
                    outcome
                        .stderr
                        .push_str(&format!("stdout filter failed: {error}"));
                    outcome.diagnostics = format!("stdout filter failed: {error}");
                }
            }
            all_passed &= outcome.status == TestStatus::Passed;
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
                "evidence": evidence
            });
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

fn filter_stdout(
    outcome: &mut TestOutcome,
    filter_grep: Option<&crate::project_config::FilterGrep>,
    filter_jsonpath: Option<&crate::project_config::FilterJsonPath>,
) -> Result<(), String> {
    if let Some(filter) = filter_jsonpath {
        let document =
            crate::checks::parse_document(&Value::String(outcome.stdout.clone()), "stdout")?;
        let filtered = match filter {
            crate::project_config::FilterJsonPath::Expressions(expressions) => {
                let mut selected = Vec::new();
                for expression in expressions {
                    let path = JsonPath::parse(expression).map_err(|error| {
                        format!("invalid filter_jsonpath expression {expression:?}: {error}")
                    })?;
                    selected.extend(path.query(&document).all().into_iter().cloned());
                }
                match selected.len() {
                    0 => {
                        return Err(format!(
                            "filter_jsonpath expressions {expressions:?} selected no values"
                        ));
                    }
                    1 => selected.into_iter().next().expect("one selected value"),
                    _ => Value::Array(selected),
                }
            }
            crate::project_config::FilterJsonPath::Labeled(expressions) => {
                let mut selected = serde_json::Map::new();
                for (label, expression) in expressions {
                    let path = JsonPath::parse(expression).map_err(|error| {
                        format!("invalid filter_jsonpath expression for label {label:?}: {error}")
                    })?;
                    let values = path
                        .query(&document)
                        .all()
                        .into_iter()
                        .cloned()
                        .collect::<Vec<_>>();
                    let value = match values.len() {
                        0 => {
                            return Err(format!(
                                "filter_jsonpath label {label:?} selected no values with {expression:?}"
                            ));
                        }
                        1 => values.into_iter().next().expect("one selected value"),
                        _ => Value::Array(values),
                    };
                    selected.insert(label.clone(), value);
                }
                Value::Object(selected)
            }
        };
        outcome.stdout = serde_yaml::to_string(&filtered)
            .map_err(|error| format!("cannot serialize filter_jsonpath result as YAML: {error}"))?;
    }
    if let Some(filter) = filter_grep {
        match filter {
            crate::project_config::FilterGrep::Expressions(expressions) => {
                let regexes = expressions
                    .iter()
                    .map(|expression| {
                        Regex::new(expression).map_err(|error| {
                            format!("invalid filter_grep expression {expression:?}: {error}")
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                outcome.stdout = outcome
                    .stdout
                    .split_inclusive('\n')
                    .filter(|line| {
                        regexes
                            .iter()
                            .any(|regex| regex.is_match(line.trim_end_matches(['\r', '\n'])))
                    })
                    .collect();
            }
            crate::project_config::FilterGrep::Labeled(expressions) => {
                let mut selected = serde_json::Map::new();
                for (label, expression) in expressions {
                    let regex = Regex::new(expression).map_err(|error| {
                        format!("invalid filter_grep expression for label {label:?}: {error}")
                    })?;
                    let lines = outcome
                        .stdout
                        .lines()
                        .filter(|line| regex.is_match(line))
                        .map(str::to_owned)
                        .collect::<Vec<_>>();
                    let value = match lines.len() {
                        0 => {
                            return Err(format!(
                                "filter_grep label {label:?} selected no lines with {expression:?}"
                            ));
                        }
                        1 => Value::String(lines.into_iter().next().expect("one selected line")),
                        _ => Value::Array(lines.into_iter().map(Value::String).collect()),
                    };
                    selected.insert(label.clone(), value);
                }
                outcome.stdout =
                    serde_yaml::to_string(&Value::Object(selected)).map_err(|error| {
                        format!("cannot serialize labeled filter_grep result as YAML: {error}")
                    })?;
            }
        }
    }
    Ok(())
}
