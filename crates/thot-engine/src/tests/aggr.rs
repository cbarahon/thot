//! Purpose: Selects or joins output from earlier tests in the same delivery.
use serde_json::Value;
use std::collections::HashSet;
use thot_core::test_results::TestStatus;

use crate::{
    checks,
    test_execution::{TestInvocation, TestOutcome},
};

pub async fn run(invocation: &TestInvocation) -> TestOutcome {
    let selectors = match invocation.parameters.get("tests") {
        Some(value) => value
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .collect::<Vec<_>>(),
        None => return TestOutcome::error("aggregation test requires parameter tests"),
    };
    if selectors.is_empty() {
        return TestOutcome::error("aggregation test parameter tests must list at least one test");
    }

    match invocation.test_reference.as_str() {
        "aggr/select" => select(invocation, &selectors),
        "aggr/join" => join(invocation, &selectors),
        unknown => TestOutcome::error(format!("unsupported aggregation test {unknown:?}")),
    }
}

fn select(invocation: &TestInvocation, selectors: &[&str]) -> TestOutcome {
    let property = invocation
        .parameters
        .get("property")
        .map(String::as_str)
        .unwrap_or("status");
    let condition = invocation
        .parameters
        .get("condition")
        .map(String::as_str)
        .unwrap_or("==");
    let default_value = "passed";
    let expected = invocation
        .parameters
        .get("value")
        .map(String::as_str)
        .or_else(|| (property == "status" && condition == "==").then_some(default_value));

    let mut matches = Vec::new();
    for result in selected_results(invocation, selectors) {
        let Some(candidate) = checks::property_value(result, property) else {
            continue;
        };
        match checks::evaluate_condition(candidate, expected, None, condition) {
            Ok(true) => matches.push(result),
            Ok(false) => {}
            Err(error) => return TestOutcome::error(format!("aggr/select: {error}")),
        }
    }

    match matches.as_slice() {
        [result] => TestOutcome {
            status: TestStatus::Passed,
            stdout: result["stdout"].as_str().unwrap_or_default().to_owned(),
            stderr: result["stderr"].as_str().unwrap_or_default().to_owned(),
            exit_code: result["exit_code"]
                .as_i64()
                .and_then(|code| i32::try_from(code).ok()),
            diagnostics: String::new(),
            observation: serde_json::json!({}),
            evidence: Vec::new(),
        },
        [] => failed("aggr/select found no test result matching the requested condition"),
        _ => failed(format!(
            "aggr/select found {} test results matching the requested condition; select a condition that matches exactly one",
            matches.len()
        )),
    }
}

fn join(invocation: &TestInvocation, selectors: &[&str]) -> TestOutcome {
    let filter_property = invocation.parameters.get("filter_property");
    let filter_condition = invocation.parameters.get("filter_condition");
    let filter_value = invocation.parameters.get("filter_value");
    let filtering =
        filter_property.is_some() || filter_condition.is_some() || filter_value.is_some();
    if filtering && (filter_property.is_none() || filter_condition.is_none()) {
        return TestOutcome::error(
            "aggr/join filtering requires both filter_property and filter_condition",
        );
    }

    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut exit_code = 0;
    let mut included = 0usize;
    for result in selected_results(invocation, selectors) {
        if let (Some(property), Some(condition)) = (filter_property, filter_condition) {
            let Some(candidate) = checks::property_value(result, property) else {
                continue;
            };
            match checks::evaluate_condition(
                candidate,
                filter_value.map(String::as_str),
                None,
                condition,
            ) {
                Ok(true) => {}
                Ok(false) => continue,
                Err(error) => return TestOutcome::error(format!("aggr/join: {error}")),
            }
        }
        included += 1;
        append_text(&mut stdout, result["stdout"].as_str().unwrap_or_default());
        append_text(&mut stderr, result["stderr"].as_str().unwrap_or_default());
        if result["exit_code"].as_i64() != Some(0) {
            exit_code = 1;
        }
    }
    let status = if exit_code == 0 {
        TestStatus::Passed
    } else {
        TestStatus::Failed
    };
    TestOutcome {
        status,
        stdout,
        stderr,
        exit_code: Some(exit_code),
        diagnostics: if included == 0 {
            "aggr/join selected no test results".into()
        } else {
            String::new()
        },
        observation: serde_json::json!({}),
        evidence: Vec::new(),
    }
}

fn selected_results<'a>(invocation: &'a TestInvocation, selectors: &[&str]) -> Vec<&'a Value> {
    let mut seen = HashSet::new();
    let mut results = Vec::new();
    for selector in selectors {
        for result in &invocation.prior_test_results {
            let test_id = result["test_id"].as_str().unwrap_or_default();
            if selects_test_result(selector, test_id) && seen.insert(test_id.to_owned()) {
                results.push(result);
            }
        }
    }
    results
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

fn append_text(output: &mut String, text: &str) {
    if text.is_empty() {
        return;
    }
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
    output.push_str(text);
    if !output.ends_with('\n') {
        output.push('\n');
    }
}

fn failed(message: impl Into<String>) -> TestOutcome {
    let message = message.into();
    TestOutcome {
        status: TestStatus::Failed,
        stdout: String::new(),
        stderr: message.clone(),
        exit_code: Some(1),
        diagnostics: message,
        observation: serde_json::json!({}),
        evidence: Vec::new(),
    }
}
