//! Purpose: Evaluates run-wide statistical checks using one sample per tested deliverable.
use chrono::{DateTime, NaiveDate};
use regex::Regex;
use serde_json::{Value, json};
use serde_json_path::JsonPath;
use std::{cmp::Ordering, collections::BTreeMap};
use thot_core::{ProjectCheck, ProjectTest};

use crate::checks;

const STATISTICS: &[&str] = &[
    "max", "min", "avg", "freq", "p1", "p5", "p10", "p20", "p30", "p40", "p50", "p60", "p70",
    "p80", "p90", "p95", "p99", "var", "std",
];

pub(crate) struct StatsCheckResult {
    pub passed: u8,
    pub statistic: Value,
    pub sample_count: usize,
    pub student_value: Value,
}

pub(crate) fn is_stats_check(check_ref: &str) -> bool {
    matches!(check_ref, "stats/regex_extract" | "stats/yaml_extract")
}

pub(crate) fn valid_statistic(statistic: &str) -> bool {
    STATISTICS.contains(&statistic)
}

/// Calculate every statistic available for the selected scalar data type.
pub(crate) fn summarize_values(values: &[String], data_type: &str) -> Result<Value, String> {
    if values.is_empty() {
        return Err("no extracted values are available for statistical summary".into());
    }
    let samples = values
        .iter()
        .map(|value| parse_scalar(value, data_type, "extracted value"))
        .collect::<Result<Vec<_>, _>>()?;
    let mut summary = serde_json::Map::new();
    summary.insert("count".into(), json!(samples.len()));
    for name in STATISTICS {
        if data_type != "number" && matches!(*name, "avg" | "var" | "std")
            || data_type != "number" && name.starts_with('p')
        {
            continue;
        }
        let value = calculate_statistic(&samples, name, data_type)?;
        summary.insert((*name).into(), value.to_json());
    }
    Ok(Value::Object(summary))
}

/// Compare this delivery's extracted test value with a statistic over unique delivery reports.
pub(crate) fn evaluate_check(
    check: &ProjectCheck,
    test: &ProjectTest,
    current_report: &Value,
    reports_for_run: &[Value],
) -> Result<StatsCheckResult, String> {
    let parameters = check.parameters.into_strings()?;
    let property = check
        .property
        .as_deref()
        .ok_or("statistical check has no property")?;
    let data_type = required_parameter(&parameters, "data_type")?;
    let statistic_name = required_parameter(&parameters, "stat")?;
    let condition = required_parameter(&parameters, "condition")?;
    let test_id = if check.test == test.name {
        if test.repeat != 1 {
            return Err(format!(
                "statistical check for repeated test {:?} must name one result such as {:?}_01",
                test.name, test.name
            ));
        }
        test.name.as_str()
    } else {
        check.test.as_str()
    };

    let current_result = find_test_result(current_report, test_id)
        .ok_or_else(|| format!("test result {test_id:?} is missing for the current delivery"))?;
    let current_value = extract_value(check, current_result, property, &parameters)?;
    let current_scalar = parse_scalar(&current_value, data_type, "current delivery value")?;
    let current_key = report_delivery_key(current_report);

    // Reports are copied to every student in a group. Keep just one report per delivery,
    // preferring the current student's report so its own value is included in the sample set.
    let mut unique_reports = BTreeMap::new();
    unique_reports.insert(current_key.clone(), current_report);
    for report in reports_for_run {
        unique_reports
            .entry(report_delivery_key(report))
            .or_insert(report);
    }

    let mut samples = Vec::with_capacity(unique_reports.len());
    for (delivery_key, report) in unique_reports {
        let Some(result) = find_test_result(report, test_id) else {
            continue;
        };
        let extracted = match extract_value(check, result, property, &parameters) {
            Ok(value) => value,
            Err(error) if delivery_key == current_key => return Err(error),
            Err(_) => continue,
        };
        let scalar = match parse_scalar(&extracted, data_type, "delivery value") {
            Ok(value) => value,
            Err(error) if delivery_key == current_key => return Err(error),
            Err(_) => continue,
        };
        samples.push(scalar);
    }
    if !samples
        .iter()
        .any(|sample| sample.compare(&current_scalar) == Ordering::Equal)
    {
        samples.push(current_scalar.clone());
    }
    if samples.is_empty() {
        return Err("no deliverables have a usable value for this statistical check".into());
    }

    let statistic = calculate_statistic(&samples, statistic_name, data_type)?;
    let current_text = current_scalar.as_text();
    let statistic_text = statistic.as_text();
    let passed = checks::compare_captured(&current_text, &statistic_text, data_type, condition)?;
    Ok(StatsCheckResult {
        passed: u8::from(passed),
        statistic: statistic.to_json(),
        sample_count: samples.len(),
        student_value: current_scalar.to_json(),
    })
}

