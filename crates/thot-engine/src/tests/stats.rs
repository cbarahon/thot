//! Purpose: Computes YAML summaries from result values of earlier tests in the same delivery.
use regex::Regex;
use serde_json::{Value, json};
use serde_json_path::JsonPath;
use std::collections::BTreeMap;
use thot_core::test_results::TestStatus;

use crate::test_execution::{TestInvocation, TestOutcome};

pub async fn run(invocation: &TestInvocation) -> TestOutcome {
    let parameters = &invocation.parameters;
    let selected_tests = match parameters.get("tests") {
        Some(value) => value
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .collect::<Vec<_>>(),
        None => return TestOutcome::error("stats test requires parameter tests"),
    };
    if selected_tests.is_empty() {
        return TestOutcome::error("stats test parameter tests must list at least one test");
    }
    let property = match required_parameter(parameters, "property") {
        Ok(value) => value,
        Err(error) => return TestOutcome::error(error),
    };
    let data_type = parameters
        .get("data_type")
        .map(String::as_str)
        .unwrap_or("number");
    if !matches!(data_type, "number" | "text" | "date" | "datetime") {
        return TestOutcome::error("stats test data_type must be number, text, date, or datetime");
    }

    let expression = match invocation.test_reference.as_str() {
        "stats/regex_extract" => match required_parameter(parameters, "regex") {
            Ok(value) => Some(value),
            Err(error) => return TestOutcome::error(error),
        },
        "stats/yaml_extract" => match parameters
            .get("JSONPath")
            .or_else(|| parameters.get("jsonpath"))
            .map(String::as_str)
            .filter(|value| !value.is_empty())
        {
            Some(value) => Some(value),
            None => return TestOutcome::error("stats/yaml_extract requires parameter JSONPath"),
        },
        unknown => return TestOutcome::error(format!("unsupported stats test {unknown:?}")),
    };
    if invocation.test_reference == "stats/regex_extract" {
        match Regex::new(expression.unwrap_or_default()) {
            Ok(regex) if regex.captures_len() >= 2 => {}
            Ok(_) => return TestOutcome::error("stats/regex_extract regex needs a capture group"),
            Err(error) => return TestOutcome::error(format!("invalid regex: {error}")),
        }
    } else if let Err(error) = JsonPath::parse(expression.unwrap_or_default()) {
        return TestOutcome::error(format!("invalid JSONPath: {error}"));
    }

    let mut values = Vec::new();
    let mut ignored_count = 0usize;
    for result in &invocation.prior_test_results {
        let test_id = result["test_id"].as_str().unwrap_or_default();
        if !selected_tests
            .iter()
            .any(|selector| selects_test_result(selector, test_id))
        {
            continue;
        }
        match extract_value(&invocation.test_reference, result, property, parameters) {
            Ok(value) => values.push(value),
            Err(_) => ignored_count += 1,
        }
    }
    if values.is_empty() {
        return TestOutcome::error(format!(
            "no selected test results contain an extractable value for property {property:?}"
        ));
    }
    let mut summary = match crate::stats::summarize_values(&values, data_type) {
        Ok(summary) => summary,
        Err(error) => return TestOutcome::error(error),
    };
    summary["ignored_count"] = json!(ignored_count);
    let stdout = match serde_yaml::to_string(&summary) {
        Ok(stdout) => stdout,
        Err(error) => {
            return TestOutcome::error(format!("cannot serialize stats as YAML: {error}"));
        }
    };
    TestOutcome {
        status: TestStatus::Passed,
        stdout,
        stderr: String::new(),
        exit_code: Some(0),
        diagnostics: String::new(),
        observation: summary,
        evidence: Vec::new(),
    }
}

fn extract_value(
    test_reference: &str,
    result: &Value,
    property: &str,
    parameters: &BTreeMap<String, String>,
) -> Result<String, String> {
    let property_value = crate::checks::property_value(result, property)
        .ok_or_else(|| format!("test result property {property:?} does not exist"))?;
    if test_reference == "stats/regex_extract" {
        let expression = parameters
            .get("regex")
            .map(String::as_str)
            .ok_or("stats/regex_extract requires parameter regex")?;
        let regex = Regex::new(expression).map_err(|error| format!("invalid regex: {error}"))?;
        let text = crate::checks::property_text(property_value)
            .ok_or_else(|| format!("property {property:?} is not a scalar value"))?;
        return regex
            .captures(&text)
            .and_then(|captures| captures.get(1))
            .map(|capture| capture.as_str().to_owned())
            .ok_or_else(|| {
                "regex did not match a capture group in the selected test result".into()
            });
    }

    let expression = parameters
        .get("JSONPath")
        .or_else(|| parameters.get("jsonpath"))
        .map(String::as_str)
        .ok_or("stats/yaml_extract requires parameter JSONPath")?;
    let path = JsonPath::parse(expression).map_err(|error| format!("invalid JSONPath: {error}"))?;
    let document = crate::checks::parse_document(property_value, property)?;
    let selected = path
        .query(&document)
        .exactly_one()
        .map_err(|error| format!("JSONPath must select exactly one value: {error}"))?;
    crate::checks::property_text(selected)
        .ok_or_else(|| format!("JSONPath {expression:?} selected a non-scalar value"))
}

fn selects_test_result(selector: &str, test_id: &str) -> bool {
    test_id == selector
        || test_id
            .strip_prefix(selector)
            .and_then(|suffix| suffix.strip_prefix('_'))
            .is_some_and(|digits| {
                digits.len() >= 2 && digits.bytes().all(|byte| byte.is_ascii_digit())
            })
}

fn required_parameter<'a>(
    parameters: &'a BTreeMap<String, String>,
    name: &str,
) -> Result<&'a str, String> {
    parameters
        .get(name)
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("stats test requires parameter {name:?}"))
}
