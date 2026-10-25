use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Serialize)]
struct Config {
    concurrency: u8,
    global_timeout: u32,
    tests: Vec<Test>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Test {
    name: String,
    repeat: u8,
    timeout: u32,
    references: Vec<String>,
    ssh_params: Option<SshParams>,
}

#[derive(Debug, Deserialize, Serialize)]
struct SshParams {
    pass: Option<String>,
}

fn parse_and_validate_config(config_yaml: &str) -> Result<Config, String> {
    let config: Config = serde_yaml::from_str(config_yaml).map_err(|e| format!("Failed to parse YAML: {}", e))?;

    if config.concurrency < 1 || config.concurrency > 64 {
        return Err("Concurrency must be between 1 and 64".to_string());
    }

    if config.global_timeout < 1 || config.global_timeout > 86400 {
        return Err("Global timeout must be between 1 and 86400 seconds".to_string());
    }

    let mut test_names = HashSet::new();
    let mut external_references = HashSet::new();

    for test in &config.tests {
        if !test_names.insert(test.name.clone()) {
            return Err(format!("Duplicate test name: {}", test.name));
        }

        if test.timeout < 1 || test.timeout > 86400 {
            return Err(format!("Test '{}' timeout must be between 1 and 86400 seconds", test.name));
        }

        if test.repeat < 1 || test.repeat > 100 {
            return Err(format!("Test '{}' repeat must be between 1 and 100", test.name));
        }

        for ref_path in &test.references {
            if ref_path.starts_with("extern/") {
                if !external_references.insert(ref_path.clone()) {
                    return Err(format!("Duplicate external reference: {}", ref_path));
                }
                if ref_path.contains("..") {
                    return Err(format!("External reference '{}' contains parent traversal", ref_path));
                }
            } else {
                return Err(format!("Invalid reference format: {}", ref_path));
            }
        }

        if let Some(ssh_params) = &test.ssh_params {
            if let Some(pass) = &ssh_params.pass {
                if pass.is_empty() {
                    return Err(format!("SSH pass parameter cannot be empty for test: {}", test.name));
                }
            }
        }
    }

    Ok(config)
}
