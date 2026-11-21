use serde_json::Value;
use std::time::Instant;
use chrono::Utc;

/// Evaluate engine-owned observation and rubric for small expression subset.
pub fn evaluate_observation(
    observation: &ProjectObservation,
    test_results: &[ProjectTestResult],
    rubrics: &[ProjectRubric],
    rubrics_multiplier: f64,
) -> Vec<Value> {
    let mut check_rows = Vec::new();
    for result in test_results {
        if result.observation_id == observation.id {
            let passed = match &observation.expression {
                ProjectExpression::Addition { a, b, expected } => {
                    a + b == *expected
                }
                ProjectExpression::Multiplication { a, b, expected } => {
                    a * b == *expected
                }
                ProjectExpression::Division { a, b, expected } => {
                    *a / b == *expected
                }
            };
            check_rows.push(json!({
                "name": observation.name,
                "description": observation.description,
                "result": passed,
                "err": Value::Null,
                "date_time": Utc::now().to_rfc3339(),
                "duration_ms": 0,
            }));
        }
    }

    evaluate_rubrics(rubrics, &check_rows, rubrics_multiplier)
}