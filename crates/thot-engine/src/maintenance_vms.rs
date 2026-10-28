//! Purpose: Reconciles unassigned VMs in Thot's dedicated VirtualBox profile.
use crate::{
    maintenance_plan::CleanPlan,
    vm_activity::{self, VmActivityRuntime},
};
use serde_json::Value;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    process::Command,
    time::{sleep, timeout},
};

#[derive(Debug, Default, Clone)]
pub struct VmCleanupReport {
    pub unregistered_uuids: Vec<String>,
    pub removed_storage_paths: Vec<String>,
    pub preserved_paths: Vec<String>,
    pub failed_manifest_dirs: Vec<String>,
    pub failures: Vec<String>,
}

pub async fn clean_unassigned_vms(
    plan: &CleanPlan,
    runtime: &VmActivityRuntime,
) -> VmCleanupReport {
    let mut report = VmCleanupReport::default();
    let mut protected_uuids = plan.saved_vm_uuids.clone();
    let mut processed_uuids = HashSet::new();
    let mut failed_manifest_storage = Vec::new();

    for manifest in &plan.orphan_manifests {
        let cleanup = vm_activity::cleanup_from_manifest(
            runtime,
            &manifest.report_dir,
            &manifest.delivery_id,
        )
        .await;
        if cleanup.get("status").and_then(Value::as_str) == Some("passed") {
            if let Some(uuid) = manifest.vm_uuid.as_ref() {
                report.unregistered_uuids.push(uuid.clone());
                processed_uuids.insert(uuid.clone());
            }
            if !manifest.storage_path.exists() {
                report
                    .removed_storage_paths
                    .push(manifest.storage_path.display().to_string());
            }
        } else {
            failed_manifest_storage.push(manifest.storage_path.clone());
            if let Some(uuid) = manifest.vm_uuid.as_ref() {
                protected_uuids.insert(uuid.clone());
            }
            report
                .failed_manifest_dirs
                .push(manifest.report_dir.display().to_string());
            let detail = cleanup
                .get("details")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join("; ")
                })
                .unwrap_or_else(|| "manifest cleanup failed".into());
            report
                .failures
                .push(format!("{}: {detail}", manifest.report_dir.display()));
        }
    }

    let inventory = match run_vbox(runtime, &["list", "vms"], Duration::from_secs(30)).await {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).into_owned()
        }
        Ok(output) => {
            report.failures.push(format!(
                "VBoxManage list vms failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
            return report;
        }
        Err(error) => {
            report.failures.push(format!(
                "cannot inspect dedicated VirtualBox registry: {error}"
            ));
            return report;
        }
    };
    for (vm_name, uuid) in parse_vm_inventory(&inventory) {
        if plan.saved_vm_uuids.contains(&uuid)
            || protected_uuids.contains(&uuid)
            || processed_uuids.contains(&uuid)
        {
            continue;
        }
        let info_output = match run_vbox(
            runtime,
            &["showvminfo", &uuid, "--machinereadable"],
            Duration::from_secs(25),
        )
        .await
        {
            Ok(output) if output.status.success() => output,
            Ok(output) => {
                report.failures.push(format!(
                    "cannot inspect VM {uuid}: {}",
                    String::from_utf8_lossy(&output.stderr)
                ));
                continue;
            }
            Err(error) => {
                report
                    .failures
                    .push(format!("cannot inspect VM {uuid}: {error}"));
                continue;
            }
        };
        let info = parse_machine_readable(&String::from_utf8_lossy(&info_output.stdout));
        let settings_path = info.get("CfgFile").map(PathBuf::from);
        let media_paths = info
            .iter()
            .filter_map(|(key, value)| {
                let lower = value.to_ascii_lowercase();
                (key.contains("-")
                    && [".vdi", ".vmdk", ".vhd", ".iso"]
                        .iter()
                        .any(|suffix| lower.ends_with(suffix)))
                .then(|| PathBuf::from(value))
            })
            .collect::<Vec<_>>();
        if failed_manifest_storage.iter().any(|storage| {
            settings_path
                .as_ref()
                .is_some_and(|path| path.starts_with(storage))
                || media_paths.iter().any(|path| path.starts_with(storage))
        }) {
            continue;
        }
        let in_storage = settings_path
            .as_deref()
            .is_some_and(|path| path_is_under(path, &runtime.storage_root))
            && media_paths
                .iter()
                .all(|path| path_is_under(path, &runtime.storage_root));
        let vm_state = info.get("VMState").map(String::as_str).unwrap_or("unknown");
        if !matches!(vm_state, "poweroff" | "aborted") {
            let _ = run_vbox(
                runtime,
                &["controlvm", &uuid, "acpipowerbutton"],
                Duration::from_secs(15),
            )
            .await;
            if !wait_for_poweroff(runtime, &uuid, runtime.shutdown_grace).await {
                match run_vbox(
                    runtime,
                    &["controlvm", &uuid, "poweroff"],
                    Duration::from_secs(20),
                )
                .await
                {
                    Ok(output) if output.status.success() => {}
                    Ok(output) => {
                        report.failures.push(format!(
                            "cannot power off VM {uuid}: {}",
                            String::from_utf8_lossy(&output.stderr)
                        ));
                        continue;
                    }
                    Err(error) => {
                        report
                            .failures
                            .push(format!("cannot power off VM {uuid}: {error}"));
                        continue;
                    }
                }
                if !wait_for_poweroff(runtime, &uuid, Duration::from_secs(10)).await {
                    report
                        .failures
                        .push(format!("VM {uuid} remained active after poweroff request"));
                    continue;
                }
            }
        }
        let mut args = vec!["unregistervm", uuid.as_str()];
        if in_storage {
            args.push("--delete");
        }
        match run_vbox(runtime, &args, Duration::from_secs(120)).await {
            Ok(output) if output.status.success() => {
                report.unregistered_uuids.push(uuid);
                if in_storage {
                    if let Some(settings_path) = settings_path {
                        let dir = settings_path.parent().unwrap_or(&settings_path);
                        report.removed_storage_paths.push(dir.display().to_string());
                    }
                } else {
                    if let Some(path) = settings_path {
                        report.preserved_paths.push(path.display().to_string());
                    }
                    report.preserved_paths.extend(
                        media_paths
                            .into_iter()
                            .map(|path| path.display().to_string()),
                    );
                    report.failures.push(format!("VM {vm_name} was unregistered without deleting files because ownership by Thot storage could not be proven"));
                }
            }
            Ok(output) => report.failures.push(format!(
                "cannot unregister VM {uuid}: {}",
                String::from_utf8_lossy(&output.stderr)
            )),
            Err(error) => report
                .failures
                .push(format!("cannot unregister VM {uuid}: {error}")),
        }
    }
    report
}

