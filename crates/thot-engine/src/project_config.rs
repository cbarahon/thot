//! Purpose: Parses and validates the root configuration.yaml project test model.
use regex::Regex;
use serde_json_path::JsonPath;
use std::collections::{BTreeMap, HashSet};
use thot_core::{
    LABELED_GREP_FILTER_PREFIX, LABELED_JSONPATH_FILTER_PREFIX, ProjectCheck, ProjectEvaluation,
    ProjectRubric, ProjectTest, VMActivityProject,
};

pub fn parse_configuration(bytes: &[u8]) -> Result<VMActivityProject, String> {
    let project: VMActivityProject =
        serde_yaml::from_slice(bytes).map_err(|e| format!("invalid configuration.yaml: {e}"))?;
    validate_project(&project)?;
    Ok(project)
}

pub fn validate_project(project: &VMActivityProject) -> Result<(), String> {
    if project.activity_name.trim().is_empty() {
        return Err("activity_name must not be empty".into());
    }
    if project.delivery_folder.trim().is_empty() {
        return Err("delivery_folder must not be empty".into());
    }
    if project.concurrency == 0 || project.concurrency > 64 {
        return Err("concurrency must be between 1 and 64".into());
    }
    if project.timeout == 0 || project.timeout > 86_400 {
        return Err("timeout must be between 1 and 86400 seconds".into());
    }
    for (key, multiplier) in [
        ("rubrics_multiplier", project.rubrics_multiplier),
        ("evaluation_multiplier", project.evaluation_multiplier),
    ] {
        if !multiplier.is_finite() || multiplier <= 0.0 {
            return Err(format!("{key} must be a finite number greater than zero"));
        }
    }
    if project
        .not_delivered_mark
        .is_some_and(|mark| !mark.is_finite())
    {
        return Err("not_delivered_mark must be a finite number or null".into());
    }
    if project.tests.is_empty() {
        return Err("tests must contain at least one test".into());
    }

    let mut names = HashSet::new();
    let mut prior_names = HashSet::new();
    let mut prior_repeats = BTreeMap::new();
    for test in &project.tests {
        validate_test(test, project.timeout)?;
        if !names.insert(test.name.as_str()) {
            return Err(format!("test name {:?} is repeated", test.name));
        }
        for requirement in &test.requirements {
            if !prior_names.contains(requirement.as_str()) {
                return Err(format!(
                    "test {:?} requirement {:?} must name an earlier test",
                    test.name, requirement
                ));
            }
        }
        let test_parameters = test.parameters.into_strings()?;
        if test.test_ref == "import/csv" {
            if test.repeat != 1 {
                return Err(format!(
                    "CSV test {:?} cannot be repeated; configure repeat: 1",
                    test.name
                ));
            }
            validate_csv_import_parameters(&test_parameters, "CSV test", true)?;
            if let Some(override_test) = test_parameters.get("override_test") {
                let target = project
                    .tests
                    .iter()
                    .find(|candidate| candidate.name == *override_test)
                    .ok_or_else(|| {
                        format!(
                            "CSV test {:?} override_test {:?} must name an existing test",
                            test.name, override_test
                        )
                    })?;
                if target.name == test.name || target.test_ref == "import/csv" {
                    return Err(format!(
                        "CSV test {:?} override_test must name a different non-import/csv test",
                        test.name
                    ));
                }
                if target.repeat != 1 {
                    return Err(format!(
                        "CSV test {:?} cannot override repeated test {:?}; configure repeat: 1",
                        test.name, target.name
                    ));
                }
            }
        }
        if test.test_ref == "network/nmap" {
            crate::tests::network::validate_parameters(&test_parameters)
                .map_err(|error| format!("test {:?}: {error}", test.name))?;
        }
        if matches!(
            test.test_ref.as_str(),
            "stats/regex_extract" | "stats/yaml_extract"
        ) {
            validate_stats_test(test, &test_parameters, &prior_names)?;
        }
        if matches!(test.test_ref.as_str(), "aggr/select" | "aggr/join") {
            validate_aggr_test(test, &test_parameters, &prior_names, &prior_repeats)?;
        }
        prior_names.insert(test.name.as_str());
        prior_repeats.insert(test.name.as_str(), test.repeat);
    }
    let mut check_names = HashSet::new();
    let has_check_csv = project
        .checks
        .iter()
        .any(|check| check.check_ref == "import/csv");
    for check in &project.checks {
        validate_check(check, &project.tests, &names)?;
        if !check_names.insert(check.name.as_str()) {
            return Err(format!("check name {:?} is repeated", check.name));
        }
    }
    validate_aggregate_check_dependencies(&project.checks, &check_names)?;
    let mut rubric_names = HashSet::new();
    let has_rubric_csv = project
        .rubrics
        .iter()
        .any(|rubric| rubric.calculation == "import/csv");
    for rubric in &project.rubrics {
        validate_rubric(rubric, &check_names, has_check_csv)?;
        if !rubric_names.insert(rubric.name.as_str()) {
            return Err(format!("rubric name {:?} is repeated", rubric.name));
        }
    }
    let mut evaluation_names = HashSet::new();
    let has_evaluation_rubric_csv = has_rubric_csv;
    for item in &project.evaluation {
        validate_evaluation(item, &rubric_names, has_evaluation_rubric_csv)?;
        if !evaluation_names.insert(item.name.as_str()) {
            return Err(format!("evaluation item name {:?} is repeated", item.name));
        }
    }
    Ok(())
}

