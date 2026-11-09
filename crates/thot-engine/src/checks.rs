//! Purpose: Evaluates configured internal checks against the test results in an expedient.
use crate::stats;
use chrono::{DateTime, NaiveDate, Utc};
use regex::Regex;
use serde_json::{Value, json};
use serde_json_path::JsonPath;
use std::{
    cmp::Ordering,
    collections::{BTreeMap, HashMap, HashSet},
    time::Instant,
};
use thot_core::{ProjectCheck, ProjectTest};

/// Evaluate every configured check and return the report rows to store in an expedient.
pub fn evaluate_checks(
    checks: &[ProjectCheck],
    tests: &[ProjectTest],
    report: &Value,
    reports_for_run: &[Value],
) -> Vec<Value> {
    checks
        .iter()
        .map(|check| {
            let started = Instant::now();
            let result = match find_test(check, tests) {
                Some(test) if stats::is_stats_check(&check.check_ref) => {
                    stats::evaluate_check(check, test, report, reports_for_run)
                        .map(|result| (result.passed, Some(result)))
                }
                Some(test) => run_check(check, test, report).map(|passed| (passed, None)),
                None => Err(format!("configured test {:?} was not found", check.test)),
            };
            let (value, error, details) = match result {
                Ok((value, details)) => (value, Value::Null, details),
                Err(error) => (0, json!(error), None),
            };
            let mut row = json!({
                "name": check.name,
                "description": check.description,
                "result": value,
                "err": error,
                "date_time": Utc::now().to_rfc3339(),
                "duration_ms": started.elapsed().as_millis(),
            });
            if let Some(details) = details {
                row["statistic"] = details.statistic;
                row["sample_count"] = json!(details.sample_count);
                row["student_value"] = details.student_value;
            }
            row
        })
        .collect()
}

/// Add configured aggregate checks after ordinary and CSV-imported check results exist.
pub fn evaluate_aggregate_checks(checks: &[ProjectCheck], check_rows: &[Value]) -> Vec<Value> {
    let definitions = checks
        .iter()
        .map(|check| (check.name.as_str(), check))
        .collect::<HashMap<_, _>>();
    let mut rows = check_rows.to_vec();
    let mut computed = HashMap::<String, Result<u8, String>>::new();
    for row in check_rows {
        if let Some(name) = row["name"].as_str() {
            let result = row["result"]
                .as_u64()
                .filter(|value| *value <= 1)
                .map(|value| value as u8)
                .ok_or_else(|| format!("check result {name:?} is not 0 or 1"));
            computed.insert(name.to_owned(), result);
        }
    }

    for check in checks
        .iter()
        .filter(|check| check.check_ref == "aggr/checks")
    {
        if rows
            .iter()
            .any(|row| row["name"].as_str() == Some(check.name.as_str()))
        {
            continue;
        }
        let started = Instant::now();
        let mut visiting = HashSet::new();
        let result = aggregate_check_result(
            &check.name,
            &definitions,
            &rows,
            &mut computed,
            &mut visiting,
        );
        let (value, error) = match result {
            Ok(value) => (value, Value::Null),
            Err(error) => (0, json!(error)),
        };
        rows.push(json!({
            "name": check.name,
            "description": check.description,
            "result": value,
            "err": error,
            "date_time": Utc::now().to_rfc3339(),
            "duration_ms": started.elapsed().as_millis(),
        }));
    }
    rows
}

