//! Purpose: Calculates numeric rubric results from the configured check outcomes.
use chrono::Utc;
use serde_json::{Value, json};
use std::time::Instant;
use thot_core::ProjectRubric;

const PERCENTILES: &[u32] = &[1, 5, 10, 20, 30, 40, 50, 60, 70, 80, 90, 95, 99];

pub fn supported_calculations() -> &'static [&'static str] {
    &[
        "avg",
        "max",
        "min",
        "median",
        "p1",
        "p5",
        "p10",
        "p20",
        "p30",
        "p40",
        "p50",
        "p60",
        "p70",
        "p80",
        "p90",
        "p95",
        "p99",
        "weighted_avg",
        "sum",
    ]
}

/// Evaluate each rubric against the check rows from one student's expedient.
pub fn evaluate_rubrics(rubrics: &[ProjectRubric], check_rows: &[Value]) -> Vec<Value> {
    rubrics
        .iter()
        .map(|rubric| {
            let started = Instant::now();
            let result = evaluate_one(rubric, check_rows);
            let (value, check_results, weights, error) = match result {
                Ok((value, check_results, weights)) => {
                    (json!(value), check_results, weights, Value::Null)
                }
                Err(error) => (Value::Null, Vec::new(), None, json!(error)),
            };
            let mut row = json!({
                "name": rubric.name,
                "description": rubric.description,
                "calculation": rubric.calculation,
                "checks": rubric.checks,
                "check_results": check_results,
                "result": value,
                "err": error,
                "date_time": Utc::now().to_rfc3339(),
                "duration_ms": started.elapsed().as_millis(),
            });
            if let Some(weights) = weights {
                row["weights"] = json!(weights);
            }
            row
        })
        .collect()
}

fn evaluate_one(
    rubric: &ProjectRubric,
    check_rows: &[Value],
) -> Result<(f64, Vec<Value>, Option<Vec<f64>>), String> {
    let mut values = Vec::with_capacity(rubric.checks.len());
    let mut results = Vec::with_capacity(rubric.checks.len());
    for check_name in &rubric.checks {
        let row = check_rows
            .iter()
            .find(|row| row["name"].as_str() == Some(check_name.as_str()))
            .ok_or_else(|| format!("check result {check_name:?} is missing"))?;
        let result = row["result"]
            .as_f64()
            .filter(|value| value.is_finite())
            .ok_or_else(|| format!("check result {check_name:?} is not a finite number"))?;
        values.push(result);
        results.push(json!({
            "name": check_name,
            "result": result,
            "err": row["err"],
        }));
    }

    if values.is_empty() {
        return Err("cannot calculate a rubric without checks".into());
    }
    let weights = if rubric.calculation == "weighted_avg" {
        let parameters = rubric.parameters.into_strings()?;
        let encoded = parameters
            .get("weights")
            .ok_or("weighted_avg rubric requires parameter weights")?;
        let weights = parse_weights(encoded)?;
        if weights.len() != values.len() {
            return Err(format!(
                "weighted_avg has {} weights for {} checks",
                weights.len(),
                values.len()
            ));
        }
        Some(weights)
    } else {
        None
    };
    let result = calculate(&values, &rubric.calculation, weights.as_deref())?;
    Ok((result, results, weights))
}

fn parse_weights(value: &str) -> Result<Vec<f64>, String> {
    value
        .split(',')
        .map(|part| {
            let weight = part
                .parse::<f64>()
                .map_err(|_| format!("weight {part:?} is not a number"))?;
            if !weight.is_finite() || weight < 0.0 {
                return Err(format!(
                    "weight {part:?} must be a finite non-negative number"
                ));
            }
            Ok(weight)
        })
        .collect()
}

fn calculate(values: &[f64], calculation: &str, weights: Option<&[f64]>) -> Result<f64, String> {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let result = match calculation {
        "avg" => values.iter().sum::<f64>() / values.len() as f64,
        "max" => *sorted.last().ok_or("cannot calculate max without checks")?,
        "min" => *sorted
            .first()
            .ok_or("cannot calculate min without checks")?,
        "median" => percentile(&sorted, 50),
        "sum" => values.iter().sum(),
        "weighted_avg" => {
            let weights = weights.ok_or("weighted_avg rubric is missing weights")?;
            let weight_sum = weights.iter().sum::<f64>();
            if weight_sum <= 0.0 {
                return Err("weighted_avg weights must have a positive sum".into());
            }
            values
                .iter()
                .zip(weights)
                .map(|(value, weight)| value * weight)
                .sum::<f64>()
                / weight_sum
        }
        value if value.starts_with('p') => {
            let percentile_rank = value[1..]
                .parse::<u32>()
                .map_err(|_| format!("unknown rubric calculation {calculation:?}"))?;
            if !PERCENTILES.contains(&percentile_rank) {
                return Err(format!("unknown rubric calculation {calculation:?}"));
            }
            percentile(&sorted, percentile_rank)
        }
        _ => return Err(format!("unknown rubric calculation {calculation:?}")),
    };
    if result.is_finite() {
        Ok(result)
    } else {
        Err(format!("rubric calculation {calculation:?} is not finite"))
    }
}

fn percentile(sorted: &[f64], percentile: u32) -> f64 {
    let position = (sorted.len() - 1) as f64 * f64::from(percentile) / 100.0;
    let lower = position.floor() as usize;
    let upper = position.ceil() as usize;
    if lower == upper {
        sorted[lower]
    } else {
        let fraction = position - lower as f64;
        sorted[lower] + (sorted[upper] - sorted[lower]) * fraction
    }
}