/// Return configuration warnings that should be shown to the project owner.
pub fn configuration_warnings(rubrics_multiplier: f64, evaluation_multiplier: f64) -> Vec<String> {
    if rubrics_multiplier != 1.0 && evaluation_multiplier != 1.0 {
        vec![format!(
            "rubrics_multiplier ({}) and evaluation_multiplier ({}) are both different from 1; rubric marks are scaled first and evaluation marks are scaled again",
            rubrics_multiplier, evaluation_multiplier
        )]
    } else {
        Vec::new()
    }
}

fn validate_rubric(
    rubric: &ProjectRubric,
    check_names: &HashSet<&str>,
    has_check_csv: bool,
) -> Result<(), String> {
    if rubric.name.trim().is_empty()
        || !rubric
            .name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(format!("invalid rubric name {:?}", rubric.name));
    }
    if rubric.calculation == "import/csv" {
        let parameters = rubric.parameters.into_strings()?;
        validate_csv_import_parameters(&parameters, "CSV rubric", false)?;
        if !rubric.checks.is_empty() {
            return Err(format!(
                "CSV rubric {:?} does not accept a checks list",
                rubric.name
            ));
        }
        return Ok(());
    }
    if rubric.checks.is_empty() {
        return Err(format!(
            "rubric {:?} must reference at least one check",
            rubric.name
        ));
    }
    for check in &rubric.checks {
        if !check_names.contains(check.as_str()) && !has_check_csv {
            return Err(format!(
                "rubric {:?} references unknown check {:?}",
                rubric.name, check
            ));
        }
    }
    if !crate::rubrics::supported_calculations().contains(&rubric.calculation.as_str()) {
        return Err(format!(
            "rubric {:?} has unknown calculation {:?}; supported calculations are {}",
            rubric.name,
            rubric.calculation,
            crate::rubrics::supported_calculations().join(", ")
        ));
    }
    let parameters = rubric.parameters.into_strings()?;
    if rubric.calculation == "weighted_avg" {
        if parameters.keys().any(|key| key != "weights") {
            return Err(format!(
                "weighted_avg rubric {:?} accepts only the weights parameter",
                rubric.name
            ));
        }
        let weights = parameters.get("weights").ok_or_else(|| {
            format!(
                "weighted_avg rubric {:?} requires parameter weights",
                rubric.name
            )
        })?;
        let weights = weights
            .split(',')
            .map(|weight| {
                let value = weight.parse::<f64>().map_err(|_| {
                    format!("rubric {:?} has invalid weight {weight:?}", rubric.name)
                })?;
                if !value.is_finite() || value < 0.0 {
                    return Err(format!(
                        "rubric {:?} weights must be finite non-negative numbers",
                        rubric.name
                    ));
                }
                Ok(value)
            })
            .collect::<Result<Vec<_>, String>>()?;
        if weights.len() != rubric.checks.len() {
            return Err(format!(
                "weighted_avg rubric {:?} has {} weights for {} checks",
                rubric.name,
                weights.len(),
                rubric.checks.len()
            ));
        }
        let total_weight = weights.iter().sum::<f64>();
        if !total_weight.is_finite() || total_weight <= 0.0 {
            return Err(format!(
                "weighted_avg rubric {:?} weights must have a positive sum",
                rubric.name
            ));
        }
    } else if !parameters.is_empty() {
        return Err(format!(
            "rubric {:?} calculation {:?} does not accept parameters",
            rubric.name, rubric.calculation
        ));
    }
    Ok(())
}

fn validate_evaluation(
    item: &ProjectEvaluation,
    rubric_names: &HashSet<&str>,
    has_rubric_csv: bool,
) -> Result<(), String> {
    if item.name.trim().is_empty()
        || !item
            .name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(format!("invalid evaluation item name {:?}", item.name));
    }
    if item.calculation == "import/csv" {
        let parameters = item.parameters.into_strings()?;
        validate_csv_import_parameters(&parameters, "CSV evaluation", false)?;
        if !item.rubrics.is_empty() {
            return Err(format!(
                "CSV evaluation item {:?} does not accept a rubrics list",
                item.name
            ));
        }
        return Ok(());
    }
    if item.rubrics.is_empty() {
        return Err(format!(
            "evaluation item {:?} must reference at least one rubric",
            item.name
        ));
    }
    for rubric in &item.rubrics {
        if !rubric_names.contains(rubric.as_str()) && !has_rubric_csv {
            return Err(format!(
                "evaluation item {:?} references unknown rubric {:?}",
                item.name, rubric
            ));
        }
    }
    if !crate::rubrics::supported_calculations().contains(&item.calculation.as_str()) {
        return Err(format!(
            "evaluation item {:?} has unknown calculation {:?}; supported calculations are {}",
            item.name,
            item.calculation,
            crate::rubrics::supported_calculations().join(", ")
        ));
    }
    let parameters = item.parameters.into_strings()?;
    if item.calculation == "weighted_avg" {
        if parameters.keys().any(|key| key != "weights") {
            return Err(format!(
                "weighted_avg evaluation item {:?} accepts only the weights parameter",
                item.name
            ));
        }
        let weights = parameters.get("weights").ok_or_else(|| {
            format!(
                "weighted_avg evaluation item {:?} requires parameter weights",
                item.name
            )
        })?;
        let weights = weights
            .split(',')
            .map(|weight| {
                let value = weight.parse::<f64>().map_err(|_| {
                    format!(
                        "evaluation item {:?} has invalid weight {weight:?}",
                        item.name
                    )
                })?;
                if !value.is_finite() || value < 0.0 {
                    return Err(format!(
                        "evaluation item {:?} weights must be finite non-negative numbers",
                        item.name
                    ));
                }
                Ok(value)
            })
            .collect::<Result<Vec<_>, String>>()?;
        if weights.len() != item.rubrics.len() {
            return Err(format!(
                "weighted_avg evaluation item {:?} has {} weights for {} rubrics",
                item.name,
                weights.len(),
                item.rubrics.len()
            ));
        }
        let total_weight = weights.iter().sum::<f64>();
        if !total_weight.is_finite() || total_weight <= 0.0 {
            return Err(format!(
                "weighted_avg evaluation item {:?} weights must have a positive sum",
                item.name
            ));
        }
    } else if !parameters.is_empty() {
        return Err(format!(
            "evaluation item {:?} calculation {:?} does not accept parameters",
            item.name, item.calculation
        ));
    }
    Ok(())
}

