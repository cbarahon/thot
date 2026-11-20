use crate::{
    AppState,
    config::{self, Config},
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use chrono::{SecondsFormat, Utc};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path as FsPath, PathBuf},
    process::Stdio,
    time::Instant,
};
use thot_core::{
    agent_protocol::{
        AGENT_PROTOCOL_VERSION, AgentCapabilities, AgentResult, AgentTask, AgentTaskStatus,
    },
    test_results::TestStatus,
};
use tokio::{io::AsyncRead, process::Command};

#[derive(Debug, Clone)]
pub struct TaskEntry {
    payload_hash: String,
    status: AgentTaskStatus,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

fn authorized(headers: &HeaderMap, config: &Config) -> bool {
    match config
        .auth_token
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        None => true,
        Some(token) => {
            headers
                .get(axum::http::header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                == Some(&format!("Bearer {token}"))
        }
    }
}

fn error(status: StatusCode, message: impl Into<String>) -> impl IntoResponse {
    (
        status,
        Json(ErrorResponse {
            error: message.into(),
        }),
    )
}

pub async fn health() -> Json<Value> {
    Json(json!({"status":"ok","protocol_version":AGENT_PROTOCOL_VERSION}))
}

pub async fn capabilities(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if !authorized(&headers, &state.config) {
        return error(StatusCode::UNAUTHORIZED, "invalid agent token").into_response();
    }
    let version = match run_remote_vbox(&state.config, &["--version".into()], 15).await {
        Ok(output) if output.exit_code == Some(0) => output.stdout.trim().to_owned(),
        Ok(output) => {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                format!("remote VBoxManage --version failed: {}", output.stderr),
            )
            .into_response();
        }
        Err(message) => return error(StatusCode::SERVICE_UNAVAILABLE, message).into_response(),
    };
    let version_number = version.split('r').next().unwrap_or_default();
    if version_number != state.config.ssh.expected_vbox_version {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            format!(
                "VirtualBox host version {version_number} differs from configured version {}",
                state.config.ssh.expected_vbox_version
            ),
        )
        .into_response();
    }
    let inventory = match run_remote_vbox(&state.config, &["list".into(), "vms".into()], 15).await {
        Ok(output) if output.exit_code == Some(0) => output,
        Ok(output) => {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                format!(
                    "cannot inspect dedicated VirtualBox registry: {}",
                    output.stderr
                ),
            )
            .into_response();
        }
        Err(message) => return error(StatusCode::SERVICE_UNAVAILABLE, message).into_response(),
    };
    if !inventory.stdout.trim().is_empty() {
        return error(StatusCode::SERVICE_UNAVAILABLE, "dedicated VirtualBox profile contains unaccounted VMs; resolve them before starting a run").into_response();
    }
    Json(AgentCapabilities {
        protocol_version: AGENT_PROTOCOL_VERSION,
        agent_id: "thot-virtualbox-agent".into(),
        agent_version: env!("CARGO_PKG_VERSION").into(),
        tests: vec![
            "VirtualBox/import_ova".into(),
            "VirtualBox/start_vm".into(),
            "VirtualBox/stop_vm".into(),
            "VirtualBox/destroy_vm".into(),
        ],
        host_version: Some(version),
        status: "ready".into(),
    })
    .into_response()
}

