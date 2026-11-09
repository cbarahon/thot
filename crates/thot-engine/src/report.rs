//! Purpose: Builds run, delivery, test/check/rubric statistics, and expedient matrix reports.
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sqlx::SqlitePool;
use std::collections::{BTreeMap, HashSet};

use crate::{delivery_run_service::ServiceError, project_config};
use thot_core::{ProjectTest, VMActivityProject};

struct DeliveryData {
    delivery_id: String,
    filename: String,
    student_count: i64,
    status: String,
    report: Value,
}

#[derive(Clone)]
struct ExpedientData {
    expedient_id: String,
    delivery_filename: String,
    status: String,
    report: Value,
}

pub(crate) async fn runs(
    pool: &SqlitePool,
    artifact_root: &std::path::Path,
) -> Result<Value, ServiceError> {
    let run_rows = sqlx::query_as::<_, (String, String, String, String)>(
        "SELECT run_id, project_id, created_at, status FROM vm_activity_runs ORDER BY created_at DESC, run_id DESC",
    )
    .fetch_all(pool)
    .await?;
    let mut result = Vec::with_capacity(run_rows.len());
    for (run_id, project_id, created_at, status) in run_rows {
        let deliveries = load_deliveries(pool, &run_id).await?;
        let configured_tests = configured_test_names(artifact_root, &run_id);
        let test_names = combined_test_names(&configured_tests, &deliveries);
        let active = matches!(status.as_str(), "queued" | "running" | "cancelling");
        let mut counts = TestCounts::default();
        let mut started_at = None::<String>;
        let mut finished_at = None::<String>;
        for delivery in &deliveries {
            let by_name = delivery_test_statuses(&delivery.report);
            let running_test = currently_running_test(&delivery.report, &test_names);
            for test_name in &test_names {
                let status = by_name
                    .get(test_name)
                    .copied()
                    .or_else(|| (running_test == Some(test_name.as_str())).then_some("running"))
                    .unwrap_or("not_run");
                match status {
                    "not_run" | "pending" | "queued" => counts.pending += 1,
                    "running" => counts.running += 1,
                    status => {
                        counts.finished += 1;
                        if is_error_status(status) {
                            counts.finished_with_errors += 1;
                        } else {
                            counts.finished_without_errors += 1;
                        }
                    }
                }
            }
            if let Some(value) = delivery.report["started_at"].as_str() {
                choose_earlier(&mut started_at, value);
            }
            if let Some(value) = delivery.report["finished_at"].as_str() {
                choose_later(&mut finished_at, value);
            }
        }
        if started_at.is_none() {
            started_at = Some(created_at.clone());
        }
        let visible_finished_at = if active { None } else { finished_at.clone() };
        let duration_ms = started_at.as_deref().and_then(|start| {
            let start = parse_time(start)?;
            let end = if active {
                Utc::now()
            } else {
                parse_time(visible_finished_at.as_deref()?)?
            };
            Some((end - start).num_milliseconds().max(0) as u64)
        });
        let mut run_report = json!({
            "run_id": run_id,
            "project_id": project_id,
            "status": status,
            "started_at": started_at,
            "finished_at": visible_finished_at,
            "duration_ms": duration_ms,
            "tests_pending": counts.pending,
            "tests_running": counts.running,
            "tests_finished": counts.finished,
            "tests_finished_without_errors": counts.finished_without_errors,
            "tests_finished_with_errors": counts.finished_with_errors,
        });
        if active {
            run_report["deliveries"] = json!(
                deliveries
                    .iter()
                    .map(|delivery| delivery_progress(delivery, &test_names))
                    .collect::<Vec<_>>()
            );
        }
        result.push(run_report);
    }
    Ok(json!({"runs": result}))
}

