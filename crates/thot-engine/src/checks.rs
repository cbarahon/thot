//! Purpose: Evaluates configured internal checks against the test results in an expedient.
use chrono::Utc;
use serde_json::{Value, json};
use thot_core::{ProjectCheck, ProjectTest};

/// Evaluate every configured check and return the report rows to store in an expedient.
pub fn evaluate_checks(
    checks: &[ProjectCheck],
    tests: &[ProjectTest],
    report: &Value,
) -> Vec<Value> {
    checks
        .iter()
        .map(|check| {
            let result = match tests.iter().find(|test| test.name == check.test) {
                Some(test) => run_check(check, test, report),
                None => Err(format!("configured test {:?} was not found", check.test)),
            };
            let (value, error) = match result {
                Ok(value) => (value, Value::Null),
                Err(error) => (0, json!(error)),
            };
            json!({
                "name": check.name,
                "description": check.description,
                "result": value,
                "err": error,
                "date_time": Utc::now().to_rfc3339()
            })
        })
        .collect()
}

fn run_check(check: &ProjectCheck, test: &ProjectTest, report: &Value) -> Result<u8, String> {
    check.parameters.into_strings()?;
    match check.check_ref.as_str() {
        "pass" => passed_test_result(test, report),
        unknown => Err(format!("unknown internal check {unknown:?}")),
    }
}

fn passed_test_result(test: &ProjectTest, report: &Value) -> Result<u8, String> {
    let results = report["test_results"]
        .as_array()
        .ok_or("expedient has no test_results array")?;
    let expected = (0..test.repeat)
        .map(|index| {
            if test.repeat == 1 {
                test.name.clone()
            } else {
                format!("{}_{:02}", test.name, index + 1)
            }
        })
        .collect::<Vec<_>>();

    let mut found = 0;
    let all_passed = expected.iter().all(|test_id| {
        match results
            .iter()
            .find(|row| row["test_id"].as_str() == Some(test_id))
        {
            Some(row) => {
                found += 1;
                row["status"].as_str() == Some("passed")
            }
            None => false,
        }
    });
    if found != expected.len() {
        return Err(format!(
            "test {:?} is missing one or more result records",
            test.name
        ));
    }
    Ok(u8::from(all_passed))
}