pub async fn submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(task): Json<AgentTask>,
) -> impl IntoResponse {
    if !authorized(&headers, &state.config) {
        return error(StatusCode::UNAUTHORIZED, "invalid agent token").into_response();
    }
    if task.protocol_version != AGENT_PROTOCOL_VERSION {
        return error(StatusCode::BAD_REQUEST, "unsupported protocol_version").into_response();
    }
    if task.task_id.is_empty() || task.task_id.len() > 128 {
        return error(StatusCode::BAD_REQUEST, "task_id is missing or too long").into_response();
    }
    if !matches!(
        task.test_reference.as_str(),
        "VirtualBox/import_ova"
            | "VirtualBox/start_vm"
            | "VirtualBox/stop_vm"
            | "VirtualBox/destroy_vm"
    ) {
        return error(StatusCode::BAD_REQUEST, "unsupported test reference").into_response();
    }
    if task.timeout_seconds == 0 || task.timeout_seconds > 86_400 {
        return error(
            StatusCode::BAD_REQUEST,
            "timeout_seconds must be between 1 and 86400",
        )
        .into_response();
    }
    let encoded = match serde_json::to_vec(&task) {
        Ok(value) => value,
        Err(e) => return error(StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    let hash = format!("{:x}", Sha256::digest(encoded));
    {
        let mut tasks = state.tasks.write().await;
        if let Some(existing) = tasks.get(&task.task_id) {
            if existing.payload_hash != hash {
                return error(
                    StatusCode::CONFLICT,
                    "task_id was already used with a different payload",
                )
                .into_response();
            }
            return (StatusCode::ACCEPTED, Json(existing.status.clone())).into_response();
        }
        tasks.insert(
            task.task_id.clone(),
            TaskEntry {
                payload_hash: hash,
                status: AgentTaskStatus {
                    task_id: task.task_id.clone(),
                    state: "queued".into(),
                    result: None,
                    error: None,
                },
            },
        );
    }
    let task_id = task.task_id.clone();
    let response_task_id = task_id.clone();
    let worker_state = state.clone();
    tokio::spawn(async move {
        {
            if let Some(entry) = worker_state.tasks.write().await.get_mut(&task_id) {
                entry.status.state = "running".into();
            }
        }
        let _guard = worker_state.vm_gate.lock().await;
        let result = execute(&worker_state.config, &task).await;
        if let Some(entry) = worker_state.tasks.write().await.get_mut(&task_id) {
            let cancellation_requested = entry.status.state == "cancellation_requested";
            match result {
                Ok(mut result) if cancellation_requested => {
                    result.status = TestStatus::Cancelled;
                    entry.status.state = "cancelled".into();
                    entry.status.result = Some(result);
                }
                Ok(result) => {
                    entry.status.state = "completed".into();
                    entry.status.result = Some(result);
                }
                Err(message) => {
                    entry.status.state = if cancellation_requested {
                        "cancelled"
                    } else {
                        "failed"
                    }
                    .into();
                    entry.status.error = Some(message);
                }
            }
        }
    });
    (
        StatusCode::ACCEPTED,
        Json(AgentTaskStatus {
            task_id: response_task_id,
            state: "queued".into(),
            result: None,
            error: None,
        }),
    )
        .into_response()
}

pub async fn task_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
) -> impl IntoResponse {
    if !authorized(&headers, &state.config) {
        return error(StatusCode::UNAUTHORIZED, "invalid agent token").into_response();
    }
    match state.tasks.read().await.get(&task_id) {
        Some(entry) => (StatusCode::OK, Json(entry.status.clone())).into_response(),
        None => error(StatusCode::NOT_FOUND, "unknown task_id").into_response(),
    }
}

pub async fn cancel_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
) -> impl IntoResponse {
    if !authorized(&headers, &state.config) {
        return error(StatusCode::UNAUTHORIZED, "invalid agent token").into_response();
    }
    let mut tasks = state.tasks.write().await;
    match tasks.get_mut(&task_id) {
        Some(entry) if matches!(entry.status.state.as_str(), "queued" | "running") => {
            entry.status.state = "cancellation_requested".into();
            (StatusCode::ACCEPTED, Json(entry.status.clone())).into_response()
        }
        Some(entry) => (StatusCode::OK, Json(entry.status.clone())).into_response(),
        None => error(StatusCode::NOT_FOUND, "unknown task_id").into_response(),
    }
}

async fn execute(config: &Config, task: &AgentTask) -> Result<AgentResult, String> {
    let started = Utc::now();
    let timer = Instant::now();
    let test_directory =
        config::path_is_within_after_create(&task.test_directory, &config.allowed_output_roots)?;
    tokio::fs::create_dir_all(&test_directory)
        .await
        .map_err(|e| format!("cannot create assigned test folder: {e}"))?;
    let test_directory = test_directory.canonicalize().map_err(|e| e.to_string())?;
    config::path_is_within(&test_directory, &config.allowed_output_roots)?;
    tokio::fs::create_dir_all(test_directory.join("artifacts"))
        .await
        .map_err(|e| format!("cannot create assigned artifacts folder: {e}"))?;
    let remote_deliverable = if task.test_reference == "VirtualBox/import_ova" {
        let source_metadata = std::fs::symlink_metadata(&task.deliverable_path)
            .map_err(|e| format!("deliverable is unavailable: {e}"))?;
        if source_metadata.file_type().is_symlink() || !source_metadata.is_file() {
            return Err("deliverable must be a regular non-symlink file".into());
        }
        let deliverable =
            config::path_is_within(&task.deliverable_path, &config.allowed_delivery_roots)?;
        map_remote_path(config, &deliverable)?
    } else {
        PathBuf::new()
    };
    let vm_storage = map_remote_path(config, &task.vm_storage_path)?;
    validate_vm_storage(config, &vm_storage)?;
    let (status, stdout, stderr, exit_code, diagnostics, observation, context, evidence) =
        match task.test_reference.as_str() {
            "VirtualBox/import_ova" => {
                import_ova(
                    config,
                    task,
                    &test_directory,
                    &remote_deliverable,
                    &vm_storage,
                )
                .await?
            }
            "VirtualBox/start_vm" => start_vm(config, task).await?,
            "VirtualBox/stop_vm" => stop_vm(config, task).await?,
            "VirtualBox/destroy_vm" => destroy_vm(config, task).await?,
            _ => unreachable!(),
        };
    let finished = Utc::now();
    Ok(AgentResult {
        protocol_version: AGENT_PROTOCOL_VERSION,
        task_id: task.task_id.clone(),
        status,
        stdout,
        stderr,
        exit_code,
        diagnostics,
        observation,
        context,
        evidence,
        started_at: started.to_rfc3339_opts(SecondsFormat::Millis, true),
        finished_at: finished.to_rfc3339_opts(SecondsFormat::Millis, true),
        duration_ms: timer.elapsed().as_millis(),
    })
}

