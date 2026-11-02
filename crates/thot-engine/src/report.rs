//! Purpose: Builds compact run and delivery summaries from saved run and expedient data.
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sqlx::SqlitePool;
use std::collections::{BTreeMap, HashSet};

use crate::{delivery_run_service::ServiceError, project_config};

struct DeliveryData {
    delivery_id: String,
    filename: String,
    student_count: i64,
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
        let active = matches!(status.as_str(), "queued" | "running" | "cancelling");
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
        result.push(json!({
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
        }));
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

async fn load_deliveries(
    pool: &SqlitePool,
    run_id: &str,
) -> Result<Vec<DeliveryData>, ServiceError> {
    let rows = sqlx::query_as::<_, (String, String, String, i64)>(
        "SELECT e.delivery_id, MIN(e.report_path), d.original_filename, COUNT(*) FROM vm_activity_expedients e JOIN vm_activity_deliveries d ON d.delivery_id=e.delivery_id WHERE e.run_id=? GROUP BY e.delivery_id, d.original_filename ORDER BY d.original_filename COLLATE NOCASE, e.delivery_id",
    )
    .bind(run_id)
    .fetch_all(pool)
    .await?;
    let mut deliveries = Vec::with_capacity(rows.len());
    for (delivery_id, report_path, filename, student_count) in rows {
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
