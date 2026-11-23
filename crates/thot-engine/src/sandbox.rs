use std::collections::HashMap;
use thot_core::sandbox_project::SandboxEvaluationItem;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxCriterionResult {
    pub check: String,
    pub earned_points: u32,
    pub maximum_points: u32,
    pub pending: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxItemStatus {
    Complete,
    Pending,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxEvaluationItemResult {
    pub id: String,
    pub title: String,
    pub grade: Option<u32>,
    pub scale: u32,
    pub status: SandboxItemStatus,
    pub criteria: Vec<SandboxCriterionResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxTestAssessment {
    Passed,
    Failed,
    Skipped,
    Pending,
}

/// Grades v3 items independently. Missing or infrastructure-pending evidence
/// keeps only the affected item pending; completed items retain their grade.
pub fn evaluate_sandbox_items(
    checks: &HashMap<String, Vec<String>>,
    assessments: &HashMap<String, SandboxTestAssessment>,
    items: &[SandboxEvaluationItem],
) -> Result<Vec<SandboxEvaluationItemResult>, String> {
    items
        .iter()
        .map(|item| {
            let mut pending = false;
            let mut criteria = Vec::with_capacity(item.criteria.len());
            for criterion in &item.criteria {
                let tests = checks.get(&criterion.check).ok_or_else(|| {
                    format!(
                        "evaluation references undefined check `{}`",
                        criterion.check
                    )
                })?;
                if tests.is_empty() {
                    return Err(format!("check `{}` references no tests", criterion.check));
                }
                let states = tests
                    .iter()
                    .map(|test_id| {
                        assessments.get(test_id).copied().ok_or_else(|| {
                            format!(
                                "check `{}` references unknown test `{test_id}`",
                                criterion.check
                            )
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let criterion_pending = states.contains(&SandboxTestAssessment::Pending);
                pending |= criterion_pending;
                let passed = states
                    .iter()
                    .all(|state| *state == SandboxTestAssessment::Passed);
                criteria.push(SandboxCriterionResult {
                    check: criterion.check.clone(),
                    earned_points: if passed { criterion.points } else { 0 },
                    maximum_points: criterion.points,
                    pending: criterion_pending,
                });
            }
            let grade = (!pending).then(|| criteria.iter().map(|row| row.earned_points).sum());
            Ok(SandboxEvaluationItemResult {
                id: item.id.clone(),
                title: item.title.clone(),
                grade,
                scale: item.scale,
                status: if pending {
                    SandboxItemStatus::Pending
                } else {
                    SandboxItemStatus::Complete
                },
                criteria,
            })
        })
        .collect()
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
                pending: false,
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

    #[test]
    fn v3_items_are_independent_when_one_has_infrastructure_pending_evidence() {
        let checks = HashMap::from([
            ("layout".into(), vec!["partitions".into()]),
            ("boot".into(), vec!["boot-windows".into()]),
        ]);
        let assessments = HashMap::from([
            ("partitions".into(), SandboxTestAssessment::Passed),
            ("boot-windows".into(), SandboxTestAssessment::Pending),
        ]);
        let items = vec![
            SandboxEvaluationItem {
                id: "layout".into(),
                title: "Disk layout".into(),
                calculation: "weighted_sum".into(),
                scale: 10,
                criteria: vec![thot_core::sandbox_project::SandboxCriterion {
                    check: "layout".into(),
                    points: 10,
                }],
            },
            SandboxEvaluationItem {
                id: "runtime".into(),
                title: "Boot".into(),
                calculation: "weighted_sum".into(),
                scale: 10,
                criteria: vec![thot_core::sandbox_project::SandboxCriterion {
                    check: "boot".into(),
                    points: 10,
                }],
            },
        ];
        let results = evaluate_sandbox_items(&checks, &assessments, &items).unwrap();
        assert_eq!(results[0].grade, Some(10));
        assert_eq!(results[0].status, SandboxItemStatus::Complete);
        assert_eq!(results[1].grade, None);
        assert_eq!(results[1].status, SandboxItemStatus::Pending);
        assert!(results[1].criteria[0].pending);
    }
}