fn validate_stats_test(
    test: &ProjectTest,
    parameters: &std::collections::BTreeMap<String, String>,
    prior_tests: &HashSet<&str>,
) -> Result<(), String> {
    let selectors = parameters
        .get("tests")
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("stats test {:?} requires parameter tests", test.name))?;
    let selected = selectors
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();
    if selected.is_empty() {
        return Err(format!(
            "stats test {:?} must select at least one earlier test",
            test.name
        ));
    }
    for name in selected {
        if !prior_tests.contains(name) {
            return Err(format!(
                "stats test {:?} selects {:?}, which must name an earlier configured test",
                test.name, name
            ));
        }
    }
    let property = parameters
        .get("property")
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("stats test {:?} requires parameter property", test.name))?;
    if property.split('.').any(str::is_empty) {
        return Err(format!(
            "stats test {:?} property must use non-empty dot-separated path components",
            test.name
        ));
    }
    if let Some(data_type) = parameters.get("data_type")
        && !matches!(data_type.as_str(), "number" | "text" | "date" | "datetime")
    {
        return Err(format!(
            "stats test {:?} data_type must be number, text, date, or datetime",
            test.name
        ));
    }
    match test.test_ref.as_str() {
        "stats/regex_extract" => {
            let expression = parameters
                .get("regex")
                .filter(|value| !value.is_empty())
                .ok_or_else(|| format!("stats test {:?} requires parameter regex", test.name))?;
            let regex = Regex::new(expression).map_err(|error| {
                format!("stats test {:?} has invalid regex: {error}", test.name)
            })?;
            if regex.captures_len() < 2 {
                return Err(format!(
                    "stats test {:?} regex must contain a capture group",
                    test.name
                ));
            }
        }
        "stats/yaml_extract" => {
            let expression = parameters
                .get("JSONPath")
                .or_else(|| parameters.get("jsonpath"))
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| format!("stats test {:?} requires parameter JSONPath", test.name))?;
            JsonPath::parse(expression).map_err(|error| {
                format!("stats test {:?} has invalid JSONPath: {error}", test.name)
            })?;
        }
        _ => unreachable!("only stats test references call this validator"),
    }
    Ok(())
}

fn validate_aggr_test(
    test: &ProjectTest,
    parameters: &BTreeMap<String, String>,
    prior_tests: &HashSet<&str>,
    prior_repeats: &BTreeMap<&str, usize>,
) -> Result<(), String> {
    let selectors = parameters
        .get("tests")
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("aggregation test {:?} requires parameter tests", test.name))?;
    let selected = selectors
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();
    if selected.is_empty() {
        return Err(format!(
            "aggregation test {:?} must select at least one earlier test",
            test.name
        ));
    }
    for selector in &selected {
        let valid_repetition = selector
            .rsplit_once('_')
            .filter(|(_, suffix)| {
                suffix.len() >= 2 && suffix.bytes().all(|byte| byte.is_ascii_digit())
            })
            .and_then(|(base, suffix)| suffix.parse::<usize>().ok().map(|index| (base, index)))
            .is_some_and(|(base, index)| {
                index > 0
                    && prior_repeats
                        .get(base)
                        .is_some_and(|repeat| index <= *repeat)
            });
        if !prior_tests.contains(selector) && !valid_repetition {
            return Err(format!(
                "aggregation test {:?} selects {:?}, which must name an earlier configured test",
                test.name, selector
            ));
        }
    }

    let allowed: &[&str] = if test.test_ref == "aggr/select" {
        &["tests", "property", "condition", "value"]
    } else {
        &[
            "tests",
            "filter_property",
            "filter_condition",
            "filter_value",
        ]
    };
    if let Some(unknown) = parameters
        .keys()
        .find(|key| !allowed.contains(&key.as_str()))
    {
        return Err(format!(
            "aggregation test {:?} does not support parameter {:?}",
            test.name, unknown
        ));
    }

    if test.test_ref == "aggr/select" {
        let property = parameters
            .get("property")
            .map(String::as_str)
            .unwrap_or("status");
        let condition = parameters
            .get("condition")
            .map(String::as_str)
            .unwrap_or("==");
        if property.trim().is_empty() || property.split('.').any(str::is_empty) {
            return Err(format!(
                "aggregation test {:?} property must use non-empty dot-separated path components",
                test.name
            ));
        }
        crate::checks::validate_condition_syntax(condition, Some("text"))
            .map_err(|error| format!("aggregation test {:?}: {error}", test.name))?;
        if !is_predicate_condition(condition)
            && parameters
                .get("value")
                .is_none_or(|value| value.trim().is_empty())
            && !(property == "status" && condition == "==")
        {
            return Err(format!(
                "aggregation test {:?} requires parameter value for condition {:?}",
                test.name, condition
            ));
        }
    } else {
        let has_filter = ["filter_property", "filter_condition", "filter_value"]
            .iter()
            .any(|key| parameters.contains_key(*key));
        if has_filter {
            let property = parameters
                .get("filter_property")
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    format!(
                        "aggregation test {:?} filtering requires filter_property",
                        test.name
                    )
                })?;
            if property.split('.').any(str::is_empty) {
                return Err(format!(
                    "aggregation test {:?} filter_property must use non-empty dot-separated path components",
                    test.name
                ));
            }
            let condition = parameters
                .get("filter_condition")
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    format!(
                        "aggregation test {:?} filtering requires filter_condition",
                        test.name
                    )
                })?;
            crate::checks::validate_condition_syntax(condition, Some("text"))
                .map_err(|error| format!("aggregation test {:?}: {error}", test.name))?;
            if !is_predicate_condition(condition)
                && parameters
                    .get("filter_value")
                    .is_none_or(|value| value.trim().is_empty())
            {
                return Err(format!(
                    "aggregation test {:?} requires filter_value for condition {:?}",
                    test.name, condition
                ));
            }
        }
    }
    Ok(())
}

