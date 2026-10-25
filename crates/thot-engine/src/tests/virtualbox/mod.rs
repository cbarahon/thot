//! Purpose: Dispatches VirtualBox tests and provides shared VM ownership and command helpers.
mod destroy_vm;
mod import_ova;
mod start_vm;
mod stop_vm;

use std::{
    path::{Component, Path, PathBuf},
    time::Duration,
};
use thot_runner::vbox::LocalVirtualBoxRunner;

use crate::{
    test_execution::{TestInvocation, TestOutcome},
    vm_activity::atomic_write,
};

pub async fn run(invocation: &TestInvocation) -> TestOutcome {
    match invocation.test_reference.as_str() {
        "VirtualBox/import_ova" => import_ova::run(invocation).await,
        "VirtualBox/start_vm" => start_vm::run(invocation).await,
        "VirtualBox/stop_vm" => stop_vm::run(invocation).await,
        "VirtualBox/destroy_vm" => destroy_vm::run(invocation).await,
        _ => TestOutcome::error(format!(
            "unknown VirtualBox test {:?}",
            invocation.test_reference
        )),
    }
}

pub(super) fn runner(invocation: &TestInvocation) -> LocalVirtualBoxRunner {
    LocalVirtualBoxRunner::new(
        invocation.runtime.vbox_manage.clone(),
        invocation.runtime.vbox_user_home.clone(),
        invocation.timeout,
        invocation.runtime.storage_root.clone(),
        thot_runner::vbox::OutputLimits {
            stdout_bytes: invocation.runtime.output_limit,
            stderr_bytes: invocation.runtime.output_limit,
        },
    )
}

pub(super) fn vm_name(invocation: &TestInvocation) -> Result<String, String> {
    invocation
        .parameters
        .get("vm_name")
        .cloned()
        .ok_or_else(|| {
            "vm_name is unavailable; import_ova must pass its generated name to later tests".into()
        })
}

pub(super) fn vm_storage(invocation: &TestInvocation) -> Result<PathBuf, String> {
    let relative = invocation
        .parameters
        .get("vm_path")
        .map(String::as_str)
        .unwrap_or("./thot_vms");
    let relative = Path::new(relative);
    if relative.is_absolute()
        || relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_) | Component::CurDir))
    {
        return Err(
            "vm_path must be a safe relative path below the project archive directory".into(),
        );
    }
    let base = invocation
        .project_archive_parent
        .canonicalize()
        .map_err(|e| format!("project archive directory is unavailable: {e}"))?;
    let root = base.join(relative);
    let run_storage = root
        .join(&invocation.run_id)
        .join(&invocation.delivery.delivery_id);
    let mut current = root.as_path();
    while current.starts_with(&base) {
        if std::fs::symlink_metadata(current)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err("vm_path must not traverse a symlink".into());
        }
        if current == base {
            break;
        }
        current = current.parent().ok_or("vm_path has an invalid parent")?;
    }
    if paths_overlap(&run_storage, &invocation.runtime.artifact_root)
        || paths_overlap(&run_storage, &invocation.runtime.vbox_user_home)
    {
        return Err("vm_path overlaps VirtualBox profile or artifact storage".into());
    }
    Ok(run_storage)
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    left.starts_with(right) || right.starts_with(left)
}

pub(super) fn manifest_path(invocation: &TestInvocation) -> PathBuf {
    invocation.report_dir.join("cleanup-manifest.json")
}

pub(super) fn require_owned_vm(invocation: &TestInvocation, name: &str) -> Result<(), String> {
    let bytes = std::fs::read(manifest_path(invocation)).map_err(|_| {
        "VM operation requires a manifest created by this delivery's import_ova test".to_owned()
    })?;
    let manifest: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|e| format!("invalid delivery cleanup manifest: {e}"))?;
    if manifest["delivery_id"].as_str() != Some(invocation.delivery.delivery_id.as_str())
        || manifest["run_id"].as_str() != Some(invocation.run_id.as_str())
        || manifest["vm_name"].as_str() != Some(name)
        || manifest["vm_uuid"].as_str().is_none()
        || manifest["may_be_registered"] != true
    {
        return Err("VM identity is not owned by this delivery's manifest".into());
    }
    Ok(())
}

pub(super) fn write_manifest(
    invocation: &TestInvocation,
    value: &serde_json::Value,
) -> Result<(), String> {
    std::fs::create_dir_all(&invocation.report_dir).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    atomic_write(&manifest_path(invocation), &bytes).map_err(|e| e.to_string())
}

pub(super) fn output_outcome(
    result: Result<thot_runner::vbox::VBoxCommandOutput, String>,
    action: &str,
) -> TestOutcome {
    match result {
        Ok(output) => {
            let success = output.exit_code == Some(0);
            TestOutcome {
                status: if success {
                    thot_core::test_results::TestStatus::Passed
                } else {
                    thot_core::test_results::TestStatus::Failed
                },
                stdout: output.stdout,
                stderr: output.stderr,
                exit_code: output.exit_code,
                diagnostics: if success {
                    String::new()
                } else {
                    format!("{action} exited with {:?}", output.exit_code)
                },
                observation: serde_json::json!({"duration_ms": output.duration.as_millis()}),
                evidence: Vec::new(),
            }
        }
        Err(error) => {
            let timed_out = error.to_ascii_lowercase().contains("timed out");
            TestOutcome {
                status: if timed_out {
                    thot_core::test_results::TestStatus::TimedOut
                } else {
                    thot_core::test_results::TestStatus::Error
                },
                stdout: String::new(),
                stderr: format!("{action}: {error}"),
                exit_code: None,
                diagnostics: format!("{action}: {error}"),
                observation: serde_json::json!({}),
                evidence: Vec::new(),
            }
        }
    }
}

pub(super) async fn vm_state(invocation: &TestInvocation, name: &str) -> Result<String, String> {
    let result = runner(invocation)
        .run(&["showvminfo".into(), name.into(), "--machinereadable".into()])
        .await?;
    if result.exit_code != Some(0) {
        return Err(format!("cannot inspect VM state: {}", result.stderr));
    }
    Ok(result
        .stdout
        .lines()
        .find_map(|line| {
            line.strip_prefix("VMState=\"")
                .and_then(|s| s.strip_suffix('"'))
                .map(str::to_owned)
        })
        .unwrap_or_default())
}

pub(super) async fn wait_for_poweroff(
    invocation: &TestInvocation,
    name: &str,
    deadline: Duration,
) -> Result<(), String> {
    let until = tokio::time::Instant::now() + deadline;
    while tokio::time::Instant::now() < until {
        if matches!(
            vm_state(invocation, name).await?.as_str(),
            "poweroff" | "aborted"
        ) {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    Err("VM did not power off before the deadline".into())
}
