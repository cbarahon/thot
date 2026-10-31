//! Purpose: Evaluates configured internal checks against the test results in an expedient.
use chrono::{DateTime, NaiveDate, Utc};
use regex::Regex;
use serde_json::{Value, json};
use std::{cmp::Ordering, collections::BTreeMap};
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
    let parameters = check.parameters.into_strings()?;
    match check.check_ref.as_str() {
        "pass" => passed_test_result(test, report),
        "regex_extract" => regex_extract_check(check, test, report, &parameters),
        unknown => Err(format!("unknown internal check {unknown:?}")),
    }
}

fn regex_extract_check(
    check: &ProjectCheck,
    test: &ProjectTest,
    report: &Value,
    parameters: &BTreeMap<String, String>,
) -> Result<u8, String> {
    let property = check
        .property
        .as_deref()
        .ok_or("regex_extract check has no property")?;
    let regex_text = required_parameter(parameters, "regex")?;
    let data_type = required_parameter(parameters, "data_type")?;
    let condition = required_parameter(parameters, "condition")?;
    let expected = parameters
        .get("value")
        .map(String::as_str)
        .ok_or("regex_extract check is missing parameter \"value\"")?;
    let regex = Regex::new(regex_text).map_err(|error| format!("invalid regex: {error}"))?;
    if regex.captures_len() < 2 {
        return Err("regex_extract expression must contain a capture group".into());
    }

    let results = report["test_results"]
        .as_array()
        .ok_or("expedient has no test_results array")?;
    let expected_ids = expected_test_ids(test);
    let mut found_results = Vec::with_capacity(expected_ids.len());
    for test_id in expected_ids {
        let row = results
            .iter()
            .find(|row| row["test_id"].as_str() == Some(&test_id))
            .ok_or_else(|| format!("test result {test_id:?} is missing"))?;
        found_results.push(row);
    }

    for test_result in found_results {
        let Some(value) = property_value(test_result, property) else {
            return Err(format!("test result property {property:?} does not exist"));
        };
        let Some(value) = property_text(value) else {
            return Err(format!(
                "test result property {property:?} is not a scalar value"
            ));
        };
        let Some(captures) = regex.captures(&value) else {
            return Ok(0);
        };
        let Some(captured) = captures.get(1) else {
            return Err("regex_extract expression did not produce capture group 1".into());
        };
        if !compare_captured(captured.as_str(), expected, data_type, condition)? {
            return Ok(0);
        }
    }
    Ok(1)
}

fn required_parameter<'a>(
    parameters: &'a BTreeMap<String, String>,
    key: &str,
) -> Result<&'a str, String> {
    parameters
        .get(key)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("regex_extract check is missing parameter {key:?}"))
}

fn expected_test_ids(test: &ProjectTest) -> Vec<String> {
    (0..test.repeat)
        .map(|index| {
            if test.repeat == 1 {
                test.name.clone()
            } else {
                format!("{}_{:02}", test.name, index + 1)
            }
        })
        .collect()
}

fn property_value<'a>(root: &'a Value, property: &str) -> Option<&'a Value> {
    let mut value = root;
    for component in property.split('.') {
        value = if let Some(object) = value.as_object() {
            object.get(component)?
        } else if let (Some(array), Ok(index)) = (value.as_array(), component.parse::<usize>()) {
            array.get(index)?
        } else {
            return None;
        };
    }
    Some(value)
}

fn property_text(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Null | Value::Array(_) | Value::Object(_) => None,
    }
}

fn compare_captured(
    captured: &str,
    expected: &str,
    data_type: &str,
    condition: &str,
) -> Result<bool, String> {
    if data_type == "text" && condition == "contains" {
        return Ok(captured.contains(expected));
    }
    let ordering = match data_type {
        "number" => {
            let captured = parse_finite_number(captured, "captured regex value")?;
            let expected = parse_finite_number(expected, "check value")?;
            captured
                .partial_cmp(&expected)
                .ok_or_else(|| "numeric values cannot be compared".to_owned())?
        }
        "text" => captured.cmp(expected),
        "date" => {
            let captured = NaiveDate::parse_from_str(captured, "%Y-%m-%d")
                .map_err(|error| format!("captured value is not a YYYY-MM-DD date: {error}"))?;
            let expected = NaiveDate::parse_from_str(expected, "%Y-%m-%d")
                .map_err(|error| format!("check value is not a YYYY-MM-DD date: {error}"))?;
            captured.cmp(&expected)
        }
        "datetime" => {
            let captured = DateTime::parse_from_rfc3339(captured)
                .map_err(|error| format!("captured value is not an RFC 3339 datetime: {error}"))?;
            let expected = DateTime::parse_from_rfc3339(expected)
                .map_err(|error| format!("check value is not an RFC 3339 datetime: {error}"))?;
            captured.cmp(&expected)
        }
        other => return Err(format!("unsupported regex_extract data_type {other:?}")),
    };
    match condition {
        "<" => Ok(ordering == Ordering::Less),
        "<=" => Ok(ordering != Ordering::Greater),
        ">" => Ok(ordering == Ordering::Greater),
        ">=" => Ok(ordering != Ordering::Less),
        "==" => Ok(ordering == Ordering::Equal),
        "!=" => Ok(ordering != Ordering::Equal),
        "contains" => Err("condition contains is only supported for text".into()),
        other => Err(format!("unsupported regex_extract condition {other:?}")),
    }
}

fn parse_finite_number(value: &str, label: &str) -> Result<f64, String> {
    let number = value
        .parse::<f64>()
        .map_err(|error| format!("{label} is not a number: {error}"))?;
    if !number.is_finite() {
        return Err(format!("{label} must be a finite number"));
    }
    Ok(number)
}

fn passed_test_result(test: &ProjectTest, report: &Value) -> Result<u8, String> {
    let results = report["test_results"]
        .as_array()
        .ok_or("expedient has no test_results array")?;
    let expected = expected_test_ids(test);
    let mut all_passed = true;
    for test_id in expected {
        let row = results
            .iter()
            .find(|row| row["test_id"].as_str() == Some(&test_id))
            .ok_or_else(|| format!("test result {test_id:?} is missing"))?;
        all_passed &= row["status"].as_str() == Some("passed");
    }
    Ok(u8::from(all_passed))
}