fn is_predicate_condition(condition: &str) -> bool {
    condition.starts_with("r'") || condition.starts_with("json'")
}

fn validate_check(
    check: &ProjectCheck,
    tests: &[ProjectTest],
    test_names: &HashSet<&str>,
) -> Result<(), String> {
    let valid_name = |name: &str| {
        !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    };
    if !valid_name(&check.name) {
        return Err(format!("invalid check name {:?}", check.name));
    }
    let parameters = check.parameters.into_strings()?;
    if check.check_ref == "aggr/checks" {
        validate_aggregate_check_parameters(check, &parameters)?;
        return Ok(());
    }
    if check.check_ref == "import/csv" {
        validate_csv_import_parameters(&parameters, "CSV check", false)?;
        return Ok(());
    }
    let statistical = crate::stats::is_stats_check(&check.check_ref);
    let matched_test = tests.iter().find(|test| {
        test.name == check.test
            || (statistical
                && crate::checks::expected_test_ids(test)
                    .iter()
                    .any(|test_id| test_id == &check.test))
    });
    if (!statistical && !test_names.contains(check.test.as_str())) || matched_test.is_none() {
        return Err(format!(
            "check {:?} refers to unknown test {:?}",
            check.name, check.test
        ));
    }
    if statistical && matched_test.is_some_and(|test| test.repeat > 1 && test.name == check.test) {
        return Err(format!(
            "statistical check {:?} must refer to one repeated test result, for example {:?}_01",
            check.name, check.test
        ));
    }
    match check.check_ref.as_str() {
        "pass" => Ok(()),
        "regex_match" => {
            validate_extract_property(check, "regex_match")?;
            let regex = parameters
                .get("regex")
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    format!(
                        "check {:?} using regex_match requires parameter regex",
                        check.name
                    )
                })?;
            Regex::new(regex).map_err(|error| {
                format!(
                    "check {:?} has invalid regex parameter: {error}",
                    check.name
                )
            })?;
            Ok(())
        }
        "regex_extract" => {
            validate_extract_property(check, "regex_extract")?;
            let regex = parameters.get("regex").ok_or_else(|| {
                format!(
                    "check {:?} using regex_extract requires parameter regex",
                    check.name
                )
            })?;
            let compiled = Regex::new(regex).map_err(|error| {
                format!(
                    "check {:?} has invalid regex parameter: {error}",
                    check.name
                )
            })?;
            if compiled.captures_len() < 2 {
                return Err(format!(
                    "check {:?} regex must contain a capture group",
                    check.name
                ));
            }
            validate_comparison_parameters(check, &parameters, "regex_extract")
        }
        "yaml_extract" => {
            validate_extract_property(check, "yaml_extract")?;
            let expression = parameters
                .get("JSONPath")
                .or_else(|| parameters.get("jsonpath"))
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    format!(
                        "check {:?} using yaml_extract requires parameter JSONPath",
                        check.name
                    )
                })?;
            JsonPath::parse(expression)
                .map_err(|error| format!("check {:?} has invalid JSONPath: {error}", check.name))?;
            validate_comparison_parameters(check, &parameters, "yaml_extract")
        }
        "yaml_match" => {
            validate_extract_property(check, "yaml_match")?;
            let expression = parameters
                .get("JSONPath")
                .or_else(|| parameters.get("jsonpath"))
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    format!(
                        "check {:?} using yaml_match requires parameter JSONPath",
                        check.name
                    )
                })?;
            JsonPath::parse(expression)
                .map_err(|error| format!("check {:?} has invalid JSONPath: {error}", check.name))?;
            Ok(())
        }
        "stats/regex_extract" => {
            validate_extract_property(check, "stats/regex_extract")?;
            let regex = parameters.get("regex").ok_or_else(|| {
                format!(
                    "check {:?} using stats/regex_extract requires parameter regex",
                    check.name
                )
            })?;
            let compiled = Regex::new(regex).map_err(|error| {
                format!(
                    "check {:?} has invalid regex parameter: {error}",
                    check.name
                )
            })?;
            if compiled.captures_len() < 2 {
                return Err(format!(
                    "check {:?} regex must contain a capture group",
                    check.name
                ));
            }
            validate_stat_parameters(check, &parameters)
        }
        "stats/yaml_extract" => {
            validate_extract_property(check, "stats/yaml_extract")?;
            let expression = parameters
                .get("JSONPath")
                .or_else(|| parameters.get("jsonpath"))
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    format!(
                        "check {:?} using stats/yaml_extract requires parameter JSONPath",
                        check.name
                    )
                })?;
            JsonPath::parse(expression)
                .map_err(|error| format!("check {:?} has invalid JSONPath: {error}", check.name))?;
            validate_stat_parameters(check, &parameters)
        }
        unknown => Err(format!(
            "check {:?} uses unknown internal check {unknown:?}",
            check.name
        )),
    }
}

