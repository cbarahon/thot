use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxCriterionResult {
    pub check: String,
    pub earned_points: u32,
    pub maximum_points: u32,
}

/// Evaluates a completed task observation against one declared exact-output check.
pub fn evaluate_sandbox_test(
    exit_code: Option<i32>,
    timed_out: bool,
    expected_exit_code: i32,
    expected_stdout: Option<&str>,
    stdout: &str,
) -> bool {
    !timed_out
        && exit_code == Some(expected_exit_code)
        && expected_stdout.is_none_or(|expected| expected == stdout)
}

/// Applies a weighted rubric check over the test IDs explicitly named by the professor.
pub fn evaluate_sandbox_rubric(
    checks: &HashMap<String, Vec<String>>,
    test_passed: &HashMap<String, bool>,
    criteria: &[(String, u32)],
) -> Result<Vec<SandboxCriterionResult>, String> {
    criteria
        .iter()
        .map(|(check, maximum_points)| {
            let tests = checks
                .get(check)
                .ok_or_else(|| format!("rubric references undefined check `{check}`"))?;
            if tests.is_empty()
                || tests
                    .iter()
                    .any(|test_id| !test_passed.contains_key(test_id))
            {
                return Err(format!(
                    "check `{check}` references no tests or an unknown test ID"
                ));
            }
            let passed = tests
                .iter()
                .all(|test_id| test_passed.get(test_id).copied().unwrap_or(false));
            Ok(SandboxCriterionResult {
                check: check.clone(),
                earned_points: if passed { *maximum_points } else { 0 },
                maximum_points: *maximum_points,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_test_check_requires_exit_and_stdout() {
        assert!(evaluate_sandbox_test(
            Some(0),
            false,
            0,
            Some("42\n"),
            "42\n"
        ));
        assert!(!evaluate_sandbox_test(
            Some(0),
            false,
            0,
            Some("42\n"),
            "425\n"
        ));
        assert!(!evaluate_sandbox_test(Some(0), true, 0, None, ""));
        assert!(!evaluate_sandbox_test(None, false, 0, None, ""));
    }

    #[test]
    fn weighted_rubric_uses_each_referenced_test() {
        let checks = HashMap::from([
            ("syntax_check".into(), vec!["syntax".into()]),
            ("functional_check".into(), vec!["sum-result".into()]),
        ]);
        let passed = HashMap::from([("syntax".into(), true), ("sum-result".into(), false)]);
        let results = evaluate_sandbox_rubric(
            &checks,
            &passed,
            &[("syntax_check".into(), 2), ("functional_check".into(), 8)],
        )
        .unwrap();
        assert_eq!(results[0].earned_points, 2);
        assert_eq!(results[1].earned_points, 0);
    }
}