pub(crate) async fn deliveries(
    pool: &SqlitePool,
    artifact_root: &std::path::Path,
    requested_run_id: Option<&str>,
    view: &str,
) -> Result<Value, ServiceError> {
    if !matches!(view, "summary" | "tests" | "checks") {
        return Err(ServiceError::Invalid(format!(
            "unknown delivery report view {view:?}; use summary, tests, or checks"
        )));
    }
    let run = if let Some(run_id) = requested_run_id {
        sqlx::query_as::<_, (String, String)>(
            "SELECT run_id, project_id FROM vm_activity_runs WHERE run_id=?",
        )
        .bind(run_id)
        .fetch_optional(pool)
        .await?
    } else {
        sqlx::query_as::<_, (String, String)>(
            "SELECT run_id, project_id FROM vm_activity_runs ORDER BY created_at DESC, run_id DESC LIMIT 1",
        )
        .fetch_optional(pool)
        .await?
    };
    let Some((run_id, _project_id)) = run else {
        if requested_run_id.is_some() {
            return Err(ServiceError::Invalid(format!(
                "run {:?} was not found",
                requested_run_id.unwrap_or_default()
            )));
        }
        return Ok(json!({
            "run_id": null,
            "deliveries": [],
            "test_names": [],
            "check_names": []
        }));
    };

    let deliveries = load_deliveries(pool, &run_id).await?;
    let test_names =
        combined_test_names(&configured_test_names(artifact_root, &run_id), &deliveries);
    let check_names = combined_check_names(artifact_root, &run_id, &deliveries);
    let rows = deliveries
        .iter()
        .map(|delivery| {
            let delivery_test_statuses = delivery_test_statuses(&delivery.report);
            let running_test = currently_running_test(&delivery.report, &test_names);
            let tests = test_names
                .iter()
                .map(|name| {
                    let status = delivery_test_statuses
                        .get(name)
                        .copied()
                        .or_else(|| (running_test == Some(name.as_str())).then_some("running"))
                        .map(test_status_label)
                        .unwrap_or("pending");
                    (name.clone(), json!(status))
                })
                .collect::<serde_json::Map<_, _>>();
            let checks = check_names
                .iter()
                .map(|name| {
                    let result = delivery.report["checks"].as_array().and_then(|rows| {
                        rows.iter()
                            .find(|row| row["name"].as_str() == Some(name.as_str()))
                    });
                    let label = result.map(check_status_label).unwrap_or("pending");
                    (name.clone(), json!(label))
                })
                .collect::<serde_json::Map<_, _>>();
            let tests_passed = tests.values().filter(|value| *value == "pass").count();
            let tests_failed = tests.values().filter(|value| *value == "error").count();
            let checks_passed = checks.values().filter(|value| *value == "pass").count();
            let checks_failed = checks.values().filter(|value| *value == "error").count();
            let mut row = json!({
                "delivery_id": delivery.delivery_id,
                "filename": delivery.filename,
                "student_count": delivery.student_count,
                "tests_passed": tests_passed,
                "tests_failed": tests_failed,
                "checks_passed": checks_passed,
                "checks_failed": checks_failed,
            });
            if view == "tests" {
                row["tests"] = Value::Object(tests);
            }
            if view == "checks" {
                row["checks"] = Value::Object(checks);
            }
            row
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "run_id": run_id,
        "deliveries": rows,
        "test_names": test_names,
        "check_names": check_names,
    }))
}

pub(crate) async fn test_stats(
    pool: &SqlitePool,
    artifact_root: &std::path::Path,
    requested_run_id: Option<&str>,
) -> Result<Value, ServiceError> {
    let (run_id, _run_status, project, expedients) =
        load_run_expedients(pool, artifact_root, requested_run_id).await?;
    let tested = expedients_with_deliveries(&expedients);
    let names = test_names(project.as_ref(), &tested);
    let statistics = names
        .iter()
        .map(|name| test_statistic(name, project.as_ref(), &names, &tested))
        .collect::<Vec<_>>();
    Ok(json!({"run_id": run_id, "statistics": statistics}))
}

pub(crate) async fn checks_stats(
    pool: &SqlitePool,
    artifact_root: &std::path::Path,
    requested_run_id: Option<&str>,
) -> Result<Value, ServiceError> {
    let (run_id, run_status, project, expedients) =
        load_run_expedients(pool, artifact_root, requested_run_id).await?;
    let tested = expedients_with_deliveries(&expedients);
    let checks_in_progress = is_check_phase(&run_status, &tested);
    let names = check_names(project.as_ref(), &tested);
    let statistics = names
        .iter()
        .map(|name| check_statistic(name, &tested, checks_in_progress))
        .collect::<Vec<_>>();
    Ok(json!({"run_id": run_id, "statistics": statistics}))
}

pub(crate) async fn rubric_stats(
    pool: &SqlitePool,
    artifact_root: &std::path::Path,
    requested_run_id: Option<&str>,
) -> Result<Value, ServiceError> {
    let (run_id, run_status, project, expedients) =
        load_run_expedients(pool, artifact_root, requested_run_id).await?;
    let tested = expedients_with_deliveries(&expedients);
    let checks_in_progress = is_check_phase(&run_status, &tested);
    let names = rubric_names(project.as_ref(), &tested);
    let statistics = names
        .iter()
        .map(|name| rubric_statistic(name, &tested, checks_in_progress))
        .collect::<Vec<_>>();
    Ok(json!({"run_id": run_id, "statistics": statistics}))
}