fn aggregate_check_result(
    name: &str,
    definitions: &HashMap<&str, &ProjectCheck>,
    rows: &[Value],
    computed: &mut HashMap<String, Result<u8, String>>,
    visiting: &mut HashSet<String>,
) -> Result<u8, String> {
    if let Some(result) = computed.get(name) {
        return result.clone();
    }
    if !visiting.insert(name.to_owned()) {
        return Err(format!(
            "aggregate check dependency cycle found at {name:?}"
        ));
    }
    let result = (|| {
        let check = definitions
            .get(name)
            .copied()
            .ok_or_else(|| format!("configured check {name:?} was not found"))?;
        if check.check_ref != "aggr/checks" {
            return Err(format!("check result {name:?} is missing"));
        }
        let parameters = check.parameters.into_strings()?;
        let names = parameters
            .get("checks")
            .map(|value| value.split(',').map(str::trim).collect::<Vec<_>>())
            .unwrap_or_default();
        if names.is_empty() || names.iter().any(|dependency| dependency.is_empty()) {
            return Err("aggregate check requires a non-empty checks list".into());
        }
        let mut passed_count = 0usize;
        for dependency in &names {
            let value = if let Some(row) = rows
                .iter()
                .find(|row| row["name"].as_str() == Some(dependency))
            {
                row["result"]
                    .as_u64()
                    .filter(|value| *value <= 1)
                    .ok_or_else(|| format!("check result {dependency:?} is not 0 or 1"))?
                    as u8
            } else {
                aggregate_check_result(dependency, definitions, rows, computed, visiting)?
            };
            passed_count += usize::from(value == 1);
        }

        let condition = parameters
            .get("condition")
            .map(String::as_str)
            .ok_or("aggregate check is missing parameter condition")?;
        let passed = match condition {
            "all" => passed_count == names.len(),
            "any" => passed_count > 0,
            "one" => passed_count == 1,
            "none" => passed_count == 0,
            "majority" => passed_count * 2 > names.len(),
            ">=" | "<=" | ">" | "<" | "==" | "!=" => {
                let expected = parameters
                    .get("value")
                    .ok_or_else(|| format!("condition {condition:?} requires value"))?
                    .parse::<usize>()
                    .map_err(|_| "aggregate check value must be a non-negative integer")?;
                match condition {
                    ">=" => passed_count >= expected,
                    "<=" => passed_count <= expected,
                    ">" => passed_count > expected,
                    "<" => passed_count < expected,
                    "==" => passed_count == expected,
                    "!=" => passed_count != expected,
                    _ => unreachable!(),
                }
            }
            _ => return Err(format!("unknown aggregate check condition {condition:?}")),
        };
        Ok(u8::from(passed))
    })();
    visiting.remove(name);
    computed.insert(name.to_owned(), result.clone());
    result
}

fn find_test<'a>(check: &ProjectCheck, tests: &'a [ProjectTest]) -> Option<&'a ProjectTest> {
    tests.iter().find(|test| {
        test.name == check.test
            || (stats::is_stats_check(&check.check_ref)
                && expected_test_ids(test).iter().any(|id| id == &check.test))
    })
}

fn run_check(check: &ProjectCheck, test: &ProjectTest, report: &Value) -> Result<u8, String> {
    let parameters = check.parameters.into_strings()?;
    match check.check_ref.as_str() {
        "pass" => passed_test_result(test, report),
        "regex_match" => regex_match_check(check, test, report, &parameters),
        "regex_extract" => regex_extract_check(check, test, report, &parameters),
        "yaml_match" => yaml_match_check(check, test, report, &parameters),
        "yaml_extract" => yaml_extract_check(check, test, report, &parameters),
        unknown => Err(format!("unknown internal check {unknown:?}")),
    }
}

fn regex_match_check(
    check: &ProjectCheck,
    test: &ProjectTest,
    report: &Value,
    parameters: &BTreeMap<String, String>,
) -> Result<u8, String> {
    let property = check
        .property
        .as_deref()
        .ok_or("regex_match check has no property")?;
    let regex_text = required_parameter(parameters, "regex")?;
    let regex = Regex::new(regex_text).map_err(|error| format!("invalid regex: {error}"))?;
    let results = report["test_results"]
        .as_array()
        .ok_or("expedient has no test_results array")?;

    for test_id in expected_test_ids(test) {
        let test_result = results
            .iter()
            .find(|row| row["test_id"].as_str() == Some(&test_id))
            .ok_or_else(|| format!("test result {test_id:?} is missing"))?;
        let Some(value) = property_value(test_result, property) else {
            return Err(format!("test result property {property:?} does not exist"));
        };
        let Some(value) = property_text(value) else {
            return Err(format!(
                "test result property {property:?} is not a scalar value"
            ));
        };
        if !regex.is_match(&value) {
            return Ok(0);
        }
    }
    Ok(1)
}