async fn import_ova(
    config: &Config,
    task: &AgentTask,
    test_dir: &FsPath,
    deliverable: &FsPath,
    storage: &FsPath,
) -> Result<
    (
        TestStatus,
        String,
        String,
        Option<i32>,
        String,
        Value,
        Value,
        Vec<String>,
    ),
    String,
> {
    if let Some(reason) = task.parameters.get("_validation_error") {
        return Ok((
            TestStatus::Error,
            String::new(),
            reason.clone(),
            None,
            "OVA validation failed".into(),
            json!({}),
            task.context.clone(),
            vec![],
        ));
    }
    let name = task.parameters.get("vm_name").cloned().unwrap_or_else(|| {
        format!(
            "thot_{}",
            uuid::Uuid::new_v4().simple().to_string()[..12].to_owned()
        )
    });
    if !safe_name(&name) {
        return Err("vm_name contains unsupported characters".into());
    }
    let group = task
        .parameters
        .get("vbox_group")
        .map(String::as_str)
        .unwrap_or("/thot");
    if !group.starts_with('/') || group.contains(['\n', '\r', '"']) {
        return Err("vbox_group must be an absolute VirtualBox group path".into());
    }
    let pending_context = json!({"run_id":task.run_id,"submission_id":task.submission_id,"vm_name":name,"vm_uuid":null,"storage_path":storage,"may_be_registered":true});
    let pending_manifest = json!({"schema_version":1,"run_id":task.run_id,"submission_id":task.submission_id,"vm_name":name,"vm_uuid":null,"storage_path":storage,"may_be_registered":true});
    write_task_file(
        &test_dir.join("artifacts"),
        "cleanup-manifest.json",
        &pending_manifest,
    )
    .await?;
    let make_dir = run_ssh(
        config,
        task.timeout_seconds,
        &format!("mkdir -p {}", shell_quote(&storage.to_string_lossy())),
    )
    .await?;
    if make_dir.exit_code != Some(0) {
        let result = command_tuple(make_dir, "cannot create remote VM storage");
        return Ok(with_context(result, pending_context));
    }
    let import = match run_vbox(
        config,
        task,
        &[
            "import".into(),
            deliverable.to_string_lossy().into_owned(),
            "--vsys".into(),
            "0".into(),
            "--group".into(),
            group.into(),
            "--vmname".into(),
            name.clone(),
            "--basefolder".into(),
            storage.to_string_lossy().into_owned(),
        ],
    )
    .await
    {
        Ok(output) => output,
        Err(message) => return Ok(failure_with_context(message, pending_context.clone())),
    };
    if import.exit_code != Some(0) {
        let result = command_tuple(import, "VBoxManage import failed");
        return Ok(with_context(result, pending_context));
    }
    let info = match run_vbox(
        config,
        task,
        &[
            "showvminfo".into(),
            name.clone(),
            "--machinereadable".into(),
        ],
    )
    .await
    {
        Ok(output) => output,
        Err(message) => return Ok(failure_with_context(message, pending_context.clone())),
    };
    if info.exit_code != Some(0) {
        let result = command_tuple(info, "VM imported but its identity could not be inspected");
        return Ok(with_context(result, pending_context));
    }
    let fields = parse_info(&info.stdout);
    let Some(uuid) = fields.get("UUID").cloned() else {
        return Ok(failure_with_context(
            "VBoxManage did not report the imported VM UUID".into(),
            pending_context.clone(),
        ));
    };
    if !fields
        .get("CfgFile")
        .is_some_and(|path| PathBuf::from(path).starts_with(storage))
    {
        return Ok(failure_with_context(
            "imported VM settings path is outside this run's VM storage".into(),
            pending_context.clone(),
        ));
    }
    let context = json!({"run_id":task.run_id,"submission_id":task.submission_id,"vm_name":name,"vm_uuid":uuid,"storage_path":storage,"may_be_registered":true});
    let manifest = json!({"schema_version":1,"run_id":task.run_id,"submission_id":task.submission_id,"vm_name":name,"vm_uuid":uuid,"storage_path":storage,"may_be_registered":true});
    write_task_file(
        &test_dir.join("artifacts"),
        "cleanup-manifest.json",
        &manifest,
    )
    .await?;
    let mut stdout = info.stdout.clone();
    let mut stderr = info.stderr.clone();
    let isolation = match run_vbox(
        config,
        task,
        &[
            "modifyvm".into(),
            name.clone(),
            "--clipboard".into(),
            "disabled".into(),
            "--draganddrop".into(),
            "disabled".into(),
            "--usb".into(),
            "off".into(),
        ],
    )
    .await
    {
        Ok(output) => output,
        Err(message) => return Ok(failure_with_context(message, context.clone())),
    };
    stdout.push_str(&isolation.stdout);
    stderr.push_str(&isolation.stderr);
    if isolation.exit_code != Some(0) {
        let result = command_tuple(isolation, "could not apply VM isolation settings");
        return Ok(with_context(
            (
                result.0, stdout, stderr, result.3, result.4, result.5, result.6, result.7,
            ),
            context,
        ));
    }
    for (key, folder) in &fields {
        if (key.starts_with("SharedFolderNameMachineMapping")
            || key.starts_with("SharedFolderNameTransientMapping"))
            && !folder.is_empty()
        {
            let removed = match run_vbox(
                config,
                task,
                &[
                    "sharedfolder".into(),
                    "remove".into(),
                    name.clone(),
                    "--name".into(),
                    folder.clone(),
                ],
            )
            .await
            {
                Ok(output) => output,
                Err(message) => return Ok(failure_with_context(message, context.clone())),
            };
            stdout.push_str(&removed.stdout);
            stderr.push_str(&removed.stderr);
            if removed.exit_code != Some(0) {
                return Ok((
                    TestStatus::Failed,
                    stdout,
                    stderr,
                    removed.exit_code,
                    "could not remove imported shared folder".into(),
                    json!({"vm_name":name}),
                    context,
                    vec!["artifacts/cleanup-manifest.json".into()],
                ));
            }
        }
    }
    let verified = match run_vbox(
        config,
        task,
        &[
            "showvminfo".into(),
            name.clone(),
            "--machinereadable".into(),
        ],
    )
    .await
    {
        Ok(output) => output,
        Err(message) => return Ok(failure_with_context(message, context.clone())),
    };
    if verified.exit_code != Some(0) {
        let result = command_tuple(verified, "cannot verify VM isolation settings");
        return Ok(with_context(
            (
                result.0, stdout, stderr, result.3, result.4, result.5, result.6, result.7,
            ),
            context,
        ));
    }
    stdout.push_str(&verified.stdout);
    stderr.push_str(&verified.stderr);
    let verify_fields = parse_info(&verified.stdout);
    if verify_fields.get("clipboard").map(String::as_str) != Some("disabled")
        || verify_fields.get("draganddrop").map(String::as_str) != Some("disabled")
        || verify_fields.get("usb").map(String::as_str) != Some("off")
    {
        return Ok((
            TestStatus::Failed,
            stdout,
            stderr,
            Some(1),
            "VirtualBox isolation settings did not verify".into(),
            json!({"vm_name":name}),
            context,
            vec!["artifacts/cleanup-manifest.json".into()],
        ));
    }
    Ok((
        TestStatus::Passed,
        stdout,
        stderr,
        Some(0),
        String::new(),
        json!({"vm_name":name,"vm_uuid":uuid,"storage_path":storage}),
        context,
        vec!["artifacts/cleanup-manifest.json".into()],
    ))
}

