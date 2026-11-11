//! Purpose: Runs one delivery's configured tests and persists its expedient and cleanup result.
use chrono::Utc;
use serde_json::{Value, json};
use std::{collections::HashSet, fs, path::Path, time::Instant};

use crate::{
    test_executor::execute_project_tests,
    test_result_store,
    vm_activity::{self, DeliveryInput, ProjectSnapshot, VmActivityRuntime},
};

/// Keep the grade formula definitions with each saved expedient without copying test secrets.
pub fn grading_plan(configuration: &thot_core::VMActivityProject) -> Value {
    let weights = |parameters: &thot_core::ParameterInput| {
        parameters
            .into_strings()
            .ok()
            .and_then(|parameters| parameters.get("weights").cloned())
            .map(|encoded| {
                encoded
                    .split(',')
                    .filter_map(|value| value.parse::<f64>().ok())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    let checks = |parameters: &thot_core::ParameterInput| {
        let values = parameters.into_strings().unwrap_or_default();
        let mut visible = serde_json::Map::new();
        for (key, value) in values {
            let lowered = key.to_ascii_lowercase();
            if lowered == "pass"
                || ["password", "passwd", "secret", "token", "credential"]
                    .iter()
                    .any(|needle| lowered.contains(needle))
            {
                continue;
            }
            visible.insert(key, json!(value));
        }
        Value::Object(visible)
    };
    json!({
        "tests": configuration.tests.iter().map(|test| json!({
            "name": test.name,
            "test": test.test_ref,
            "description": test.description,
            "repeat": test.repeat,
        })).collect::<Vec<_>>(),
        "checks": configuration.checks.iter().map(|check| json!({
            "name": check.name,
            "description": check.description,
            "check": check.check_ref,
            "test": check.test,
            "property": check.property,
            "parameters": checks(&check.parameters),
        })).collect::<Vec<_>>(),
        "rubrics": configuration.rubrics.iter().map(|rubric| json!({
            "name": rubric.name,
            "description": rubric.description,
            "calculation": rubric.calculation,
            "checks": rubric.checks,
            "weights": weights(&rubric.parameters),
        })).collect::<Vec<_>>(),
        "evaluation": configuration.evaluation.iter().map(|item| json!({
            "name": item.name,
            "description": item.description,
            "calculation": item.calculation,
            "rubrics": item.rubrics,
            "weights": weights(&item.parameters),
        })).collect::<Vec<_>>(),
        "rubrics_multiplier": configuration.rubrics_multiplier,
        "evaluation_multiplier": configuration.evaluation_multiplier,
        "not_delivered_mark": configuration.not_delivered_mark,
    })
}

pub async fn execute_delivery(
    runtime: &VmActivityRuntime,
    project: &ProjectSnapshot,
    virtualbox_version: &str,
    run_id: &str,
    expedient_id: &str,
    input: &DeliveryInput,
    skipped_test_names: &HashSet<String>,
    report_dir: &Path,
    report_path: &Path,
) -> Value {
    let started = Instant::now();
    let now = Utc::now().to_rfc3339();
    let grading_plan = grading_plan(&project.configuration);
    let mut report = json!({
        "schema_version": 2,
        "project_id": project.project_id,
        "activity_name": project.configuration.activity_name,
        "delivery_id": input.delivery_id,
        "expedient_id": expedient_id,
        "run_id": run_id,
        "execution_context": {
            "project_archive": project.archive_path.file_name().and_then(|n| n.to_str()),
            "project_config_sha256": project.sha256,
            "engine_version": env!("CARGO_PKG_VERSION"),
            "host_os": std::env::consts::OS,
            "virtualbox_version": virtualbox_version
        },
        "source": {
            "filename": input.filename,
            "size_bytes": input.size,
            "sha256": input.sha256,
            "source_ref": input.source,
            "validation_error": input.validation_error
        },
        "test_results": [],
        "grading_plan": grading_plan,
        "checks": [],
        "rubrics": [],
        "evaluation": {
            "status": "not_evaluated",
            "results": [],
            "rubrics_multiplier": project.configuration.rubrics_multiplier,
            "evaluation_multiplier": project.configuration.evaluation_multiplier,
            "warnings": crate::project_config::configuration_warnings(
                project.configuration.rubrics_multiplier,
                project.configuration.evaluation_multiplier
            )
        },
        "cleanup": {"status":"not_run", "details":[]},
        "overall_status": "running",
        "started_at": now,
        "finished_at": null
    });
    if let Err(error) = test_result_store::persist_report(report_path, &report) {
        report["setup_error"] = json!(format!("could not persist initial expedient: {error}"));
        return report;
    }

    let project_config = project.configuration.clone();
    if let Err(error) = execute_project_tests(
        &project_config,
        project,
        runtime,
        input,
        skipped_test_names,
        run_id,
        expedient_id,
        report_dir,
        report_path,
        &mut report,
    )
    .await
    {
        report["orchestration_error"] = json!(error);
    }

    // Cleanup is independent of configured test results. If import never created a manifest,
    // there is no VM resource owned by this delivery to delete.
    let manifest_path = report_dir.join("cleanup-manifest.json");
    let cleanup = if manifest_path.is_file() {
        vm_activity::cleanup_from_manifest(runtime, report_dir, &input.delivery_id).await
    } else {
        json!({"status":"passed", "vm_unregistered":true, "run_storage_removed":true, "details":["no VirtualBox import manifest was created"]})
    };
    report["cleanup"] = cleanup;
    report["finished_at"] = json!(Utc::now().to_rfc3339());
    report["duration_ms"] = json!(started.elapsed().as_millis());
    let failed = report["test_results"].as_array().is_some_and(|rows| {
        rows.iter()
            .any(|row| !matches!(row["status"].as_str(), Some("passed" | "skipped")))
    }) || report.get("orchestration_error").is_some()
        || report["cleanup"]["status"] != "passed";
    report["overall_status"] = json!(if failed {
        "completed_with_errors"
    } else {
        "completed"
    });
    if let Err(error) = test_result_store::persist_report(report_path, &report) {
        report["persistence_error"] = json!(error.to_string());
    }
    let _ = fs::create_dir_all(report_dir);
    report
}
