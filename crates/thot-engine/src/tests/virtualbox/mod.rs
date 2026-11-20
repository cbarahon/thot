//! Purpose: Sends VirtualBox test tasks to the independent thot-virtualbox-agent service.
use crate::{
    test_execution::{TestInvocation, TestOutcome},
    vm_activity::DeliveryInput,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
    time::Duration,
};
use thot_core::{
    agent_protocol::{
        AGENT_PROTOCOL_VERSION, AgentCapabilities, AgentResult, AgentTask, AgentTaskStatus,
    },
    test_results::TestStatus,
};

fn endpoint() -> String {
    std::env::var("THOT_VBOX_AGENT_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8090".into())
        .trim_end_matches('/')
        .to_owned()
}
fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())
}
fn add_auth(request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    match std::env::var("THOT_VBOX_AGENT_TOKEN") {
        Ok(token) if !token.is_empty() => request.bearer_auth(token),
        _ => request,
    }
}

pub async fn preflight() -> Result<AgentCapabilities, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(35))
        .build()
        .map_err(|e| e.to_string())?;
    let response = add_auth(client.get(format!("{}/v1/capabilities", endpoint())))
        .send()
        .await
        .map_err(|e| format!("VirtualBox agent is unavailable: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "VirtualBox agent capability check failed: HTTP {}",
            response.status()
        ));
    }
    let capabilities: AgentCapabilities = response
        .json()
        .await
        .map_err(|e| format!("invalid VirtualBox agent capability response: {e}"))?;
    if capabilities.protocol_version != AGENT_PROTOCOL_VERSION {
        return Err("VirtualBox agent uses an unsupported protocol version".into());
    }
    Ok(capabilities)
}

pub async fn run(invocation: &TestInvocation) -> TestOutcome {
    submit(
        invocation,
        invocation.delivery.validation_error.as_deref(),
        invocation.agent_context.clone(),
    )
    .await
}

async fn submit(
    invocation: &TestInvocation,
    validation_error: Option<&str>,
    context: Value,
) -> TestOutcome {
    let vm_path = invocation
        .parameters
        .get("vm_path")
        .map(String::as_str)
        .unwrap_or("./thot_vms");
    let relative = Path::new(vm_path);
    if relative.is_absolute()
        || relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_) | Component::CurDir))
    {
        return TestOutcome::error("vm_path must be a safe relative path below the project folder");
    }
    let vm_storage_path = invocation
        .runtime
        .storage_root
        .join(relative)
        .join(&invocation.run_id)
        .join(&invocation.delivery.delivery_id);
    let mut parameters = invocation.parameters.clone();
    if let Some(reason) = validation_error {
        parameters.insert("_validation_error".into(), reason.into());
    }
    let test_directory = invocation
        .artifacts_dir
        .parent()
        .unwrap_or(&invocation.artifacts_dir)
        .to_path_buf();
    let task = AgentTask {
        protocol_version: AGENT_PROTOCOL_VERSION,
        task_id: uuid::Uuid::new_v4().simple().to_string(),
        test_reference: invocation.test_reference.clone(),
        test_name: invocation.test_name.clone(),
        parameters,
        timeout_seconds: invocation.timeout.as_secs().max(1),
        run_id: invocation.run_id.clone(),
        submission_id: invocation.delivery.delivery_id.clone(),
        deliverable_path: invocation.delivery.source.clone(),
        test_directory,
        vm_storage_path,
        context,
    };
    match submit_and_wait(task).await {
        Ok(result) => outcome_from_agent(result),
        Err(error) => TestOutcome::error(error),
    }
}