fn failure_with_context(
    message: String,
    context: Value,
) -> (
    TestStatus,
    String,
    String,
    Option<i32>,
    String,
    Value,
    Value,
    Vec<String>,
) {
    (
        TestStatus::Error,
        String::new(),
        message.clone(),
        None,
        message,
        json!({}),
        context,
        vec!["artifacts/cleanup-manifest.json".into()],
    )
}

fn with_context(
    mut outcome: (
        TestStatus,
        String,
        String,
        Option<i32>,
        String,
        Value,
        Value,
        Vec<String>,
    ),
    context: Value,
) -> (
    TestStatus,
    String,
    String,
    Option<i32>,
    String,
    Value,
    Value,
    Vec<String>,
) {
    outcome.6 = context;
    outcome
}

async fn start_vm(
    config: &Config,
    task: &AgentTask,
) -> Result<
    (
        TestStatus,
        String,
        String,
        Option<i32>,
        String,
        Value,
        Value,
        Vec<String>,
    ),
    String,
> {
    let (name, _) = owned_vm(config, task)?;
    let result = run_vbox(
        config,
        task,
        &[
            "startvm".into(),
            name.clone(),
            "--type".into(),
            "headless".into(),
        ],
    )
    .await?;
    let out = command_tuple(result, "VBoxManage startvm");
    Ok((
        out.0,
        out.1,
        out.2,
        out.3,
        out.4,
        json!({"vm_name":name}),
        task.context.clone(),
        vec![],
    ))
}