fn extract_value(
    check: &ProjectCheck,
    test_result: &Value,
    property: &str,
    parameters: &BTreeMap<String, String>,
) -> Result<String, String> {
    let value = checks::property_value(test_result, property)
        .ok_or_else(|| format!("test result property {property:?} does not exist"))?;
    match check.check_ref.as_str() {
        "stats/regex_extract" => {
            let expression = required_parameter(parameters, "regex")?;
            let regex =
                Regex::new(expression).map_err(|error| format!("invalid regex: {error}"))?;
            if regex.captures_len() < 2 {
                return Err("stats/regex_extract expression must contain a capture group".into());
            }
            let text = checks::property_text(value).ok_or_else(|| {
                format!("test result property {property:?} is not a scalar value")
            })?;
            let captures = regex
                .captures(&text)
                .ok_or("stats/regex_extract expression did not match the property")?;
            captures
                .get(1)
                .map(|capture| capture.as_str().to_owned())
                .ok_or_else(|| "stats/regex_extract did not produce capture group 1".into())
        }
        "stats/yaml_extract" => {
            let expression = parameters
                .get("JSONPath")
                .or_else(|| parameters.get("jsonpath"))
                .map(String::as_str)
                .filter(|value| !value.is_empty())
                .ok_or("stats/yaml_extract is missing parameter \"JSONPath\"")?;
            let path = JsonPath::parse(expression)
                .map_err(|error| format!("invalid stats/yaml_extract JSONPath: {error}"))?;
            let document = checks::parse_document(value, property)?;
            let selected = path.query(&document).exactly_one().map_err(|error| {
                format!("stats/yaml_extract JSONPath must select exactly one value: {error}")
            })?;
            checks::property_text(selected).ok_or_else(|| {
                format!("stats/yaml_extract JSONPath {expression:?} selected a non-scalar value")
            })
        }
        other => Err(format!("unsupported statistical check {other:?}")),
    }
}

fn find_test_result<'a>(report: &'a Value, test_id: &str) -> Option<&'a Value> {
    report["test_results"]
        .as_array()?
        .iter()
        .find(|row| row["test_id"].as_str() == Some(test_id))
}

fn report_delivery_key(report: &Value) -> String {
    report["delivery_id"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| report["expedient_id"].as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("report:{:p}", report))
}

#[derive(Clone, Debug)]
enum Scalar {
    Number(f64),
    Text(String),
    Date(NaiveDate),
    DateTime(DateTime<chrono::FixedOffset>),
}

impl Scalar {
    fn compare(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Number(left), Self::Number(right)) => {
                left.partial_cmp(right).unwrap_or(Ordering::Equal)
            }
            (Self::Text(left), Self::Text(right)) => left.cmp(right),
            (Self::Date(left), Self::Date(right)) => left.cmp(right),
            (Self::DateTime(left), Self::DateTime(right)) => left.cmp(right),
            _ => Ordering::Equal,
        }
    }

    fn as_text(&self) -> String {
        match self {
            Self::Number(value) => value.to_string(),
            Self::Text(value) => value.clone(),
            Self::Date(value) => value.format("%Y-%m-%d").to_string(),
            Self::DateTime(value) => value.to_rfc3339(),
        }
    }

    fn to_json(&self) -> Value {
        match self {
            Self::Number(value) => json!(value),
            _ => json!(self.as_text()),
        }
    }
}