async fn run_vbox(
    runtime: &VmActivityRuntime,
    args: &[&str],
    limit: Duration,
) -> Result<std::process::Output, String> {
    timeout(
        limit,
        Command::new(&runtime.vbox_manage)
            .args(args)
            .env("VBOX_USER_HOME", &runtime.vbox_user_home)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| "VBoxManage command timed out".to_owned())?
    .map_err(|e| format!("cannot start VBoxManage: {e}"))
}

async fn wait_for_poweroff(runtime: &VmActivityRuntime, uuid: &str, duration: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + duration;
    while tokio::time::Instant::now() < deadline {
        if let Ok(output) = run_vbox(
            runtime,
            &["showvminfo", uuid, "--machinereadable"],
            Duration::from_secs(10),
        )
        .await
        {
            if output.status.success()
                && parse_machine_readable(&String::from_utf8_lossy(&output.stdout))
                    .get("VMState")
                    .is_some_and(|state| matches!(state.as_str(), "poweroff" | "aborted"))
            {
                return true;
            }
        }
        sleep(Duration::from_millis(500)).await;
    }
    false
}

fn parse_vm_inventory(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let open = line.rfind('{')?;
            let close = line[open..].find('}')? + open;
            let uuid = line[open + 1..close].trim().to_owned();
            let name = line[..open].trim().trim_matches('"').to_owned();
            (!name.is_empty() && !uuid.is_empty()).then_some((name, uuid))
        })
        .collect()
}

fn parse_machine_readable(text: &str) -> std::collections::HashMap<String, String> {
    text.lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            let value = value.trim().trim_matches('"').replace("\\\"", "\"");
            Some((key.trim().to_owned(), value))
        })
        .collect()
}

fn path_is_under(path: &Path, root: &Path) -> bool {
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    let Ok(path) = path.canonicalize() else {
        return false;
    };
    path.starts_with(root)
}