async fn stop_vm(
    config: &Config,
    task: &AgentTask,
) -> Result<
    (
        TestStatus,
        String,
        String,
        Option<i32>,
        String,
        Value,
        Value,
        Vec<String>,
    ),
    String,
> {
    let (name, _) = owned_vm(config, task)?;
    let state = run_vbox(
        config,
        task,
        &[
            "showvminfo".into(),
            name.clone(),
            "--machinereadable".into(),
        ],
    )
    .await?;
    if state.exit_code != Some(0) {
        return Ok(command_tuple(state, "cannot inspect VM state"));
    }
    if parse_info(&state.stdout)
        .get("VMState")
        .is_some_and(|s| matches!(s.as_str(), "poweroff" | "aborted"))
    {
        return Ok((
            TestStatus::Passed,
            state.stdout,
            state.stderr,
            Some(0),
            String::new(),
            json!({"vm_name":name,"shutdown_method":"already_off"}),
            task.context.clone(),
            vec![],
        ));
    }
    let graceful = run_vbox(
        config,
        task,
        &["controlvm".into(), name.clone(), "acpipowerbutton".into()],
    )
    .await?;
    let mut stdout = graceful.stdout;
    let mut stderr = graceful.stderr;
    let mut method = "acpi";
    if graceful.exit_code != Some(0) || !wait_poweroff(config, task, &name, 30).await {
        let forced = run_vbox(
            config,
            task,
            &["controlvm".into(), name.clone(), "poweroff".into()],
        )
        .await?;
        stdout.push_str(&forced.stdout);
        stderr.push_str(&forced.stderr);
        if forced.exit_code != Some(0) || !wait_poweroff(config, task, &name, 10).await {
            return Ok((
                TestStatus::Failed,
                stdout,
                stderr,
                forced.exit_code,
                "graceful shutdown and forced poweroff failed".into(),
                json!({"vm_name":name}),
                task.context.clone(),
                vec![],
            ));
        }
        method = "forced_poweroff";
    }
    Ok((
        TestStatus::Passed,
        stdout,
        stderr,
        Some(0),
        String::new(),
        json!({"vm_name":name,"shutdown_method":method}),
        task.context.clone(),
        vec![],
    ))
}

async fn destroy_vm(
    config: &Config,
    task: &AgentTask,
) -> Result<
    (
        TestStatus,
        String,
        String,
        Option<i32>,
        String,
        Value,
        Value,
        Vec<String>,
    ),
    String,