fn validate_aggregate_check_parameters(
    check: &ProjectCheck,
    parameters: &BTreeMap<String, String>,
) -> Result<(), String> {
    if !check.test.is_empty() || check.property.is_some() {
        return Err(format!(
            "aggregate check {:?} does not use test or property fields",
            check.name
        ));
    }
    if let Some(unknown) = parameters
        .keys()
        .find(|key| !matches!(key.as_str(), "checks" | "condition" | "value"))
    {
        return Err(format!(
            "aggregate check {:?} does not support parameter {unknown:?}",
            check.name
        ));
    }
    let names = aggregate_check_names(check, parameters)?;
    let condition = parameters
        .get("condition")
        .filter(|condition| !condition.trim().is_empty())
        .ok_or_else(|| {
            format!(
                "aggregate check {:?} requires parameter condition",
                check.name
            )
        })?;
    if matches!(
        condition.as_str(),
        "all" | "any" | "one" | "none" | "majority"
    ) {
        if parameters.contains_key("value") {
            return Err(format!(
                "aggregate check {:?} condition {condition:?} does not use parameter value",
                check.name
            ));
        }
    } else if matches!(condition.as_str(), "<" | "<=" | ">" | ">=" | "==" | "!=") {
        let value = parameters
            .get("value")
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                format!(
                    "aggregate check {:?} condition {condition:?} requires a non-negative integer value",
                    check.name
                )
            })?;
        value.parse::<usize>().map_err(|_| {
            format!(
                "aggregate check {:?} value must be a non-negative integer",
                check.name
            )
        })?;
    } else {
        return Err(format!(
            "aggregate check {:?} condition must be all, any, one, none, majority, >=, <=, <, >, ==, or !=",
            check.name
        ));
    }
    if names.is_empty() {
        return Err(format!(
            "aggregate check {:?} requires a non-empty checks list",
            check.name
        ));
    }
    let mut unique = HashSet::new();
    if names.iter().any(|name| !unique.insert(name)) {
        return Err(format!(
            "aggregate check {:?} checks list contains a duplicate name",
            check.name
        ));
    }
    Ok(())
}

fn aggregate_check_names(
    check: &ProjectCheck,
    parameters: &BTreeMap<String, String>,
) -> Result<Vec<String>, String> {
    let value = parameters
        .get("checks")
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("aggregate check {:?} requires parameter checks", check.name))?;
    let names = value
        .split(',')
        .map(str::trim)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if names.iter().any(|name| {
        name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    }) {
        return Err(format!(
            "aggregate check {:?} checks must be non-empty valid check names",
            check.name
        ));
    }
    Ok(names)
}

fn validate_aggregate_check_dependencies(
    checks: &[ProjectCheck],
    check_names: &HashSet<&str>,
) -> Result<(), String> {
    let definitions = checks
        .iter()
        .map(|check| (check.name.as_str(), check))
        .collect::<std::collections::HashMap<_, _>>();
    for check in checks
        .iter()
        .filter(|check| check.check_ref == "aggr/checks")
    {
        let parameters = check.parameters.into_strings()?;
        for name in aggregate_check_names(check, &parameters)? {
            if !check_names.contains(name.as_str()) {
                return Err(format!(
                    "aggregate check {:?} references unknown check {:?}",
                    check.name, name
                ));
            }
        }
    }

    fn visit(
        name: &str,
        definitions: &std::collections::HashMap<&str, &ProjectCheck>,
        visiting: &mut HashSet<String>,
        visited: &mut HashSet<String>,
    ) -> Result<(), String> {
        if visited.contains(name) {
            return Ok(());
        }
        if !visiting.insert(name.to_owned()) {
            return Err(format!(
                "aggregate checks contain a dependency cycle involving {name:?}"
            ));
        }
        if let Some(check) = definitions.get(name)
            && check.check_ref == "aggr/checks"
        {
            let parameters = check.parameters.into_strings()?;
            for dependency in aggregate_check_names(check, &parameters)? {
                visit(&dependency, definitions, visiting, visited)?;
            }
        }
        visiting.remove(name);
        visited.insert(name.to_owned());
        Ok(())
    }

    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    for check in checks
        .iter()
        .filter(|check| check.check_ref == "aggr/checks")
    {
        visit(&check.name, &definitions, &mut visiting, &mut visited)?;
    }
    Ok(())
}