pub(crate) async fn expedient_matrices(
    pool: &SqlitePool,
    artifact_root: &std::path::Path,
    requested_run_id: Option<&str>,
) -> Result<Value, ServiceError> {
    let (run_id, run_status, project, expedients) =
        load_run_expedients(pool, artifact_root, requested_run_id).await?;
    let checks_in_progress = is_check_phase(&run_status, &expedients);
    let tests = test_names(project.as_ref(), &expedients);
    let checks = check_names(project.as_ref(), &expedients);
    let rubrics = rubric_names(project.as_ref(), &expedients);
    let evaluation = evaluation_names(project.as_ref(), &expedients);
    Ok(json!({
        "run_id": run_id,
        "expedient_x_test": test_matrix(&tests, &expedients),
        "expedient_x_check": check_matrix(&checks, &expedients, checks_in_progress),
        "expedient_x_rubric": rubric_matrix(&rubrics, &expedients, checks_in_progress),
        "expedient_x_evaluation": evaluation_matrix(&evaluation, &expedients, checks_in_progress),
    }))
}

async fn load_run_expedients(
    pool: &SqlitePool,
    artifact_root: &std::path::Path,
    requested_run_id: Option<&str>,
) -> Result<
    (
        String,
        String,
        Option<VMActivityProject>,
        Vec<ExpedientData>,
    ),
    ServiceError,
> {
    let run = if let Some(run_id) = requested_run_id {
        sqlx::query_as::<_, (String, String)>(
            "SELECT run_id, status FROM vm_activity_runs WHERE run_id=?",
        )
        .bind(run_id)
        .fetch_optional(pool)
        .await?
    } else {
        sqlx::query_as::<_, (String, String)>(
            "SELECT run_id, status FROM vm_activity_runs ORDER BY created_at DESC, run_id DESC LIMIT 1",
        )
        .fetch_optional(pool)
        .await?
    };
    let Some((run_id, run_status)) = run else {
        return Err(ServiceError::Invalid(
            if let Some(run_id) = requested_run_id {
                format!("run {run_id:?} was not found")
            } else {
                "no runs are available; create a run before requesting reports".into()
            },
        ));
    };
    let project = load_project(artifact_root, &run_id);
    let rows = sqlx::query_as::<_, (String, Option<i64>, String, String, String)>(
        "SELECT e.expedient_id, e.student_id, COALESCE(CASE WHEN e.delivery_id='__thot_unassigned__' THEN '' ELSE d.original_filename END,''), e.status, e.report_path FROM vm_activity_expedients e LEFT JOIN vm_activity_deliveries d ON d.delivery_id=e.delivery_id WHERE e.run_id=? ORDER BY e.student_id, e.expedient_id",
    )
    .bind(&run_id)
    .fetch_all(pool)
    .await?;
    let mut expedients = Vec::with_capacity(rows.len());
    for (expedient_id, _student_id, delivery_filename, status, report_path) in rows {
        let report = match tokio::fs::read(&report_path).await {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| {
                ServiceError::Internal(format!(
                    "saved report for expedient {expedient_id:?} is invalid JSON: {error}"
                ))
            })?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => json!({}),
            Err(error) => {
                return Err(ServiceError::Internal(format!(
                    "cannot read report for expedient {expedient_id:?} at {report_path}: {error}"
                )));
            }
        };
        expedients.push(ExpedientData {
            expedient_id,
            delivery_filename,
            status,
            report,
        });
    }
    Ok((run_id, run_status, project, expedients))
}

fn load_project(artifact_root: &std::path::Path, run_id: &str) -> Option<VMActivityProject> {
    let config_path = artifact_root
        .join("project-snapshots")
        .join(run_id)
        .join("project/configuration.yaml");
    let bytes = std::fs::read(config_path).ok()?;
    project_config::parse_configuration(&bytes).ok()
}

fn test_names(project: Option<&VMActivityProject>, expedients: &[ExpedientData]) -> Vec<String> {
    let configured = project
        .map(|project| expanded_test_names(&project.tests))
        .unwrap_or_default();
    combined_names(configured, expedients, |report| {
        report["test_results"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|row| row["test_id"].as_str().map(str::to_owned))
            .collect()
    })
}

fn expedients_with_deliveries(expedients: &[ExpedientData]) -> Vec<ExpedientData> {
    expedients
        .iter()
        .filter(|expedient| {
            expedient.report["delivery_id"]
                .as_str()
                .is_some_and(|delivery_id| delivery_id != "__thot_unassigned__")
        })
        .cloned()
        .collect()
}

fn check_names(project: Option<&VMActivityProject>, expedients: &[ExpedientData]) -> Vec<String> {
    let configured = project
        .map(|project| {
            project
                .checks
                .iter()
                .filter(|check| check.check_ref != "import/csv")
                .map(|check| check.name.clone())
                .collect()
        })
        .unwrap_or_default();
    combined_names(configured, expedients, |report| {
        report["checks"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|row| row["name"].as_str().map(str::to_owned))
            .collect()
    })
}