fn yaml_extract_check(
    check: &ProjectCheck,
    test: &ProjectTest,
    report: &Value,
    parameters: &BTreeMap<String, String>,
) -> Result<u8, String> {
    let property = check
        .property
        .as_deref()
        .ok_or("yaml_extract check has no property")?;
    let expression = parameters
        .get("JSONPath")
        .or_else(|| parameters.get("jsonpath"))
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .ok_or("yaml_extract check is missing parameter \"JSONPath\"")?;
    let path = JsonPath::parse(expression)
        .map_err(|error| format!("invalid yaml_extract JSONPath: {error}"))?;
    let condition = required_parameter(parameters, "condition")?;
    let data_type = parameters.get("data_type").map(String::as_str);
    let expected = parameters.get("value").map(String::as_str);

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
        let document = parse_document(value, property)?;
        let selected = path.query(&document).exactly_one().map_err(|error| {
            format!("yaml_extract JSONPath must select exactly one value: {error}")
        })?;
        let Some(captured) = property_text(selected) else {
            return Err(format!(
                "yaml_extract JSONPath {expression:?} selected a non-scalar value"
            ));
        };
        if !evaluate_condition(&Value::String(captured), expected, data_type, condition)? {
            return Ok(0);
        }
    }
    Ok(1)
}

fn yaml_match_check(
    check: &ProjectCheck,
    test: &ProjectTest,
    report: &Value,
    parameters: &BTreeMap<String, String>,
) -> Result<u8, String> {
    let property = check
        .property
        .as_deref()
        .ok_or("yaml_match check has no property")?;
    let expression = parameters
        .get("JSONPath")
        .or_else(|| parameters.get("jsonpath"))
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or("yaml_match check is missing parameter \"JSONPath\"")?;
    let path = JsonPath::parse(expression)
        .map_err(|error| format!("invalid yaml_match JSONPath: {error}"))?;
    let results = report["test_results"]
        .as_array()
        .ok_or("expedient has no test_results array")?;

    for test_id in expected_test_ids(test) {
        let test_result = results
            .iter()
            .find(|row| row["test_id"].as_str() == Some(&test_id))
            .ok_or_else(|| format!("test result {test_id:?} is missing"))?;
        let Some(value) = property_value(test_result, property) else {
            return Err(format!("test result property {property:?} does not exist"));
        };
        let document = parse_document(value, property)?;
        if path.query(&document).all().is_empty() {
            return Ok(0);
        }
    }
    Ok(1)
}

