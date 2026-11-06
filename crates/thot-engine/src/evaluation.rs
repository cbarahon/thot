//! Purpose: Calculates final evaluation marks from configured rubric outcomes.
use chrono::Utc;
use serde_json::{Value, json};
use std::time::Instant;
use thot_core::ProjectEvaluation;

/// Evaluate configured final-mark items against this student's rubric rows.
pub fn evaluate_items(
    items: &[ProjectEvaluation],
    rubric_rows: &[Value],
    evaluation_multiplier: f64,
) -> Vec<Value> {
    items
        .iter()
        .map(|item| {
            let started = Instant::now();
            let result = evaluate_one(item, rubric_rows, evaluation_multiplier);
            let (value, rubric_results, weights, error) = match result {
                Ok((value, rubric_results, weights)) => {
                    (json!(value), rubric_results, weights, Value::Null)
                }
                Err(error) => (Value::Null, Vec::new(), None, json!(error)),
            };
            let mut row = json!({
                "name": item.name,
                "description": item.description,
                "calculation": item.calculation,
                "rubrics": item.rubrics,
                "rubric_results": rubric_results,
                "evaluation_multiplier": evaluation_multiplier,
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
    item: &ProjectEvaluation,
    rubric_rows: &[Value],
    evaluation_multiplier: f64,
) -> Result<(f64, Vec<Value>, Option<Vec<f64>>), String> {
    if !evaluation_multiplier.is_finite() || evaluation_multiplier <= 0.0 {
        return Err("evaluation_multiplier must be a finite number greater than zero".into());
    }
    let mut values = Vec::with_capacity(item.rubrics.len());
    let mut results = Vec::with_capacity(item.rubrics.len());
    for rubric_name in &item.rubrics {
        let row = rubric_rows
            .iter()
            .find(|row| row["name"].as_str() == Some(rubric_name.as_str()))
            .ok_or_else(|| format!("rubric result {rubric_name:?} is missing"))?;
        if !row["err"].is_null() {
            return Err(format!(
                "rubric result {rubric_name:?} has an error: {}",
                row["err"].as_str().unwrap_or("unknown error")
            ));
        }
        let value = row["result"]
            .as_f64()
            .filter(|value| value.is_finite())
            .ok_or_else(|| format!("rubric result {rubric_name:?} is not a finite number"))?;
        values.push(value);
        results.push(json!({"name": rubric_name, "result": value}));
    }
    if values.is_empty() {
        return Err("cannot calculate an evaluation item without rubrics".into());
    }
    let weights = if item.calculation == "weighted_avg" {
        let parameters = item.parameters.into_strings()?;
        let encoded = parameters
            .get("weights")
            .ok_or("weighted_avg evaluation item requires parameter weights")?;
        let weights = parse_weights(encoded)?;
        if weights.len() != values.len() {
            return Err(format!(
                "weighted_avg has {} weights for {} rubrics",
                weights.len(),
                values.len()
            ));
        }
        Some(weights)
    } else {
        None
    };
    let scaled = crate::rubrics::calculate(&values, &item.calculation, weights.as_deref())?
        * evaluation_multiplier;
    if !scaled.is_finite() {
        return Err(format!(
            "evaluation calculation {:?} is not finite",
            item.calculation
        ));
    }
    Ok((scaled, results, weights))
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