fn rubric_names(project: Option<&VMActivityProject>, expedients: &[ExpedientData]) -> Vec<String> {
    let configured = project
        .map(|project| {
            project
                .rubrics
                .iter()
                .filter(|rubric| rubric.calculation != "import/csv")
                .map(|rubric| rubric.name.clone())
                .collect()
        })
        .unwrap_or_default();
    combined_names(configured, expedients, |report| {
        rubric_results(report)
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|row| row["name"].as_str().map(str::to_owned))
            .collect()
    })
}

fn evaluation_names(
    project: Option<&VMActivityProject>,
    expedients: &[ExpedientData],
) -> Vec<String> {
    let configured = project
        .map(|project| {
            project
                .evaluation
                .iter()
                .filter(|item| item.calculation != "import/csv")
                .map(|item| item.name.clone())
                .collect()
        })
        .unwrap_or_default();
    combined_names(configured, expedients, |report| {
        report["evaluation"]["results"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|row| row["name"].as_str().map(str::to_owned))
            .collect()
    })
}

fn is_check_phase(run_status: &str, expedients: &[ExpedientData]) -> bool {
    matches!(run_status, "queued" | "running" | "cancelling")
        && expedients
            .iter()
            .all(|expedient| expedient.status == "finished")
}

fn combined_names(
    mut names: Vec<String>,
    expedients: &[ExpedientData],
    values: impl Fn(&Value) -> Vec<String>,
) -> Vec<String> {
    let mut known = names.iter().cloned().collect::<HashSet<_>>();
    for expedient in expedients {
        for name in values(&expedient.report) {
            if known.insert(name.clone()) {
                names.push(name);
            }
        }
    }
    names
}

fn expanded_test_names(tests: &[ProjectTest]) -> Vec<String> {
    tests
        .iter()
        .flat_map(|test| {
            if test.repeat > 1 {
                (1..=test.repeat)
                    .map(|index| format!("{}_{index:02}", test.name))
                    .collect::<Vec<_>>()
            } else {
                vec![test.name.clone()]
            }
        })
        .collect()
}

fn test_statistic(
    name: &str,
    project: Option<&VMActivityProject>,
    test_names: &[String],
    expedients: &[ExpedientData],
) -> Value {
    let definition = project.and_then(|project| {
        project.tests.iter().find(|test| {
            if test.repeat > 1 {
                (1..=test.repeat).any(|index| format!("{}_{index:02}", test.name) == name)
            } else {
                test.name == name
            }
        })
    });
    let timeout_seconds = definition.map(|test| test.timeout.unwrap_or(project.unwrap().timeout));
    let mut passed = 0;
    let mut failed = 0;
    let mut errors = 0;
    let mut skipped = 0;
    let mut pending = 0;
    let mut running = 0;
    let mut durations = Vec::new();
    for expedient in expedients {
        let result = expedient.report["test_results"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|row| row["test_id"].as_str() == Some(name));
        let status = result
            .and_then(|row| row["status"].as_str())
            .or_else(|| {
                running_test_for_expedient(expedient, test_names)
                    .filter(|running_name| *running_name == name)
                    .map(|_| "running")
            })
            .unwrap_or("not_run");
        match status {
            "passed" => passed += 1,
            "failed" => failed += 1,
            "error" | "timed_out" | "cancelled" => errors += 1,
            "skipped" => skipped += 1,
            "running" => running += 1,
            _ => pending += 1,
        }
        if let Some(duration) = result.and_then(|row| row["duration_ms"].as_u64()) {
            durations.push(duration as f64);
        }
    }
    let completed = passed + failed + errors;
    let success_rate_percent = percentage(passed, completed);
    let duration_summary = numeric_summary(&mut durations);
    let mut signals = Vec::new();
    if completed > 0 && passed == 0 {
        signals.push(
            "no expedient passed; verify test setup, assumptions, and prerequisites".to_owned(),
        );
    }
    if completed >= 3 && passed == completed {
        signals.push(
            "all observed expedients passed; the test may have low discriminative power".to_owned(),
        );
    }
    if expedients.len() >= 3 && skipped * 2 >= expedients.len() {
        signals.push(
            "at least half of expedients skipped this test; review its prerequisites".to_owned(),
        );
    }
    if duration_summary
        .avg
        .is_some_and(|average| average >= 30_000.0)
    {
        signals.push("average execution time is at least 30 seconds".to_owned());
    }
    if let (Some(timeout), Some(p95)) = (timeout_seconds, duration_summary.p95)
        && p95 >= timeout as f64 * 800.0
    {
        signals.push("95th percentile is within 20% of the configured timeout".to_owned());
    }
    json!({
        "name": name,
        "test_reference": definition.map(|test| test.test_ref.as_str()),
        "timeout_seconds": timeout_seconds,
        "total_expedients": expedients.len(),
        "completed_successfully": passed,
        "failed": failed,
        "errors": errors,
        "skipped": skipped,
        "pending": pending,
        "running": running,
        "success_rate_percent": success_rate_percent,
        "duration_samples": duration_summary.count,
        "avg_duration_ms": duration_summary.avg,
        "median_duration_ms": duration_summary.median,
        "p95_duration_ms": duration_summary.p95,
        "max_duration_ms": duration_summary.max,
        "signals": signals,
    })
}

