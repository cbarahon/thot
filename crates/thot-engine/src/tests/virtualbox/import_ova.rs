//! Purpose: Imports a delivery OVA, captures its VM configuration, and records its ownership manifest.
use super::{runner, vm_storage, write_manifest};
use crate::test_execution::{TestInvocation, TestOutcome};

pub async fn run(invocation: &TestInvocation) -> TestOutcome {
    if let Some(reason) = invocation.delivery.validation_error.as_deref() {
        return TestOutcome::error(format!("OVA validation failed: {reason}"));
    }
    let vm_name = invocation
        .parameters
        .get("vm_name")
        .cloned()
        .unwrap_or_else(|| {
            format!(
                "thot_{}",
                uuid::Uuid::new_v4().simple().to_string()[..12].to_owned()
            )
        });
    if !safe_name(&vm_name) {
        return TestOutcome::error("vm_name contains unsupported characters");
    }
    let group = invocation
        .parameters
        .get("vbox_group")
        .map(String::as_str)
        .unwrap_or("/thot");
    if !group.starts_with('/') || group.contains('"') || group.contains('\n') {
        return TestOutcome::error("vbox_group must be an absolute VirtualBox group path");
    }
    let storage = match vm_storage(invocation) {
        Ok(path) => path,
        Err(error) => return TestOutcome::error(error),
    };
    let project_vm_path = invocation
        .parameters
        .get("vm_path")
        .cloned()
        .unwrap_or_else(|| "./thot_vms".to_owned());
    if let Err(error) = tokio::fs::create_dir_all(&storage).await {
        return TestOutcome::error(format!("cannot create VM storage directory: {error}"));
    }
    let storage = match storage.canonicalize() {
        Ok(path) => path,
        Err(error) => {
            return TestOutcome::error(format!("cannot resolve VM storage directory: {error}"));
        }
    };
    let manifest = serde_json::json!({
        "schema_version": 2,
        "run_id": invocation.run_id,
        "delivery_id": invocation.delivery.delivery_id,
        "vm_name": vm_name,
        "vm_uuid": null,
        "vm_group": group,
        "storage_path": storage,
        "archive_parent": invocation.project_archive_parent,
        "project_vm_path": project_vm_path,
        "may_be_registered": true,
        "disk_paths": []
    });
    if let Err(error) = write_manifest(invocation, &manifest) {
        return TestOutcome::error(format!(
            "cannot persist cleanup manifest before import: {error}"
        ));
    }

    let import = runner(invocation)
        .run(&[
            "import".into(),
            invocation.delivery.source.to_string_lossy().into_owned(),
            "--vsys".into(),
            "0".into(),
            "--group".into(),
            group.into(),
            "--vmname".into(),
            vm_name.clone(),
            "--basefolder".into(),
            storage.to_string_lossy().into_owned(),
        ])
        .await;
    let mut output = match import {
        Ok(output) if output.exit_code == Some(0) => output,
        Ok(output) => {
            return TestOutcome {
                status: thot_core::test_results::TestStatus::Failed,
                stdout: output.stdout,
                stderr: output.stderr,
                exit_code: output.exit_code,
                diagnostics: "VBoxManage import failed".into(),
                observation: serde_json::json!({"vm_name": vm_name, "storage_path": storage}),
                evidence: Vec::new(),
            };
        }
        Err(error) => {
            let timed_out = error.to_ascii_lowercase().contains("timed out");
            return TestOutcome {
                status: if timed_out {
                    thot_core::test_results::TestStatus::TimedOut
                } else {
                    thot_core::test_results::TestStatus::Error
                },
                stdout: String::new(),
                stderr: error.clone(),
                exit_code: None,
                diagnostics: format!("VBoxManage import failed: {error}"),
                observation: serde_json::json!({"vm_name": vm_name, "storage_path": storage}),
                evidence: Vec::new(),
            };
        }
    };

    let info = runner(invocation)
        .run(&[
            "showvminfo".into(),
            vm_name.clone(),
            "--machinereadable".into(),
        ])
        .await;
    let initial_info = match info {
        Ok(info) if info.exit_code == Some(0) => {
            output.stdout.push_str(&info.stdout);
            output.stderr.push_str(&info.stderr);
            crate::vm_activity::parse_machine_info(&info.stdout)
        }
        Ok(info) => {
            return TestOutcome {
                status: thot_core::test_results::TestStatus::Failed,
                stdout: output.stdout,
                stderr: format!("{}{}", output.stderr, info.stderr),
                exit_code: info.exit_code,
                diagnostics: "VM imported but its identity could not be inspected".into(),
                observation: serde_json::json!({"vm_name": vm_name, "storage_path": storage}),
                evidence: vec!["cleanup-manifest.json".into()],
            };
        }
        Err(error) => {
            return TestOutcome {
                status: thot_core::test_results::TestStatus::Error,
                stdout: output.stdout,
                stderr: format!("{}showvminfo failed: {error}", output.stderr),
                exit_code: None,
                diagnostics: "VM imported but its identity could not be inspected".into(),
                observation: serde_json::json!({"vm_name": vm_name, "storage_path": storage}),
                evidence: vec!["cleanup-manifest.json".into()],
            };
        }
    };
    let uuid = initial_info.get("UUID").cloned();
    let disks = crate::vm_activity::attached_media_paths(&initial_info);
    let updated = serde_json::json!({
        "schema_version": 2,
        "run_id": invocation.run_id,
        "delivery_id": invocation.delivery.delivery_id,
        "vm_name": vm_name,
        "vm_uuid": uuid,
        "vm_group": group,
        "storage_path": storage,
        "archive_parent": invocation.project_archive_parent,
        "project_vm_path": project_vm_path,
        "may_be_registered": true,
        "disk_paths": disks
    });
    if let Err(error) = write_manifest(invocation, &updated) {
        return TestOutcome::error(format!("VM imported but manifest update failed: {error}"));
    }
    if uuid.is_none() {
        return TestOutcome {
            status: thot_core::test_results::TestStatus::Failed,
            stdout: output.stdout,
            stderr: output.stderr,
            exit_code: Some(1),
            diagnostics: "VM imported but its UUID could not be verified".into(),
            observation: serde_json::json!({"vm_name": vm_name, "storage_path": storage}),
            evidence: Vec::new(),
        };
    }
    let uuid = uuid.unwrap();
    if !crate::vm_activity::is_owned_vm(&initial_info, &vm_name, &uuid, &storage) {
        return TestOutcome {
            status: thot_core::test_results::TestStatus::Failed,
            stdout: output.stdout,
            stderr: output.stderr,
            exit_code: Some(1),
            diagnostics: "VM name, UUID, and settings path do not match the imported run storage"
                .into(),
            observation: serde_json::json!({
                "vm_name": vm_name,
                "vm_uuid": uuid,
                "vm_config_path": initial_info.get("CfgFile"),
                "storage_path": storage
            }),
            evidence: vec!["cleanup-manifest.json".into()],
        };
    }
    let adapters = crate::vm_activity::adapter_observations(&initial_info);
    TestOutcome {
        status: thot_core::test_results::TestStatus::Passed,
        stdout: output.stdout,
        stderr: output.stderr,
        exit_code: Some(0),
        diagnostics: String::new(),
        observation: serde_json::json!({
            "vm_name": vm_name,
            "vm_uuid": uuid,
            "vbox_group": group,
            "vm_path": storage,
            "duration_ms": output.duration.as_millis(),
            "network_adapters": adapters,
            "vm_configuration": initial_info
        }),
        evidence: vec!["cleanup-manifest.json".into()],
    }
}

fn safe_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