fn validate_csv_import_parameters(
    parameters: &BTreeMap<String, String>,
    label: &str,
    allow_override: bool,
) -> Result<(), String> {
    let allowed = if allow_override {
        &["filepath", "override_test"][..]
    } else {
        &["filepath", "descriptions"][..]
    };
    if let Some(unknown) = parameters
        .keys()
        .find(|key| !allowed.contains(&key.as_str()))
    {
        return Err(format!("{label} does not support parameter {unknown:?}"));
    }
    let filepath = parameters
        .get("filepath")
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{label} requires parameter filepath"))?;
    let path = std::path::Path::new(filepath);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(format!(
            "{label} filepath must be a project-relative path without '..' components"
        ));
    }
    if let Some(encoded) = parameters.get("descriptions") {
        let value: serde_json::Value = serde_json::from_str(encoded)
            .map_err(|error| format!("{label} descriptions parameter is invalid: {error}"))?;
        if !value.as_object().is_some_and(|mapping| {
            mapping
                .iter()
                .all(|(key, value)| !key.trim().is_empty() && value.is_string())
        }) {
            return Err(format!(
                "{label} descriptions must map non-empty column names to strings"
            ));
        }
    }
    if !allow_override && parameters.contains_key("override_test") {
        return Err(format!("{label} does not accept override_test"));
    }
    Ok(())
}

fn validate_stat_parameters(
    check: &ProjectCheck,
    parameters: &std::collections::BTreeMap<String, String>,
) -> Result<(), String> {
    let data_type = parameters
        .get("data_type")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("check {:?} requires parameter data_type", check.name))?;
    if !matches!(data_type.as_str(), "number" | "text" | "date" | "datetime") {
        return Err(format!(
            "check {:?} data_type must be number, text, date, or datetime",
            check.name
        ));
    }
    let statistic = parameters
        .get("stat")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("check {:?} requires parameter stat", check.name))?;
    if !crate::stats::valid_statistic(statistic) {
        return Err(format!(
            "check {:?} has unknown statistic {:?}; supported statistics are max, min, avg, freq, p1, p5, p10, p20, p30, p40, p50, p60, p70, p80, p90, p95, p99, var, and std",
            check.name, statistic
        ));
    }
    if matches!(statistic.as_str(), "avg" | "var" | "std") || statistic.starts_with('p') {
        if data_type != "number" {
            return Err(format!(
                "check {:?} statistic {:?} requires data_type number",
                check.name, statistic
            ));
        }
    }
    let condition = parameters
        .get("condition")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("check {:?} requires parameter condition", check.name))?;
    crate::checks::validate_condition_syntax(condition, Some(data_type))
        .map_err(|error| format!("check {:?}: {error}", check.name))
}

fn validate_extract_property(check: &ProjectCheck, check_type: &str) -> Result<(), String> {
    let property = check.property.as_deref().unwrap_or_default();
    if property.trim().is_empty() {
        return Err(format!(
            "check {:?} using {check_type} requires a non-empty property",
            check.name
        ));
    }
    if property.split('.').any(str::is_empty) {
        return Err(format!(
            "check {:?} property must use non-empty dot-separated path components",
            check.name
        ));
    }
    Ok(())
}

fn validate_comparison_parameters(
    check: &ProjectCheck,
    parameters: &std::collections::BTreeMap<String, String>,
    check_type: &str,
) -> Result<(), String> {
    let condition = parameters
        .get("condition")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            format!(
                "check {:?} using {check_type} requires parameter condition",
                check.name
            )
        })?;
    let data_type = parameters
        .get("data_type")
        .filter(|value| !value.is_empty());
    if let Some(data_type) = data_type
        && !matches!(data_type.as_str(), "number" | "text" | "date" | "datetime")
    {
        return Err(format!(
            "check {:?} data_type must be number, text, date, or datetime",
            check.name
        ));
    }
    crate::checks::validate_condition_syntax(condition, data_type.map(String::as_str))
        .map_err(|error| format!("check {:?}: {error}", check.name))?;
    if !is_predicate_condition(condition) && data_type.is_none() {
        return Err(format!(
            "check {:?} using {check_type} requires parameter data_type",
            check.name
        ));
    }
    let value = parameters.get("value").filter(|value| !value.is_empty());
    if !is_predicate_condition(condition) && value.is_none() {
        return Err(format!(
            "check {:?} using {check_type} requires parameter value",
            check.name
        ));
    }
    let (Some(data_type), Some(value)) = (data_type, value) else {
        return Ok(());
    };
    match data_type.as_str() {
        "number" => {
            let parsed = value
                .parse::<f64>()
                .map_err(|_| format!("check {:?} value must be a number", check.name))?;
            if !parsed.is_finite() {
                return Err(format!(
                    "check {:?} value must be a finite number",
                    check.name
                ));
            }
        }
        "date" => {
            chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| {
                format!(
                    "check {:?} value must use YYYY-MM-DD for data_type date",
                    check.name
                )
            })?;
        }
        "datetime" => {
            chrono::DateTime::parse_from_rfc3339(value).map_err(|_| {
                format!(
                    "check {:?} value must be RFC 3339 for data_type datetime",
                    check.name
                )
            })?;
        }
        "text" => {}
        _ => unreachable!("data_type was validated above"),
    }
    Ok(())
}