fn check_statistic(name: &str, expedients: &[ExpedientData], checks_in_progress: bool) -> Value {
    let mut passed = 0;
    let mut failed = 0;
    let mut errors = 0;
    let mut pending = 0;
    let mut running = 0;
    let mut durations = Vec::new();
    for expedient in expedients {
        let result = find_named_result(&expedient.report["checks"], name);
        match result {
            Some(row) if !row["err"].is_null() => errors += 1,
            Some(row) if row["result"].as_i64() == Some(1) => passed += 1,
            Some(_) => failed += 1,
            None if checks_in_progress => running += 1,
            None => pending += 1,
        }
        if let Some(duration) = result.and_then(|row| row["duration_ms"].as_u64()) {
            durations.push(duration as f64);
        }
    }
    let completed = passed + failed + errors;
    let success_rate_percent = percentage(passed, passed + failed);
    let duration_summary = numeric_summary(&mut durations);
    let mut signals = Vec::new();
    if completed > 0 && passed == 0 {
        signals.push("no expedient passed; review the check rule or its source test".to_owned());
    }
    if completed >= 3 && passed == completed {
        signals.push(
            "all observed expedients passed; the check may have low discriminative power"
                .to_owned(),
        );
    }
    if errors > 0 {
        signals.push("one or more check evaluations returned errors".to_owned());
    }
    if duration_summary
        .avg
        .is_some_and(|average| average >= 30_000.0)
    {
        signals.push("average check evaluation time is at least 30 seconds".to_owned());
    }
    json!({
        "name": name,
        "total_expedients": expedients.len(),
        "completed_successfully": passed,
        "failed": failed,
        "errors": errors,
        "pending": pending,
        "running": running,
        "success_rate_percent": success_rate_percent,
        "duration_samples": duration_summary.count,
        "avg_duration_ms": duration_summary.avg,
        "median_duration_ms": duration_summary.median,
        "p95_duration_ms": duration_summary.p95,
        "max_duration_ms": duration_summary.max,
        "signals": signals,
    })
}

fn rubric_statistic(name: &str, expedients: &[ExpedientData], checks_in_progress: bool) -> Value {
    let mut calculated = 0;
    let mut errors = 0;
    let mut pending = 0;
    let mut running = 0;
    let mut scores = Vec::new();
    let mut durations = Vec::new();
    for expedient in expedients {
        let result = find_named_result(rubric_results(&expedient.report), name);
        match result {
            Some(row) if !row["err"].is_null() => errors += 1,
            Some(row) if row["result"].as_f64().is_some() => {
                calculated += 1;
                scores.push(row["result"].as_f64().unwrap_or_default());
            }
            Some(_) => errors += 1,
            None if checks_in_progress => running += 1,
            None => pending += 1,
        }
        if let Some(duration) = result.and_then(|row| row["duration_ms"].as_u64()) {
            durations.push(duration as f64);
        }
    }
    let score_summary = numeric_summary(&mut scores);
    let duration_summary = numeric_summary(&mut durations);
    let mut signals = Vec::new();
    if expedients.len() > 0 && calculated == 0 && errors > 0 {
        signals.push(
            "no rubric mark was calculated; inspect rubric errors and required checks".to_owned(),
        );
    }
    if score_summary.count >= 3 && score_summary.min == score_summary.max {
        signals.push(
            "all calculated marks are identical; the rubric may have low discrimination".to_owned(),
        );
    }
    if errors > 0 {
        signals.push("one or more rubric calculations returned errors".to_owned());
    }
    if duration_summary
        .avg
        .is_some_and(|average| average >= 30_000.0)
    {
        signals.push("average rubric calculation time is at least 30 seconds".to_owned());
    }
    json!({
        "name": name,
        "total_expedients": expedients.len(),
        "completed_successfully": calculated,
        "failed": errors,
        "errors": errors,
        "pending": pending,
        "running": running,
        "score_samples": score_summary.count,
        "avg_score": score_summary.avg,
        "median_score": score_summary.median,
        "p10_score": score_summary.p10,
        "p90_score": score_summary.p90,
        "min_score": score_summary.min,
        "max_score": score_summary.max,
        "stddev_score": score_summary.stddev,
        "duration_samples": duration_summary.count,
        "avg_duration_ms": duration_summary.avg,
        "p95_duration_ms": duration_summary.p95,
        "max_duration_ms": duration_summary.max,
        "signals": signals,
    })
}

