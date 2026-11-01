//! Purpose: Runs an unprivileged Nmap TCP connect scan and reports open-port services.
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};
use thot_core::test_results::TestStatus;
use thot_runner::external::run_external_process;

use crate::test_execution::{TestInvocation, TestOutcome};

const NMAP_EXECUTABLE: &str = "nmap";

pub async fn run(invocation: &TestInvocation) -> TestOutcome {
    let parameters = &invocation.parameters;
    if let Err(error) = validate_parameters(parameters) {
        return TestOutcome::error(error);
    }
    let host = parameters.get("host").expect("host validated above");
    let mut arguments = vec![
        "-n".to_owned(),
        "-Pn".to_owned(),
        "-sT".to_owned(),
        "-sV".to_owned(),
        "--version-light".to_owned(),
        "--reason".to_owned(),
        "--max-retries".to_owned(),
        "1".to_owned(),
        "-oN".to_owned(),
        "-".to_owned(),
    ];
    if let Some(ports) = parameters.get("ports") {
        arguments.push("-p".into());
        arguments.push(ports.clone());
    } else {
        let top_ports = parameters
            .get("top_ports")
            .map(String::as_str)
            .unwrap_or("1000");
        arguments.push("--top-ports".into());
        arguments.push(top_ports.into());
    }
    arguments.push(host.clone());

    let working_directory = if invocation.project_root.as_os_str().is_empty() {
        Path::new(".")
    } else {
        &invocation.project_root
    };
    let output = match run_external_process(
        Path::new(NMAP_EXECUTABLE),
        &arguments,
        working_directory,
        &[],
        invocation.timeout,
        invocation.runtime.output_limit,
    )
    .await
    {
        Ok(output) => output,
        Err(error) if error.contains("No such file or directory") => {
            return TestOutcome::error(
                "network/nmap requires the nmap executable on the thotd PATH; install the nmap package on the server",
            );
        }
        Err(error) => return TestOutcome::error(format!("cannot run nmap: {error}")),
    };
    let open_ports = parse_open_ports(&output.stdout);
    let status = if output.timed_out {
        TestStatus::TimedOut
    } else if output.exit_code == Some(0) {
        TestStatus::Passed
    } else {
        TestStatus::Failed
    };
    let diagnostics = if output.timed_out {
        format!(
            "nmap scan exceeded the configured timeout of {:.3} seconds",
            invocation.timeout.as_secs_f64()
        )
    } else if output.stdout_truncated || output.stderr_truncated {
        "nmap output was truncated at the configured output limit".into()
    } else if output.exit_code != Some(0) {
        format!("nmap exited with {:?}", output.exit_code)
    } else {
        String::new()
    };
    let observation = json!({
        "host": host,
        "scan": "tcp_connect_service_detection",
        "ports_scanned": parameters.get("ports").cloned().unwrap_or_else(|| format!("top {} TCP ports", parameters.get("top_ports").map(String::as_str).unwrap_or("1000"))),
        "open_ports": open_ports,
        "duration_ms": output.elapsed.as_millis(),
        "output_truncated": output.stdout_truncated || output.stderr_truncated,
    });
    TestOutcome {
        status,
        stdout: output.stdout,
        stderr: output.stderr,
        exit_code: output.exit_code,
        diagnostics,
        observation,
        evidence: Vec::new(),
    }
}

pub(crate) fn validate_parameters(parameters: &BTreeMap<String, String>) -> Result<(), String> {
    let host = parameters
        .get("host")
        .map(String::as_str)
        .filter(|host| !host.trim().is_empty())
        .ok_or("network/nmap requires parameter host")?;
    if host.starts_with('-')
        || host.chars().any(|character| {
            character.is_whitespace()
                || matches!(character, '/' | ',' | ';' | '\\' | '\0' | '&' | '|')
        })
    {
        return Err("network/nmap host must be one hostname or IP address".into());
    }
    if let Some(ports) = parameters.get("ports") {
        validate_port_spec(ports)?;
    }
    if let Some(top_ports) = parameters.get("top_ports") {
        let parsed = top_ports
            .parse::<u16>()
            .map_err(|_| "network/nmap top_ports must be between 1 and 65535")?;
        if parsed == 0 {
            return Err("network/nmap top_ports must be between 1 and 65535".into());
        }
        if parameters.contains_key("ports") {
            return Err("network/nmap accepts ports or top_ports, not both".into());
        }
    }
    Ok(())
}

fn validate_port_spec(specification: &str) -> Result<(), String> {
    if specification.trim().is_empty() {
        return Err("network/nmap ports must list one or more TCP ports".into());
    }
    for part in specification.split(',') {
        let invalid = || {
            format!(
                "invalid network/nmap ports value {specification:?}; use comma-separated ports and ranges such as 22,80,8000-8100"
            )
        };
        let (start, end) = match part.split_once('-') {
            Some((start, end)) if !end.contains('-') => (
                start.parse::<u16>().map_err(|_| invalid())?,
                end.parse::<u16>().map_err(|_| invalid())?,
            ),
            Some(_) => return Err(invalid()),
            None => {
                let port = part.parse::<u16>().map_err(|_| invalid())?;
                (port, port)
            }
        };
        if start == 0 || end == 0 || start > end {
            return Err(invalid());
        }
    }
    Ok(())
}

fn parse_open_ports(output: &str) -> Vec<Value> {
    output
        .lines()
        .filter_map(|line| {
            let mut columns = line.split_whitespace();
            let endpoint = columns.next()?;
            let (port, protocol) = endpoint.split_once('/')?;
            let port = port.parse::<u16>().ok()?;
            let state = columns.next()?;
            if state != "open" {
                return None;
            }
            let service = columns.next().unwrap_or("unknown");
            let version = columns.collect::<Vec<_>>().join(" ");
            Some(json!({
                "port": port,
                "protocol": protocol,
                "state": state,
                "service": service,
                "version": if version.is_empty() { Value::Null } else { json!(version) },
            }))
        })
        .collect()
}