> {
    let (name, expected_uuid, storage) = owned_vm_for_cleanup(config, task)?;
    let info = run_vbox(
        config,
        task,
        &[
            "showvminfo".into(),
            name.clone(),
            "--machinereadable".into(),
        ],
    )
    .await?;
    if info.exit_code != Some(0) {
        if info
            .stderr
            .to_ascii_lowercase()
            .contains("could not find a registered machine")
            || info.stderr.to_ascii_lowercase().contains("not found")
        {
            let storage_cleanup = remove_vm_storage(config, task, &storage).await?;
            let storage_removed = storage_cleanup.exit_code == Some(0);
            let cleanup = json!({
                "status": if storage_removed { "passed" } else { "failed" },
                "vm_unregistered": true,
                "run_storage_removed": storage_removed,
                "details": if storage_removed { Vec::<String>::new() } else { vec!["could not remove partial-import VM storage".to_owned()] }
            });
            write_task_file(
                &task.test_directory.join("artifacts"),
                "cleanup-result.yaml",
                &cleanup,
            )
            .await?;
            let context = json!({"cleaned":storage_removed,"run_id":task.run_id,"submission_id":task.submission_id,"vm_name":name,"vm_uuid":expected_uuid,"storage_path":storage});
            return Ok((
                if storage_removed {
                    TestStatus::Passed
                } else {
                    TestStatus::Failed
                },
                info.stdout,
                info.stderr,
                storage_cleanup.exit_code,
                if storage_removed {
                    String::new()
                } else {
                    "could not remove partial-import VM storage".into()
                },
                cleanup,
                context,
                vec!["artifacts/cleanup-result.yaml".into()],
            ));
        }
        return Ok(with_context(
            command_tuple(info, "cannot verify VM before cleanup"),
            task.context.clone(),
        ));
    }
    let fields = parse_info(&info.stdout);
    let uuid = fields
        .get("UUID")
        .cloned()
        .ok_or("registered VM has no UUID; refusing cleanup")?;
    if expected_uuid
        .as_deref()
        .is_some_and(|expected| expected != uuid)
        || !fields
            .get("CfgFile")
            .is_some_and(|path| PathBuf::from(path).starts_with(&storage))
    {
        return Err("VM identity or settings path does not match engine-owned run context".into());
    }
    let state = fields
        .get("VMState")
        .map(String::as_str)
        .unwrap_or_default();
    let mut stdout = info.stdout;
    let mut stderr = info.stderr;
    if !matches!(state, "poweroff" | "aborted") {
        let graceful = run_vbox(
            config,
            task,
            &["controlvm".into(), name.clone(), "acpipowerbutton".into()],
        )
        .await?;
        stdout.push_str(&graceful.stdout);
        stderr.push_str(&graceful.stderr);
        if graceful.exit_code != Some(0) || !wait_poweroff(config, task, &name, 30).await {
            let forced = run_vbox(
                config,
                task,
                &["controlvm".into(), name.clone(), "poweroff".into()],
            )
            .await?;
            stdout.push_str(&forced.stdout);
            stderr.push_str(&forced.stderr);
            if forced.exit_code != Some(0) || !wait_poweroff(config, task, &name, 10).await {
                return Ok((
                    TestStatus::Failed,
                    stdout,
                    stderr,
                    forced.exit_code,
                    "graceful shutdown and forced poweroff failed".into(),
                    json!({"status":"failed","vm_unregistered":false,"run_storage_removed":false,"details":["VM could not be powered off"]}),
                    task.context.clone(),
                    vec![],
                ));
            }
        }
    }
    let remove = run_vbox(
        config,
        task,
        &["unregistervm".into(), name.clone(), "--delete".into()],
    )
    .await?;
    stdout.push_str(&remove.stdout);
    stderr.push_str(&remove.stderr);
    let storage_cleanup = if remove.exit_code == Some(0) {
        Some(remove_vm_storage(config, task, &storage).await?)
    } else {
        None
    };
    if let Some(storage_cleanup) = &storage_cleanup {
        stdout.push_str(&storage_cleanup.stdout);
        stderr.push_str(&storage_cleanup.stderr);
    }
    let vm_unregistered = remove.exit_code == Some(0);
    let storage_removed = storage_cleanup
        .as_ref()
        .is_some_and(|output| output.exit_code == Some(0));
    let passed = vm_unregistered && storage_removed;
    let cleanup = json!({
        "status": if passed { "passed" } else { "failed" },
        "vm_unregistered": vm_unregistered,
        "run_storage_removed": storage_removed,
        "details": if passed { Vec::<String>::new() } else { vec!["manifest-scoped VM cleanup failed".to_owned()] }
    });
    write_task_file(
        &task.test_directory.join("artifacts"),
        "cleanup-result.yaml",
        &cleanup,
    )
    .await?;
    let result_context = json!({"cleaned":passed,"run_id":task.run_id,"submission_id":task.submission_id,"vm_name":name,"vm_uuid":uuid,"storage_path":storage});
    Ok((
        if passed {
            TestStatus::Passed
        } else {
            TestStatus::Failed
        },
        stdout,
        stderr,
        if passed {
            Some(0)
        } else {
            storage_cleanup
                .as_ref()
                .and_then(|output| output.exit_code)
                .or(remove.exit_code)
        },
        if passed {
            String::new()
        } else {
            "manifest-scoped VM cleanup failed".into()
        },
        cleanup,
        result_context,
        vec!["artifacts/cleanup-result.yaml".into()],
    ))
}

fn owned_vm_for_cleanup(
    config: &Config,
    task: &AgentTask,
) -> Result<(String, Option<String>, PathBuf), String> {
    let name = task
        .context
        .get("vm_name")
        .and_then(Value::as_str)
        .ok_or("cleanup task has no engine-owned vm_name")?;
    let storage = task
        .context
        .get("storage_path")
        .and_then(Value::as_str)
        .ok_or("cleanup task has no engine-owned storage_path")?;
    let storage = PathBuf::from(storage);
    validate_vm_storage(config, &storage)?;
    if !task.vm_storage_path.as_os_str().is_empty() {
        let expected_storage = map_remote_path(config, &task.vm_storage_path)?;
        if storage != expected_storage {
            return Err("cleanup storage path does not match the engine-owned task path".into());
        }
    }
    if !safe_name(name)
        || task.context.get("run_id").and_then(Value::as_str) != Some(task.run_id.as_str())
        || task.context.get("submission_id").and_then(Value::as_str)
            != Some(task.submission_id.as_str())
    {
        return Err("cleanup context does not match this run and submission".into());
    }
    let uuid = task
        .context
        .get("vm_uuid")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Ok((name.to_owned(), uuid, storage))
}