fn validate_test(test: &ProjectTest, global_timeout: u64) -> Result<(), String> {
    if test.name.is_empty()
        || !test
            .name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(format!("invalid test name {:?}", test.name));
    }
    if test.repeat == 0 || test.repeat > 100 {
        return Err(format!(
            "test {:?} repeat must be between 1 and 100",
            test.name
        ));
    }
    if test
        .timeout
        .is_some_and(|seconds| seconds == 0 || seconds > 86_400)
    {
        return Err(format!(
            "test {:?} timeout must be between 1 and 86400 seconds",
            test.name
        ));
    }
    if test.timeout.unwrap_or(global_timeout) == 0 {
        return Err(format!("test {:?} has no usable timeout", test.name));
    }
    if test.requirements.iter().any(|item| item == &test.name) {
        return Err(format!("test {:?} cannot require itself", test.name));
    }
    let parameters = test.parameters.into_strings()?;
    if let Some(encoded) = parameters.get("filter_grep") {
        match parse_filter_grep(encoded)? {
            FilterGrep::Expressions(expressions) => {
                for expression in expressions {
                    validate_grep_filter(&test.name, &expression)?;
                }
            }
            FilterGrep::Labeled(expressions) => {
                for (label, expression) in expressions {
                    validate_grep_filter(&test.name, &expression)
                        .map_err(|error| format!("filter_grep label {label:?}: {error}"))?;
                }
            }
            FilterGrep::Items(items) => {
                for item in items {
                    match item {
                        FilterExpression::Raw(expression) => {
                            validate_grep_filter(&test.name, &expression)?;
                        }
                        FilterExpression::Labeled { label, expression } => {
                            validate_grep_filter(&test.name, &expression)
                                .map_err(|error| format!("filter_grep label {label:?}: {error}"))?;
                        }
                    }
                }
            }
        }
    }
    if let Some(encoded) = parameters.get("filter_jsonpath") {
        match parse_filter_jsonpath(encoded)? {
            FilterJsonPath::Expressions(expressions) => {
                for expression in expressions {
                    validate_jsonpath_filter(&test.name, &expression)?;
                }
            }
            FilterJsonPath::Labeled(expressions) => {
                for (label, expression) in expressions {
                    validate_jsonpath_filter(&test.name, &expression)
                        .map_err(|error| format!("filter_jsonpath label {label:?}: {error}"))?;
                }
            }
            FilterJsonPath::Items(items) => {
                for item in items {
                    match item {
                        FilterExpression::Raw(expression) => {
                            validate_jsonpath_filter(&test.name, &expression)?;
                        }
                        FilterExpression::Labeled { label, expression } => {
                            validate_jsonpath_filter(&test.name, &expression).map_err(|error| {
                                format!("filter_jsonpath label {label:?}: {error}")
                            })?;
                        }
                    }
                }
            }
        }
    }
    validate_test_reference(&test.test_ref)
        .map_err(|reason| format!("test {:?}: {reason}", test.name))
}

pub(crate) fn filter_expressions(encoded: &str) -> impl Iterator<Item = &str> {
    encoded.split('\0')
}

#[derive(Debug, Clone)]
pub(crate) enum FilterGrep {
    Expressions(Vec<String>),
    Labeled(BTreeMap<String, String>),
    Items(Vec<FilterExpression>),
}

#[derive(Debug, Clone)]
pub(crate) enum FilterExpression {
    Raw(String),
    Labeled { label: String, expression: String },
}

pub(crate) fn parse_filter_grep(encoded: &str) -> Result<FilterGrep, String> {
    if let Some(json) = encoded.strip_prefix(LABELED_GREP_FILTER_PREFIX) {
        let encoded: serde_json::Value = serde_json::from_str(json)
            .map_err(|error| format!("invalid labeled filter_grep mapping: {error}"))?;
        return match encoded {
            serde_json::Value::Object(values) => {
                let expressions = values
                    .into_iter()
                    .map(|(label, expression)| match expression {
                        serde_json::Value::String(expression) => Ok((label, expression)),
                        _ => Err(format!(
                            "filter_grep label {label:?} must map to a string expression"
                        )),
                    })
                    .collect::<Result<BTreeMap<_, _>, _>>()?;
                if expressions.is_empty() {
                    return Err("filter_grep mapping must not be empty".into());
                }
                Ok(FilterGrep::Labeled(expressions))
            }
            serde_json::Value::Array(values) => Ok(FilterGrep::Items(parse_filter_items(
                values,
                "filter_grep",
            )?)),
            _ => Err("labeled filter_grep must encode a mapping or filter list".into()),
        };
    }
    Ok(FilterGrep::Expressions(
        filter_expressions(encoded).map(str::to_owned).collect(),
    ))
}

fn validate_grep_filter(test_name: &str, expression: &str) -> Result<(), String> {
    if expression.trim().is_empty() {
        return Err(format!(
            "test {test_name:?} filter_grep expressions must not be empty"
        ));
    }
    Regex::new(expression).map_err(|error| {
        format!("test {test_name:?} has an invalid filter_grep expression {expression:?}: {error}")
    })?;
    Ok(())
}