async fn submit_and_wait(task: AgentTask) -> Result<AgentResult, String> {
    let client = client()?;
    let base = endpoint();
    let response = add_auth(client.post(format!("{base}/v1/tasks")))
        .json(&task)
        .send()
        .await
        .map_err(|e| format!("could not submit task to VirtualBox agent: {e}"))?;
    if response.status() != reqwest::StatusCode::ACCEPTED {
        return Err(format!(
            "VirtualBox agent rejected task: HTTP {}",
            response.status()
        ));
    }
    let status: AgentTaskStatus = response
        .json()
        .await
        .map_err(|e| format!("invalid VirtualBox agent task response: {e}"))?;
    if status.task_id != task.task_id {
        return Err("VirtualBox agent returned a different task_id".into());
    }
    let deadline =
        tokio::time::Instant::now() + Duration::from_secs(task.timeout_seconds.saturating_add(60));
    loop {
        if tokio::time::Instant::now() >= deadline {
            let _ = add_auth(client.delete(format!("{base}/v1/tasks/{}", task.task_id)))
                .send()
                .await;
            return Err(format!(
                "VirtualBox agent task {} exceeded its deadline",
                task.task_id
            ));
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
        let response = add_auth(client.get(format!("{base}/v1/tasks/{}", task.task_id)))
            .send()
            .await
            .map_err(|e| format!("could not read VirtualBox agent task status: {e}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "VirtualBox agent task status failed: HTTP {}",
                response.status()
            ));
        }
        let status: AgentTaskStatus = response
            .json()
            .await
            .map_err(|e| format!("invalid VirtualBox agent task status: {e}"))?;
        match status.state.as_str() {
            "completed" => {
                return status
                    .result
                    .ok_or_else(|| "VirtualBox agent completed without a result".into());
            }
            "failed" => {
                return Err(status
                    .error
                    .unwrap_or_else(|| "VirtualBox agent task failed".into()));
            }
            "cancelled" => return Err("VirtualBox agent task was cancelled".into()),
            "queued" | "running" | "cancellation_requested" => {}
            other => {
                return Err(format!(
                    "VirtualBox agent returned unknown task state {other:?}"
                ));
            }
        }
    }
}

fn outcome_from_agent(result: AgentResult) -> TestOutcome {
    if result.protocol_version != AGENT_PROTOCOL_VERSION {
        return TestOutcome::error("VirtualBox agent result has an unsupported protocol version");
    }
    let mut observation = result.observation;
    observation["_agent_task_id"] = json!(result.task_id);
    observation["_agent_protocol_version"] = json!(result.protocol_version);
    if !result.context.is_null() {
        observation["_agent_context"] = result.context;
    }
    TestOutcome {
        status: result.status,
        stdout: result.stdout,
        stderr: result.stderr,
        exit_code: result.exit_code,
        diagnostics: result.diagnostics,
        observation,
        evidence: result.evidence,
    }
}

pub async fn cleanup(
    run_id: &str,
    input: &DeliveryInput,
    report_dir: &Path,
    context: Value,
    timeout: Duration,
) -> Value {
    if context.get("vm_name").is_none()
        || context.get("cleaned").and_then(Value::as_bool) == Some(true)
    {
        return json!({"status":"passed","vm_unregistered":true,"run_storage_removed":true,"details":["no registered VirtualBox VM remains"]});
    }
    let storage = context
        .get("storage_path")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_default();
    let task = AgentTask {
        protocol_version: AGENT_PROTOCOL_VERSION,
        task_id: uuid::Uuid::new_v4().simple().to_string(),
        test_reference: "VirtualBox/destroy_vm".into(),
        test_name: "cleanup".into(),
        parameters: BTreeMap::new(),
        timeout_seconds: timeout.as_secs().max(1),
        run_id: run_id.into(),
        submission_id: input.delivery_id.clone(),
        deliverable_path: input.source.clone(),
        test_directory: report_dir
            .join("tests")
            .join(&input.delivery_id)
            .join("cleanup"),
        vm_storage_path: storage,
        context,
    };
    match submit_and_wait(task).await {
        Ok(result) => {
            let mut cleanup = result.observation;
            cleanup["status"] = json!(if result.status == TestStatus::Passed {
                "passed"
            } else {
                "failed"
            });
            if !result.diagnostics.is_empty() {
                cleanup["details"] = json!([result.diagnostics]);
            }
            cleanup
        }
        Err(error) => {
            json!({"status":"failed","vm_unregistered":false,"run_storage_removed":false,"details":[error]})
        }
    }
}
