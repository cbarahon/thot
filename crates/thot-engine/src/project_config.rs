//! Purpose: Parses and validates the root configuration.yaml project test model.
use std::collections::HashSet;
use thot_core::{ProjectTest, VMActivityProject};

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
        test.parameters.into_strings()?;
        prior_names.insert(test.name.as_str());
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

    const EXAMPLE: &str = "activity_name: The Linux Competition\ndelivery_folder: ./delivery\nconcurrency: 1\ntimeout: 120\ntests:\n  - name: import_ova\n    test: VirtualBox/import_ova\n    parameters:\n      - vm_name: linux_competition\n      - vbox_group: /linux_competition\n  - name: start_time\n    test: extern/tests/vm_start_time.py\n    requirements: [import_ova]\n    repeat: 3\n    parameters:\n      vm_name: linux_competition\n";

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