fn find_named_result<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    value
        .as_array()?
        .iter()
        .find(|row| row["name"].as_str() == Some(name))
}

fn running_test_for_expedient<'a>(
    expedient: &ExpedientData,
    test_names: &'a [String],
) -> Option<&'a str> {
    currently_running_test(&expedient.report, test_names).or_else(|| {
        (expedient.status == "running"
            && expedient.report["test_results"]
                .as_array()
                .is_none_or(Vec::is_empty))
        .then(|| test_names.first().map(String::as_str))
        .flatten()
    })
}

fn test_matrix(names: &[String], expedients: &[ExpedientData]) -> Value {
    let rows = expedients
        .iter()
        .map(|expedient| {
            let running = running_test_for_expedient(expedient, names);
            let results = names
                .iter()
                .map(|name| {
                    let row = expedient.report["test_results"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .find(|row| row["test_id"].as_str() == Some(name));
                    let value = row
                        .and_then(|row| row["status"].as_str())
                        .map(test_matrix_marker)
                        .unwrap_or_else(|| {
                            if running == Some(name.as_str()) {
                                "R"
                            } else {
                                ""
                            }
                        });
                    (name.clone(), json!(value))
                })
                .collect::<serde_json::Map<_, _>>();
            json!({
                "expedient_id": expedient.expedient_id,
                "delivery_filename": expedient.delivery_filename,
                "values": results,
            })
        })
        .collect::<Vec<_>>();
    json!({"columns": names, "rows": rows})
}

fn check_matrix(names: &[String], expedients: &[ExpedientData], checks_in_progress: bool) -> Value {
    let rows = expedients
        .iter()
        .map(|expedient| {
            let results = names
                .iter()
                .map(|name| {
                    let row = find_named_result(&expedient.report["checks"], name);
                    let value = row
                        .map(check_matrix_marker)
                        .unwrap_or_else(|| if checks_in_progress { "R" } else { "" });
                    (name.clone(), json!(value))
                })
                .collect::<serde_json::Map<_, _>>();
            json!({
                "expedient_id": expedient.expedient_id,
                "delivery_filename": expedient.delivery_filename,
                "values": results,
            })
        })
        .collect::<Vec<_>>();
    json!({"columns": names, "rows": rows})
}

fn rubric_matrix(
    names: &[String],
    expedients: &[ExpedientData],
    checks_in_progress: bool,
) -> Value {
    let rows = expedients
        .iter()
        .map(|expedient| {
            let results = names
                .iter()
                .map(|name| {
                    let row = find_named_result(rubric_results(&expedient.report), name);
                    let value = row.map(rubric_matrix_marker).unwrap_or_else(|| {
                        if checks_in_progress {
                            "R".into()
                        } else {
                            String::new()
                        }
                    });
                    (name.clone(), json!(value))
                })
                .collect::<serde_json::Map<_, _>>();
            json!({
                "expedient_id": expedient.expedient_id,
                "delivery_filename": expedient.delivery_filename,
                "values": results,
            })
        })
        .collect::<Vec<_>>();
    json!({"columns": names, "rows": rows})
}

fn evaluation_matrix(
    names: &[String],
    expedients: &[ExpedientData],
    checks_in_progress: bool,
) -> Value {
    let rows = expedients
        .iter()
        .map(|expedient| {
            let results = names
                .iter()
                .map(|name| {
                    let row = find_named_result(&expedient.report["evaluation"]["results"], name);
                    let value = row.map(rubric_matrix_marker).unwrap_or_else(|| {
                        if checks_in_progress {
                            "R".into()
                        } else {
                            String::new()
                        }
                    });
                    (name.clone(), json!(value))
                })
                .collect::<serde_json::Map<_, _>>();
            json!({
                "expedient_id": expedient.expedient_id,
                "delivery_filename": expedient.delivery_filename,
                "values": results,
            })
        })
        .collect::<Vec<_>>();
    json!({"columns": names, "rows": rows})
}

fn rubric_results(report: &Value) -> &Value {
    if report["rubrics"].is_array() {
        &report["rubrics"]
    } else {
        // Read expedients written before rubric results had their own top-level section.
        &report["evaluation"]["results"]
    }
}

fn test_matrix_marker(status: &str) -> &'static str {
    match status {
        "passed" => "Ok",
        "failed" | "error" | "timed_out" | "cancelled" => "X",
        "running" => "R",
        _ => "",
    }
}

