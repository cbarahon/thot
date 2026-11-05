//! Purpose: Parses and validates the root configuration.yaml project test model.
use regex::Regex;
use serde_json_path::JsonPath;
use std::collections::HashSet;
use thot_core::{ProjectCheck, ProjectRubric, ProjectTest, VMActivityProject};

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
    if project.tests.is_empty() {
        return Err("tests must contain at least one test".into());
    }

    let mut names = HashSet::new();
    let mut prior_names = HashSet::new();
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
        prior_names.insert(test.name.as_str());
    }
    let mut check_names = HashSet::new();
    for check in &project.checks {
        validate_check(check, &project.tests, &names)?;
        if !check_names.insert(check.name.as_str()) {
            return Err(format!("check name {:?} is repeated", check.name));
        }
    }
    let mut rubric_names = HashSet::new();
    for rubric in &project.rubrics {
        validate_rubric(rubric, &check_names)?;
        if !rubric_names.insert(rubric.name.as_str()) {
            return Err(format!("rubric name {:?} is repeated", rubric.name));
        }
    }
    Ok(())
}

fn validate_rubric(rubric: &ProjectRubric, check_names: &HashSet<&str>) -> Result<(), String> {
    if rubric.name.trim().is_empty()
        || !rubric
            .name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(format!("invalid rubric name {:?}", rubric.name));
    }
    if rubric.checks.is_empty() {
        return Err(format!(
            "rubric {:?} must reference at least one check",
            rubric.name
        ));
    }
    for check in &rubric.checks {
        if !check_names.contains(check.as_str()) {
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
    let parameters = check.parameters.into_strings()?;
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
    let valid_condition = matches!(condition.as_str(), "<" | "<=" | ">" | ">=" | "==" | "!=")
        || (condition == "contains" && data_type == "text");
    if !valid_condition {
        return Err(format!(
            "check {:?} condition must be <, <=, >, >=, ==, !=, or contains for text",
            check.name
        ));
    }
    Ok(())
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
    let data_type = parameters
        .get("data_type")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            format!(
                "check {:?} using {check_type} requires parameter data_type",
                check.name
            )
        })?;
    if !matches!(data_type.as_str(), "number" | "text" | "date" | "datetime") {
        return Err(format!(
            "check {:?} data_type must be number, text, date, or datetime",
            check.name
        ));
    }
    let condition = parameters
        .get("condition")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            format!(
                "check {:?} using {check_type} requires parameter condition",
                check.name
            )
        })?;
    let valid_condition = matches!(condition.as_str(), "<" | "<=" | ">" | ">=" | "==" | "!=")
        || (condition == "contains" && data_type == "text");
    if !valid_condition {
        return Err(format!(
            "check {:?} condition must be <, <=, >, >=, ==, !=, or contains for text",
            check.name
        ));
    }
    let value = parameters
        .get("value")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            format!(
                "check {:?} using {check_type} requires parameter value",
                check.name
            )
        })?;
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
    validate_test_reference(&test.test_ref)
        .map_err(|reason| format!("test {:?}: {reason}", test.name))
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