fn owned_vm(config: &Config, task: &AgentTask) -> Result<(String, String), String> {
    let name = task
        .context
        .get("vm_name")
        .and_then(Value::as_str)
        .ok_or("VM task is missing engine-owned vm_name context")?;
    let uuid = task
        .context
        .get("vm_uuid")
        .and_then(Value::as_str)
        .ok_or("VM task is missing engine-owned vm_uuid context")?;
    let storage = task
        .context
        .get("storage_path")
        .and_then(Value::as_str)
        .ok_or("VM task is missing engine-owned storage_path context")?;
    let expected_storage = map_remote_path(config, &task.vm_storage_path)?;
    if !safe_name(name)
        || uuid.is_empty()
        || PathBuf::from(storage) != expected_storage
        || task.context.get("run_id").and_then(Value::as_str) != Some(task.run_id.as_str())
        || task.context.get("submission_id").and_then(Value::as_str)
            != Some(task.submission_id.as_str())
    {
        return Err("VM context does not match this run and submission".into());
    }
    Ok((name.into(), uuid.into()))
}
fn safe_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}
fn parse_info(output: &str) -> BTreeMap<String, String> {
    output
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            Some((key.into(), value.trim_matches('"').into()))
        })
        .collect()
}
fn command_tuple(
    out: SshOutput,
    message: &str,
) -> (
    TestStatus,
    String,
    String,
    Option<i32>,
    String,
    Value,
    Value,
    Vec<String>,
) {
    let status = if out.timed_out {
        TestStatus::TimedOut
    } else if out.exit_code == Some(0) {
        TestStatus::Passed
    } else {
        TestStatus::Failed
    };
    let diagnostic = if status == TestStatus::Passed {
        String::new()
    } else {
        format!("{message}: exit code {:?}", out.exit_code)
    };
    (
        status,
        out.stdout,
        out.stderr,
        out.exit_code,
        diagnostic,
        json!({}),
        json!({}),
        vec![],
    )
}