fn check_matrix_marker(row: &Value) -> &'static str {
    if !row["err"].is_null() || row["result"].as_i64() == Some(0) {
        "X"
    } else if row["result"].as_i64() == Some(1) {
        "Ok"
    } else {
        ""
    }
}

fn rubric_matrix_marker(row: &Value) -> String {
    if !row["err"].is_null() {
        return "X".into();
    }
    let Some(score) = row["result"].as_f64() else {
        return String::new();
    };
    let mut value = format!("{score:.3}");
    while value.ends_with('0') {
        value.pop();
    }
    if value.ends_with('.') {
        value.pop();
    }
    value
}

struct NumericSummary {
    count: usize,
    avg: Option<f64>,
    median: Option<f64>,
    p10: Option<f64>,
    p90: Option<f64>,
    p95: Option<f64>,
    min: Option<f64>,
    max: Option<f64>,
    stddev: Option<f64>,
}

fn numeric_summary(values: &mut [f64]) -> NumericSummary {
    if values.is_empty() {
        return NumericSummary {
            count: 0,
            avg: None,
            median: None,
            p10: None,
            p90: None,
            p95: None,
            min: None,
            max: None,
            stddev: None,
        };
    }
    values.sort_by(f64::total_cmp);
    let avg = values.iter().sum::<f64>() / values.len() as f64;
    let variance = values
        .iter()
        .map(|value| (value - avg).powi(2))
        .sum::<f64>()
        / values.len() as f64;
    NumericSummary {
        count: values.len(),
        avg: Some(avg),
        median: percentile(values, 50.0),
        p10: percentile(values, 10.0),
        p90: percentile(values, 90.0),
        p95: percentile(values, 95.0),
        min: values.first().copied(),
        max: values.last().copied(),
        stddev: Some(variance.sqrt()),
    }
}

fn percentile(values: &[f64], percentile: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let index =
        (percentile / 100.0 * (values.len() - 1) as f64).clamp(0.0, (values.len() - 1) as f64);
    let lower = index.floor() as usize;
    let upper = index.ceil() as usize;
    let fraction = index - lower as f64;
    Some(values[lower] * (1.0 - fraction) + values[upper] * fraction)
}

fn percentage(part: usize, total: usize) -> Option<f64> {
    (total > 0).then(|| part as f64 * 100.0 / total as f64)
}

async fn load_deliveries(
    pool: &SqlitePool,
    run_id: &str,
) -> Result<Vec<DeliveryData>, ServiceError> {
    let rows = sqlx::query_as::<_, (String, String, String, i64, i64, i64)>(
        "SELECT e.delivery_id, (SELECT e2.report_path FROM vm_activity_expedients e2 WHERE e2.run_id=e.run_id AND e2.delivery_id=e.delivery_id ORDER BY e2.student_id, e2.expedient_id LIMIT 1), d.original_filename, COUNT(*), MAX(CASE WHEN e.status='running' THEN 1 ELSE 0 END), MAX(CASE WHEN e.status='pending' THEN 1 ELSE 0 END) FROM vm_activity_expedients e JOIN vm_activity_deliveries d ON d.delivery_id=e.delivery_id WHERE e.run_id=? AND e.delivery_id!='__thot_unassigned__' GROUP BY e.delivery_id, d.original_filename ORDER BY d.original_filename COLLATE NOCASE, e.delivery_id",
    )
    .bind(run_id)
    .fetch_all(pool)
    .await?;
    let mut deliveries = Vec::with_capacity(rows.len());
    for (delivery_id, report_path, filename, student_count, has_running, has_pending) in rows {
        let report = match tokio::fs::read(&report_path).await {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| {
                ServiceError::Internal(format!(
                    "saved report for delivery {delivery_id:?} is invalid JSON: {error}"
                ))
            })?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => json!({}),
            Err(error) => {
                return Err(ServiceError::Internal(format!(
                    "cannot read report for delivery {delivery_id:?} at {report_path}: {error}"
                )));
            }
        };
        deliveries.push(DeliveryData {
            delivery_id,
            filename,
            student_count,
            status: if has_running != 0 {
                "running".into()
            } else if has_pending != 0 {
                "pending".into()
            } else {
                "finished".into()
            },
            report,
        });
    }
    Ok(deliveries)
}

fn configured_test_names(artifact_root: &std::path::Path, run_id: &str) -> Vec<String> {
    let config_path = artifact_root
        .join("project-snapshots")
        .join(run_id)
        .join("project/configuration.yaml");
    let Ok(bytes) = std::fs::read(config_path) else {
        return Vec::new();
    };
    let Ok(project) = project_config::parse_configuration(&bytes) else {
        return Vec::new();
    };
    project
        .tests
        .iter()
        .flat_map(|test| {
            if test.repeat > 1 {
                (1..=test.repeat)
                    .map(|index| format!("{}_{index:02}", test.name))
                    .collect::<Vec<_>>()
            } else {
                vec![test.name.clone()]
            }
        })
        .collect()
}