fn parse_scalar(value: &str, data_type: &str, label: &str) -> Result<Scalar, String> {
    match data_type {
        "number" => checks::parse_finite_number(value, label).map(Scalar::Number),
        "text" => Ok(Scalar::Text(value.to_owned())),
        "date" => NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .map(Scalar::Date)
            .map_err(|error| format!("{label} is not a YYYY-MM-DD date: {error}")),
        "datetime" => DateTime::parse_from_rfc3339(value)
            .map(Scalar::DateTime)
            .map_err(|error| format!("{label} is not an RFC 3339 datetime: {error}")),
        other => Err(format!("unsupported statistical data_type {other:?}")),
    }
}

fn calculate_statistic(samples: &[Scalar], name: &str, data_type: &str) -> Result<Scalar, String> {
    let numeric = || -> Result<Vec<f64>, String> {
        if data_type != "number" {
            return Err(format!("statistic {name:?} requires data_type number"));
        }
        samples
            .iter()
            .map(|sample| match sample {
                Scalar::Number(value) => Ok(*value),
                _ => Err("numeric statistic received a non-number sample".into()),
            })
            .collect()
    };

    match name {
        "max" => samples
            .iter()
            .max_by(|left, right| left.compare(right))
            .cloned()
            .ok_or_else(|| "cannot calculate max without samples".into()),
        "min" => samples
            .iter()
            .min_by(|left, right| left.compare(right))
            .cloned()
            .ok_or_else(|| "cannot calculate min without samples".into()),
        "freq" => {
            let mut sorted = samples.to_vec();
            sorted.sort_by(Scalar::compare);
            let mut best: Option<(Scalar, usize)> = None;
            let mut index = 0;
            while index < sorted.len() {
                let value = sorted[index].clone();
                let mut end = index + 1;
                while end < sorted.len() && sorted[end].compare(&value) == Ordering::Equal {
                    end += 1;
                }
                if best.as_ref().is_none_or(|(_, count)| end - index > *count) {
                    best = Some((value, end - index));
                }
                index = end;
            }
            best.map(|(value, _)| value)
                .ok_or_else(|| "cannot calculate freq without samples".into())
        }
        "avg" => {
            let values = numeric()?;
            let mean = values.iter().sum::<f64>() / values.len() as f64;
            finite_statistic(mean, name)
        }
        "var" | "std" => {
            let values = numeric()?;
            let mean = values.iter().sum::<f64>() / values.len() as f64;
            let variance = values
                .iter()
                .map(|value| (value - mean).powi(2))
                .sum::<f64>()
                / values.len() as f64;
            finite_statistic(
                if name == "var" {
                    variance
                } else {
                    variance.sqrt()
                },
                name,
            )
        }
        percentile => {
            let Some(percent) = percentile
                .strip_prefix('p')
                .and_then(|value| value.parse::<u32>().ok())
            else {
                return Err(format!("unknown statistical measure {name:?}"));
            };
            if !valid_statistic(percentile) {
                return Err(format!("unknown statistical measure {name:?}"));
            }
            let mut values = numeric()?;
            values.sort_by(f64::total_cmp);
            let position = (values.len() - 1) as f64 * f64::from(percent) / 100.0;
            let lower = position.floor() as usize;
            let upper = position.ceil() as usize;
            let fraction = position - lower as f64;
            finite_statistic(
                values[lower] + (values[upper] - values[lower]) * fraction,
                name,
            )
        }
    }
}

fn finite_statistic(value: f64, name: &str) -> Result<Scalar, String> {
    if value.is_finite() {
        Ok(Scalar::Number(value))
    } else {
        Err(format!("statistic {name:?} is not finite"))
    }
}

fn required_parameter<'a>(
    parameters: &'a BTreeMap<String, String>,
    name: &str,
) -> Result<&'a str, String> {
    parameters
        .get(name)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("statistical check is missing parameter {name:?}"))
}