async fn wait_poweroff(config: &Config, task: &AgentTask, name: &str, seconds: u64) -> bool {
    let deadline = tokio::time::Instant::now()
        + std::time::Duration::from_secs(seconds.min(task.timeout_seconds));
    while tokio::time::Instant::now() < deadline {
        if let Ok(output) = run_vbox(
            config,
            task,
            &["showvminfo".into(), name.into(), "--machinereadable".into()],
        )
        .await
        {
            if output.exit_code == Some(0)
                && parse_info(&output.stdout)
                    .get("VMState")
                    .is_some_and(|s| matches!(s.as_str(), "poweroff" | "aborted"))
            {
                return true;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    false
}

fn validate_vm_storage(config: &Config, path: &FsPath) -> Result<(), String> {
    if !path.is_absolute()
        || path.components().any(|part| {
            !matches!(
                part,
                std::path::Component::RootDir | std::path::Component::Normal(_)
            )
        })
    {
        return Err("remote VM storage path must be absolute and normalized".into());
    }
    if config
        .allowed_vm_storage_roots
        .iter()
        .map(|root| config::expand_home(root))
        .any(|root| path != root && path.starts_with(root))
    {
        Ok(())
    } else {
        Err(format!(
            "remote VM storage path {} is outside configured allowed_vm_storage_roots",
            path.display()
        ))
    }
}

async fn remove_vm_storage(
    config: &Config,
    task: &AgentTask,
    storage: &FsPath,
) -> Result<SshOutput, String> {
    validate_vm_storage(config, storage)?;
    let quoted = shell_quote(&storage.to_string_lossy());
    run_ssh(
        config,
        task.timeout_seconds,
        &format!("rm -rf -- {quoted} && test ! -e {quoted}"),
    )
    .await
}

fn map_remote_path(config: &Config, path: &FsPath) -> Result<PathBuf, String> {
    for mapping in &config.path_mappings {
        let local = config::expand_home(&mapping.local)
            .canonicalize()
            .map_err(|e| {
                format!(
                    "local mapped root {} is unavailable: {e}",
                    mapping.local.display()
                )
            })?;
        if let Ok(relative) = path.strip_prefix(&local) {
            return Ok(config::expand_home(&mapping.remote).join(relative));
        }
    }
    Ok(path.to_path_buf())
}

async fn run_vbox(config: &Config, task: &AgentTask, args: &[String]) -> Result<SshOutput, String> {
    let remote_args =
        std::iter::once(config.ssh.vboxmanage.clone()).chain(args.iter().map(|arg| {
            if FsPath::new(arg).is_absolute() {
                map_remote_path(config, FsPath::new(arg))
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| arg.clone())
            } else {
                arg.clone()
            }
        }));
    run_ssh(
        config,
        task.timeout_seconds,
        &vbox_remote_command(config, remote_args),
    )
    .await
}

async fn run_remote_vbox(
    config: &Config,
    args: &[String],
    timeout_seconds: u64,
) -> Result<SshOutput, String> {
    let remote_args = std::iter::once(config.ssh.vboxmanage.clone()).chain(args.iter().cloned());
    run_ssh(
        config,
        timeout_seconds,
        &vbox_remote_command(config, remote_args),
    )
    .await
}

fn vbox_remote_command(config: &Config, args: impl IntoIterator<Item = String>) -> String {
    let args = args
        .into_iter()
        .map(|arg| shell_quote(&arg))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "VBOX_USER_HOME={} {args}",
        shell_quote(&config.ssh.vbox_user_home.to_string_lossy())
    )
}

#[derive(Debug)]
struct SshOutput {
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    timed_out: bool,
}
async fn run_ssh(
    config: &Config,
    timeout_seconds: u64,
    remote_command: &str,
) -> Result<SshOutput, String> {
    let key = config::expand_home(&config.ssh.private_key);
    let known_hosts = config
        .ssh
        .known_hosts
        .as_ref()
        .map(|path| config::expand_home(path));
    let mut command = Command::new("ssh");
    command
        .args([
            "-T",
            "-o",
            "BatchMode=yes",
            "-o",
            "IdentitiesOnly=yes",
            "-o",
            "StrictHostKeyChecking=yes",
        ])
        .arg("-o")
        .arg(format!(
            "ConnectTimeout={}",
            config.ssh.connect_timeout_seconds
        ))
        .arg("-i")
        .arg(key)
        .arg("-p")
        .arg(config.ssh.port.to_string());
    if let Some(path) = known_hosts {
        command
            .arg("-o")
            .arg(format!("UserKnownHostsFile={}", path.display()));
    }
    command
        .arg(format!("{}@{}", config.ssh.user, config.ssh.host))
        .arg("--")
        .arg(remote_command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|e| format!("cannot start SSH: {e}"))?;
    let stdout = child.stdout.take().ok_or("SSH stdout unavailable")?;
    let stderr = child.stderr.take().ok_or("SSH stderr unavailable")?;
    let limit = config.output_limit;
    let result = tokio::time::timeout(std::time::Duration::from_secs(timeout_seconds), async {
        let out = read_bounded(stdout, limit);
        let err = read_bounded(stderr, limit);
        let wait = child.wait();
        tokio::join!(out, err, wait)
    })
    .await;
    match result {
        Ok((stdout, stderr, status)) => Ok(SshOutput {
            exit_code: status.map_err(|e| e.to_string())?.code(),
            stdout,
            stderr,
            timed_out: false,
        }),
        Err(_) => Ok(SshOutput {
            exit_code: None,
            stdout: String::new(),
            stderr: format!("SSH operation timed out after {timeout_seconds} seconds"),
            timed_out: true,
        }),
    }
}
async fn read_bounded<R: AsyncRead + Unpin>(mut reader: R, limit: usize) -> String {
    use tokio::io::AsyncReadExt;
    let mut bytes = Vec::with_capacity(limit.min(8192));
    let mut buffer = [0u8; 8192];
    let mut truncated = false;
    loop {
        match reader.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let keep = limit.saturating_sub(bytes.len()).min(n);
                bytes.extend_from_slice(&buffer[..keep]);
                truncated |= keep < n;
            }
        }
    }
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    if truncated {
        text.push_str("\n[output truncated]");
    }
    text
}
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
async fn write_task_file(directory: &FsPath, name: &str, value: &Value) -> Result<(), String> {
    let destination = directory.join(name);
    let temp = directory.join(format!(".{name}.{}.tmp", std::process::id()));
    let bytes = match FsPath::new(name).extension().and_then(|ext| ext.to_str()) {
        Some("yaml" | "yml") => serde_yaml::to_string(value)
            .map_err(|e| e.to_string())?
            .into_bytes(),
        _ => serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?,
    };
    tokio::fs::write(&temp, bytes)
        .await
        .map_err(|e| e.to_string())?;
    tokio::fs::rename(&temp, &destination)
        .await
        .map_err(|e| e.to_string())
}