fn combined_test_names(configured: &[String], deliveries: &[DeliveryData]) -> Vec<String> {
    let mut names = configured.to_vec();
    let mut known = names.iter().cloned().collect::<HashSet<_>>();
    for delivery in deliveries {
        for row in delivery.report["test_results"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if let Some(name) = row["test_id"].as_str()
                && known.insert(name.to_owned())
            {
                names.push(name.to_owned());
            }
        }
    }
    names
}

fn delivery_test_statuses(report: &Value) -> BTreeMap<String, &str> {
    report["test_results"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| Some((row["test_id"].as_str()?.to_owned(), row["status"].as_str()?)))
        .collect()
}

fn currently_running_test<'a>(report: &Value, test_names: &'a [String]) -> Option<&'a str> {
    if report["overall_status"].as_str() != Some("running") {
        return None;
    }
    let completed = delivery_test_statuses(report);
    test_names
        .iter()
        .find(|name| !completed.contains_key(*name))
        .map(String::as_str)
}

fn delivery_progress(delivery: &DeliveryData, test_names: &[String]) -> Value {
    let test_statuses = delivery_test_statuses(&delivery.report);
    let running_test = currently_running_test(&delivery.report, test_names);
    let mut pending = 0;
    let mut running = 0;
    let mut failed = 0;
    let mut completed = 0;
    for test_name in test_names {
        let status = test_statuses
            .get(test_name)
            .copied()
            .or_else(|| (running_test == Some(test_name.as_str())).then_some("running"))
            .unwrap_or("not_run");
        match status {
            "not_run" | "pending" | "queued" => pending += 1,
            "running" => running += 1,
            "passed" => completed += 1,
            _ => failed += 1,
        }
    }
    let status = if delivery.status == "pending" {
        "pending"
    } else if delivery.status == "running" {
        "running"
    } else if failed > 0 || delivery.report["overall_status"] == "completed_with_errors" {
        "failed"
    } else {
        "completed"
    };
    json!({
        "delivery_id": delivery.delivery_id,
        "filename": delivery.filename,
        "status": status,
        "started_at": delivery.report["started_at"],
        "finished_at": delivery.report["finished_at"],
        "tests_pending": pending,
        "tests_running": running,
        "tests_failed": failed,
        "tests_completed": completed,
        "tests_total": test_names.len(),
    })
}

fn combined_check_names(
    artifact_root: &std::path::Path,
    run_id: &str,
    deliveries: &[DeliveryData],
) -> Vec<String> {
    let config_path = artifact_root
        .join("project-snapshots")
        .join(run_id)
        .join("project/configuration.yaml");
    let mut names: Vec<String> = std::fs::read(config_path)
        .ok()
        .and_then(|bytes| project_config::parse_configuration(&bytes).ok())
        .map(|project| project.checks.into_iter().map(|check| check.name).collect())
        .unwrap_or_default();
    let mut known = names.iter().cloned().collect::<HashSet<_>>();
    for delivery in deliveries {
        for row in delivery.report["checks"].as_array().into_iter().flatten() {
            if let Some(name) = row["name"].as_str()
                && known.insert(name.to_owned())
            {
                names.push(name.to_owned());
            }
        }
    }
    names
}

#[derive(Default)]
struct TestCounts {
    pending: usize,
    running: usize,
    finished: usize,
    finished_without_errors: usize,
    finished_with_errors: usize,
}

fn test_status_label(status: &str) -> &'static str {
    match status {
        "passed" => "pass",
        "not_run" | "pending" | "queued" => "pending",
        "running" => "running",
        _ => "error",
    }
}

fn check_status_label(check: &Value) -> &'static str {
    if check["result"].as_i64() == Some(1) {
        "pass"
    } else {
        "error"
    }
}

fn is_error_status(status: &str) -> bool {
    status != "passed"
}

fn parse_time(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|time| time.with_timezone(&Utc))
}

fn choose_earlier(target: &mut Option<String>, candidate: &str) {
    if target
        .as_deref()
        .and_then(parse_time)
        .zip(parse_time(candidate))
        .is_none_or(|(current, next)| next < current)
    {
        *target = Some(candidate.to_owned());
    }
}

fn choose_later(target: &mut Option<String>, candidate: &str) {
    if target
        .as_deref()
        .and_then(parse_time)
        .zip(parse_time(candidate))
        .is_none_or(|(current, next)| next > current)
    {
        *target = Some(candidate.to_owned());
    }
}