#[derive(Debug, Clone)]
pub(crate) enum FilterJsonPath {
    Expressions(Vec<String>),
    Labeled(BTreeMap<String, String>),
    Items(Vec<FilterExpression>),
}

pub(crate) fn parse_filter_jsonpath(encoded: &str) -> Result<FilterJsonPath, String> {
    if let Some(json) = encoded.strip_prefix(LABELED_JSONPATH_FILTER_PREFIX) {
        let encoded: serde_json::Value = serde_json::from_str(json)
            .map_err(|error| format!("invalid labeled filter_jsonpath mapping: {error}"))?;
        return match encoded {
            serde_json::Value::Object(values) => {
                let expressions = values
                    .into_iter()
                    .map(|(label, expression)| match expression {
                        serde_json::Value::String(expression) => Ok((label, expression)),
                        _ => Err(format!(
                            "filter_jsonpath label {label:?} must map to a string expression"
                        )),
                    })
                    .collect::<Result<BTreeMap<_, _>, _>>()?;
                if expressions.is_empty() {
                    return Err("filter_jsonpath mapping must not be empty".into());
                }
                Ok(FilterJsonPath::Labeled(expressions))
            }
            serde_json::Value::Array(values) => Ok(FilterJsonPath::Items(parse_filter_items(
                values,
                "filter_jsonpath",
            )?)),
            _ => Err("labeled filter_jsonpath must encode a mapping or filter list".into()),
        };
    }
    Ok(FilterJsonPath::Expressions(
        filter_expressions(encoded).map(str::to_owned).collect(),
    ))
}

fn parse_filter_items(
    values: Vec<serde_json::Value>,
    parameter: &str,
) -> Result<Vec<FilterExpression>, String> {
    if values.is_empty() {
        return Err(format!("{parameter} filter list must not be empty"));
    }
    values
        .into_iter()
        .map(|value| match value {
            serde_json::Value::String(expression) => Ok(FilterExpression::Raw(expression)),
            serde_json::Value::Object(object) if object.len() == 1 => {
                let (label, expression) = object.into_iter().next().expect("one entry");
                match expression {
                    serde_json::Value::String(expression) => {
                        Ok(FilterExpression::Labeled { label, expression })
                    }
                    _ => Err(format!(
                        "{parameter} label {label:?} must map to a string expression"
                    )),
                }
            }
            _ => Err(format!(
                "{parameter} filter list entries must be strings or one-label mappings"
            )),
        })
        .collect()
}

fn validate_jsonpath_filter(test_name: &str, expression: &str) -> Result<(), String> {
    if expression.trim().is_empty() {
        return Err(format!(
            "test {test_name:?} filter_jsonpath expressions must not be empty"
        ));
    }
    JsonPath::parse(expression).map_err(|error| {
        format!(
            "test {test_name:?} has an invalid filter_jsonpath expression {expression:?}: {error}"
        )
    })?;
    Ok(())
}

fn validate_test_reference(reference: &str) -> Result<(), String> {
    if let Some(relative) = reference.strip_prefix("extern/") {
        let path = std::path::Path::new(relative);
        if relative.is_empty()
            || path.is_absolute()
            || path
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err(format!("unsafe external test path {reference:?}"));
        }
        return Ok(());
    }
    let Some((platform, test)) = reference.split_once('/') else {
        return Err(format!(
            "test reference {reference:?} must be platform/test or extern/path"
        ));
    };
    let valid_part = |part: &str| {
        !part.is_empty()
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    };
    if !valid_part(platform) || !valid_part(test) || test.contains('/') {
        return Err(format!("invalid built-in test reference {reference:?}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = "activity_name: The Linux Competition\ndelivery_folder: ./delivery\nconcurrency: 1\ntimeout: 120\ntests:\n  - name: import_ova\n    test: VirtualBox/import_ova\n    parameters:\n      - vm_name: linux_competition\n      - vbox_group: /linux_competition\n  - name: start_time\n    test: extern/tests/vm_start_time.py\n    requirements: [import_ova]\n    repeat: 3\n    parameters:\n      vm_name: linux_competition\nchecks:\n  - name: ova\n    description: has delivered an ova and it can be imported\n    check: pass\n    test: import_ova\n";

    #[test]
    fn parses_requested_project_and_parameter_formats() {
        let project = parse_configuration(EXAMPLE.as_bytes()).unwrap();
        assert_eq!(project.tests.len(), 2);
        assert_eq!(project.tests[1].repeat, 3);
        assert_eq!(
            project.tests[0].parameters.into_strings().unwrap()["vm_name"],
            "linux_competition"
        );
    }

    #[test]
    fn repository_example_configuration_is_valid() {
        let project = parse_configuration(include_bytes!(
            "../../../examples/vm-activity-poc/configuration.yaml"
        ))
        .unwrap();
        assert_eq!(project.activity_name, "The Linux Competition");
        assert_eq!(project.tests[1].repeat, 3);
    }

    #[test]
    fn rejects_external_path_traversal() {
        let bad = EXAMPLE.replace("extern/tests/vm_start_time.py", "extern/../secret.py");
        assert!(parse_configuration(bad.as_bytes()).is_err());
    }

    #[test]
    fn rejects_forward_requirements() {
        let bad = EXAMPLE.replace("[import_ova]", "[later]");
        assert!(parse_configuration(bad.as_bytes()).is_err());
    }
}