pub(crate) fn parse_document(value: &Value, property: &str) -> Result<Value, String> {
    match value {
        Value::String(document) => match serde_json::from_str(document) {
            Ok(json) => Ok(json),
            Err(json_error) => {
                let yaml = serde_yaml::from_str::<serde_yaml::Value>(document).map_err(|yaml_error| {
                    format!(
                        "test result property {property:?} is neither valid JSON nor YAML (JSON: {json_error}; YAML: {yaml_error})"
                    )
                })?;
                serde_json::to_value(yaml).map_err(|error| {
                    format!("cannot convert YAML property {property:?} to JSON: {error}")
                })
            }
        },
        structured => Ok(structured.clone()),
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
    let condition = required_parameter(parameters, "condition")?;
    let data_type = parameters.get("data_type").map(String::as_str);
    let expected = parameters.get("value").map(String::as_str);
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
        if !evaluate_condition(
            &Value::String(captured.as_str().to_owned()),
            expected,
            data_type,
            condition,
        )? {
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
        .ok_or_else(|| format!("check is missing parameter {key:?}"))
}

pub(crate) fn expected_test_ids(test: &ProjectTest) -> Vec<String> {
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

pub(crate) fn property_value<'a>(root: &'a Value, property: &str) -> Option<&'a Value> {
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

pub(crate) fn property_text(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Null | Value::Array(_) | Value::Object(_) => None,
    }
}

pub(crate) fn compare_captured(
    captured: &str,
    expected: &str,
    data_type: &str,
    condition: &str,
) -> Result<bool, String> {
    evaluate_condition(
        &Value::String(captured.to_owned()),
        Some(expected),
        Some(data_type),
        condition,
    )
}

/// Apply a comparison or a predicate condition to a value from a test result.
pub(crate) fn evaluate_condition(
    candidate: &Value,
    expected: Option<&str>,
    data_type: Option<&str>,
    condition: &str,
) -> Result<bool, String> {
    if let Some(expression) = condition_expression(condition, "r'") {
        let regex =
            Regex::new(expression).map_err(|error| format!("invalid condition regex: {error}"))?;
        let candidate = property_text(candidate).unwrap_or_else(|| {
            serde_yaml::to_string(candidate).unwrap_or_else(|_| candidate.to_string())
        });
        return Ok(regex.is_match(&candidate));
    }
    if let Some(expression) = condition_expression(condition, "json'") {
        let document = parse_document(candidate, "condition candidate")?;
        let path = JsonPath::parse(expression)
            .map_err(|error| format!("invalid condition JSONPath: {error}"))?;
        return Ok(!path.query(&document).all().is_empty());
    }
    let expected =
        expected.ok_or_else(|| format!("condition {condition:?} requires a comparison value"))?;
    let captured =
        property_text(candidate).ok_or("comparison condition requires a scalar candidate value")?;
    let inferred_type;
    let data_type = match data_type {
        Some(data_type) => data_type,
        None if condition == "contains" => "text",
        None => {
            inferred_type = infer_data_type(&captured, expected);
            inferred_type
        }
    };
    compare_scalar(&captured, expected, data_type, condition)
}

fn infer_data_type(candidate: &str, expected: &str) -> &'static str {
    if candidate
        .parse::<f64>()
        .is_ok_and(|value| value.is_finite())
        && expected.parse::<f64>().is_ok_and(|value| value.is_finite())
    {
        "number"
    } else if NaiveDate::parse_from_str(candidate, "%Y-%m-%d").is_ok()
        && NaiveDate::parse_from_str(expected, "%Y-%m-%d").is_ok()
    {
        "date"
    } else if DateTime::parse_from_rfc3339(candidate).is_ok()
        && DateTime::parse_from_rfc3339(expected).is_ok()
    {
        "datetime"
    } else {
        "text"
    }
}

pub(crate) fn validate_condition_syntax(
    condition: &str,
    data_type: Option<&str>,
) -> Result<(), String> {
    if condition_expression(condition, "r'").is_some() {
        let expression = condition_expression(condition, "r'").unwrap_or_default();
        Regex::new(expression).map_err(|error| format!("invalid condition regex: {error}"))?;
        return Ok(());
    }
    if let Some(expression) = condition_expression(condition, "json'") {
        JsonPath::parse(expression)
            .map_err(|error| format!("invalid condition JSONPath: {error}"))?;
        return Ok(());
    }
    if matches!(condition, "<" | "<=" | ">" | ">=" | "==" | "!=") {
        return Ok(());
    }
    if condition == "contains" && data_type == Some("text") {
        return Ok(());
    }
    Err("condition must be <, <=, >, >=, ==, !=, contains for text, r'<regex>', or json'<JSONPath>'".into())
}

fn condition_expression<'a>(condition: &'a str, prefix: &str) -> Option<&'a str> {
    condition
        .strip_prefix(prefix)
        .and_then(|expression| expression.strip_suffix('\''))
}

fn compare_scalar(
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
        other => return Err(format!("unsupported check data_type {other:?}")),
    };
    match condition {
        "<" => Ok(ordering == Ordering::Less),
        "<=" => Ok(ordering != Ordering::Greater),
        ">" => Ok(ordering == Ordering::Greater),
        ">=" => Ok(ordering != Ordering::Less),
        "==" => Ok(ordering == Ordering::Equal),
        "!=" => Ok(ordering != Ordering::Equal),
        "contains" => Err("condition contains is only supported for text".into()),
        other => Err(format!("unsupported check condition {other:?}")),
    }
}

pub(crate) fn parse_finite_number(value: &str, label: &str) -> Result<f64, String> {
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
