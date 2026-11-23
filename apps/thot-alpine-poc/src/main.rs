use sha2::{Digest, Sha256, Sha512};
use std::{
    fs,
    io::Read,
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};
use thot_core::{sandbox_contracts::*, sandbox_project::*, sandbox_protocol::*};
use thot_delivery::{DeliveryInfo, build_agent, default_student_input_root};
type Config = SandboxProject;

pub fn run_project(
    project_path: &std::path::Path,
    agent_path: &std::path::Path,
) -> Result<(), String> {
    let project = project_path.canonicalize().map_err(|e| e.to_string())?;
    let config_path = project.join("configuration.thot.yaml");
    let config_bytes = fs::read(&config_path).map_err(|e| e.to_string())?;
    if config_bytes.len() > 1024 * 1024 {
        return Err("configuration.thot.yaml exceeds the 1 MiB limit".into());
    }
    let config: Config = serde_yaml::from_slice(&config_bytes).map_err(|e| e.to_string())?;
    config.validate()?;
    let p = &config.sandbox_platforms[0];
    let preparation_scripts = resolve_preparation_scripts(&project, p)?;
    let test_modules = resolve_test_modules(&project, p)?;
    let input_root = default_student_input_root(&config);
    let mut delivery_agent = build_agent(
        config.delivery_agent.as_ref(),
        &project,
        config.delivery_folder.as_deref(),
        input_root.as_deref(),
        &config.delivery_limits,
    )
    .map_err(|error| error.to_string())?;
    let delivery_inventory = delivery_agent.list().map_err(|error| error.to_string())?;
    let roster = load_roster(
        &project,
        &config,
        p.lifecycle.student_clone.name_prefix.as_str(),
        &delivery_inventory,
    )?;
    for input in &p.inputs {
        if let Some(path) = &input.path {
            let _ = project_input(&project, path)?;
        }
    }
    preflight_host(p.limits.memory_mib)?;
    let agent = agent_path
        .canonicalize()
        .map_err(|error| format!("guest agent binary unavailable: {error}"))?;
    let agent_version = Command::new(&agent)
        .arg("--version")
        .output()
        .map_err(|error| format!("cannot execute the Alpine guest agent: {error}"))?;
    if !agent_version.status.success()
        || String::from_utf8_lossy(&agent_version.stdout).trim()
            != format!("{} protocol=2", p.lifecycle.agent.version)
    {
        return Err(format!(
            "guest agent version mismatch; project requires {} protocol=2",
            p.lifecycle.agent.version
        ));
    }
    let evaluation_id = uuid::Uuid::new_v4().to_string();
    let snapshot_files = vec![
        ("configuration.thot.yaml".to_owned(), config_bytes.clone()),
        (
            config.students_file.clone(),
            read_project_file(&project, &config.students_file)?,
        ),
        (
            config.assignments_file.clone(),
            read_project_file(&project, &config.assignments_file)?,
        ),
    ];
    let mut run_repository = thot_engine::sandbox_repository::ProjectRunRepository::begin(
        &project,
        &evaluation_id,
        &config.project_id,
        &snapshot_files,
    )?;
    let result_dir = run_repository.run_dir().to_path_buf();
    run_repository.start()?;
    for module in test_modules.values() {
        let manifest_artifact =
            run_repository.put_artifact(&module.manifest_bytes, "application/json", true)?;
        let package_artifact =
            run_repository.put_artifact(&module.package_bytes, "text/x-shellscript", true)?;
        run_repository.append_event(
            "test_module.pinned",
            serde_json::json!({
                "module_id": module.reference.module_id,
                "version": module.reference.version,
                "package_sha256": module.reference.package_sha256,
                "capabilities": module.reference.capabilities,
                "manifest_artifact": manifest_artifact,
                "package_artifact": package_artifact
            }),
        )?;
    }
    let root = std::env::var_os("THOT_KVM_ALPINE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/var/lib/libvirt/images/thot-alpine-poc"));
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    check_disk_capacity(&root)?;
    let _lock = RunLock::acquire(root.join(".thot-alpine.lock"))?;
    let image = root.join(format!(
        "alpine-{}.qcow2",
        p.lifecycle.template_image.release
    ));
    ensure_image(
        &image,
        &p.lifecycle.template_image.image_url,
        &p.lifecycle.template_image.sha512,
    )?;
    let template_disk = root.join(format!("{}.qcow2", p.lifecycle.template_name));
    let baseline_disk = root.join(format!("{}.qcow2", p.lifecycle.project_clone.name));
    let base_manifest = format!(
        "thot-template:v1:{}:{}:{}:{}",
        p.lifecycle.template_image.release,
        p.lifecycle.template_image.architecture,
        p.lifecycle.template_image.sha512,
        p.lifecycle.template_vm.disk_gib
    );
    let manifest_path = root.join(format!("{}.manifest", p.lifecycle.template_name));
    if template_disk.exists() {
        let current = fs::read_to_string(&manifest_path).map_err(|_| {
            format!(
                "refusing to adopt unowned template disk `{}`",
                template_disk.display()
            )
        })?;
        if current != base_manifest {
            return Err(format!(
                "template `{}` already exists with different provenance; choose a new template_name or remove it after review",
                p.lifecycle.template_name
            ));
        }
    } else {
        let temporary = template_disk.with_extension(format!("pending-{}", uuid::Uuid::new_v4()));
        fs::copy(&image, &temporary).map_err(|e| e.to_string())?;
        run(&[
            "qemu-img",
            "resize",
            temporary.to_str().unwrap(),
            &format!("{}G", p.lifecycle.template_vm.disk_gib),
        ])?;
        fs::rename(&temporary, &template_disk).map_err(|e| e.to_string())?;
        atomic_write(&manifest_path, base_manifest.as_bytes())?;
    }
    let template_info = run(&[
        "qemu-img",
        "info",
        "--output=json",
        template_disk.to_str().unwrap(),
    ])?;
    let virtual_size = serde_json::from_str::<serde_json::Value>(&template_info)
        .map_err(|e| e.to_string())?["virtual-size"]
        .as_u64()
        .unwrap_or(0);
    if virtual_size < (p.lifecycle.template_vm.disk_gib as u64) * 1024 * 1024 * 1024 {
        run(&[
            "qemu-img",
            "resize",
            template_disk.to_str().unwrap(),
            &format!("{}G", p.lifecycle.template_vm.disk_gib),
        ])?;
    }
    let baseline_manifest = root.join(format!("{}.manifest", p.lifecycle.project_clone.name));
    let preparation_identity = serde_json::to_vec(&serde_json::json!({
        "project_id": config.project_id,
        "platform_id": p.id,
        "template": base_manifest,
        "vcpu": p.lifecycle.template_vm.vcpu,
        "memory_mib": p.lifecycle.template_vm.memory_mib,
        "packages": p.lifecycle.preparation.packages,
        "scripts": preparation_scripts.iter().map(|script| (&script.path, &script.sha256, script.timeout_seconds)).collect::<Vec<_>>(),
        "network": p.network.preparation
    }))
    .map_err(|error| error.to_string())?;
    let prep_key = format!("{:x}", Sha256::digest(preparation_identity));
    let desired_manifest = format!("thot-alpine-baseline:v1:{prep_key}");
    let mut baseline_ready = false;
    if baseline_disk.exists() {
        let current = fs::read_to_string(&baseline_manifest).map_err(|_| {
            format!(
                "refusing to adopt unowned baseline disk `{}`",
                baseline_disk.display()
            )
        })?;
        if current == desired_manifest || current == format!("thot-alpine-preparing:v1:{prep_key}")
        {
            baseline_ready = run(&[
                "guestfish",
                "--ro",
                "-a",
                baseline_disk.to_str().unwrap(),
                "-i",
                "cat",
                "/var/lib/thot-preparation-complete",
            ])
            .is_ok();
            if baseline_ready && current != desired_manifest {
                atomic_write(&baseline_manifest, desired_manifest.as_bytes())?;
            }
        }
        if !baseline_ready {
            if !current.starts_with("thot-alpine-baseline:v1:")
                && !current.starts_with("thot-alpine-preparing:v1:")
            {
                return Err(format!(
                    "refusing to overwrite baseline with unrecognized manifest `{}`",
                    baseline_manifest.display()
                ));
            }
            if disk_in_use(&baseline_disk)? {
                return Err(format!(
                    "baseline `{}` is attached to a VM and cannot be replaced",
                    baseline_disk.display()
                ));
            }
            fs::remove_file(&baseline_disk).map_err(|e| e.to_string())?;
            fs::remove_file(&baseline_manifest).map_err(|e| e.to_string())?;
        }
    }
    if !baseline_ready {
        atomic_write(
            &baseline_manifest,
            format!("thot-alpine-preparing:v1:{prep_key}").as_bytes(),
        )?;
        let _baseline_domain = OwnedDomainCleanup {
            name: p.lifecycle.project_clone.name.clone(),
            disk: baseline_disk.clone(),
        };
        run(&[
            "qemu-img",
            "create",
            "-f",
            "qcow2",
            "-F",
            "qcow2",
            "-b",
            template_disk.to_str().unwrap(),
            baseline_disk.to_str().unwrap(),
        ])?;
        let cloud = root.join("baseline-cloud-init");
        fs::create_dir_all(&cloud).map_err(|e| e.to_string())?;
        let mut preparation_script = String::from(
            "#!/bin/sh\nset -u\nmkdir -p /var/lib/thot-preparation\n: > /var/lib/thot-preparation/apk.stdout\n: > /var/lib/thot-preparation/apk.stderr\n",
        );
        if !p.lifecycle.preparation.packages.is_empty() {
            preparation_script.push_str("if ! apk add --no-progress");
            for package in &p.lifecycle.preparation.packages {
                preparation_script.push(' ');
                preparation_script.push_str(package);
            }
            preparation_script.push_str(" > /var/lib/thot-preparation/apk.stdout 2> /var/lib/thot-preparation/apk.stderr; then\n  head -c 1048576 /var/lib/thot-preparation/apk.stdout > /var/lib/thot-preparation/apk.stdout.cap && mv /var/lib/thot-preparation/apk.stdout.cap /var/lib/thot-preparation/apk.stdout\n  head -c 1048576 /var/lib/thot-preparation/apk.stderr > /var/lib/thot-preparation/apk.stderr.cap && mv /var/lib/thot-preparation/apk.stderr.cap /var/lib/thot-preparation/apk.stderr\n  echo package-install-failed > /var/lib/thot-preparation-failed\n  sync\n  poweroff\n  exit 1\nfi\n");
        }
        let mut script_files = String::new();
        for (index, script) in preparation_scripts.iter().enumerate() {
            let guest_script = format!("/var/lib/thot-preparation/script-{index:03}.sh");
            let stdout_path = format!("/var/lib/thot-preparation/script-{index:03}.stdout");
            let stderr_path = format!("/var/lib/thot-preparation/script-{index:03}.stderr");
            let status_path = format!("/var/lib/thot-preparation/script-{index:03}.exit");
            script_files.push_str(&format!(
                "\n  - path: {guest_script}\n    permissions: '0700'\n    encoding: b64\n    content: {}",
                base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &script.bytes)
            ));
            preparation_script.push_str(&format!(
                "if /bin/busybox timeout -s KILL {} /bin/sh {guest_script} > {stdout_path} 2> {stderr_path}; then\n  rc=0\nelse\n  rc=$?\nfi\nhead -c 1048576 {stdout_path} > {stdout_path}.cap && mv {stdout_path}.cap {stdout_path}\nhead -c 1048576 {stderr_path} > {stderr_path}.cap && mv {stderr_path}.cap {stderr_path}\nprintf '%s\\n' \"$rc\" > {status_path}\nif [ \"$rc\" -ne 0 ]; then\n  touch /var/lib/thot-preparation-failed\n  sync\n  poweroff\n  exit \"$rc\"\nfi\n",
                script.timeout_seconds
            ));
        }
        preparation_script.push_str("head -c 1048576 /var/lib/thot-preparation/apk.stdout > /var/lib/thot-preparation/apk.stdout.cap && mv /var/lib/thot-preparation/apk.stdout.cap /var/lib/thot-preparation/apk.stdout\nhead -c 1048576 /var/lib/thot-preparation/apk.stderr > /var/lib/thot-preparation/apk.stderr.cap && mv /var/lib/thot-preparation/apk.stderr.cap /var/lib/thot-preparation/apk.stderr\napk info -v > /var/lib/thot-preparation/packages.txt\ntouch /var/lib/thot-preparation-complete\nsync\npoweroff\n");
        let encoded_script = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            preparation_script,
        );
        let userdata = format!(
            "#cloud-config\nwrite_files:\n  - path: /var/lib/thot-prepare.sh\n    permissions: '0700'\n    encoding: b64\n    content: {encoded_script}{script_files}\nruncmd:\n  - [/bin/sh, /var/lib/thot-prepare.sh]\n"
        );
        fs::write(cloud.join("user-data"), userdata).map_err(|e| e.to_string())?;
        fs::write(
            cloud.join("meta-data"),
            "instance-id: thot-alpine-poc-baseline\nlocal-hostname: thot-alpine-poc-baseline\n",
        )
        .map_err(|e| e.to_string())?;
        let seed = root.join("baseline-seed.iso");
        run(&[
            "cloud-localds",
            seed.to_str().unwrap(),
            cloud.join("user-data").to_str().unwrap(),
            cloud.join("meta-data").to_str().unwrap(),
        ])?;
        let seed_disk = format!("{},device=cdrom", seed.display());
        run(&[
            "virt-install",
            "--name",
            &p.lifecycle.project_clone.name,
            "--import",
            "--osinfo",
            "generic",
            "--memory",
            &p.lifecycle.template_vm.memory_mib.to_string(),
            "--vcpus",
            &p.lifecycle.template_vm.vcpu.to_string(),
            "--disk",
            baseline_disk.to_str().unwrap(),
            "--disk",
            &seed_disk,
            "--network",
            "network=default",
            "--graphics",
            "none",
            "--noautoconsole",
            "--transient",
            "--wait=0",
        ])?;
        wait_for_domain_shutdown(
            &p.lifecycle.project_clone.name,
            &baseline_disk,
            p.lifecycle.preparation.timeout_seconds,
        )?;
        if run(&[
            "guestfish",
            "--ro",
            "-a",
            baseline_disk.to_str().unwrap(),
            "-i",
            "cat",
            "/var/lib/thot-preparation-complete",
        ])
        .is_err()
        {
            return Err("project preparation did not create its success marker; see guest cloud-init diagnostics".into());
        }
        atomic_write(&baseline_manifest, desired_manifest.as_bytes())?;
    }
    persist_preparation_record(
        &mut run_repository,
        &baseline_disk,
        &p.lifecycle.preparation.packages,
        &preparation_scripts,
    )?;
    run_repository.append_event(
        "platform.baseline_ready",
        serde_json::json!({
            "platform_id": p.id,
            "provider": p.kind,
            "template_name": p.lifecycle.template_name,
            "template_manifest_sha256": format!("{:x}", Sha256::digest(base_manifest.as_bytes())),
            "baseline_name": p.lifecycle.project_clone.name,
            "baseline_manifest_sha256": format!("{:x}", Sha256::digest(desired_manifest.as_bytes())),
            "image_release": p.lifecycle.template_image.release,
            "guest_agent_version": p.lifecycle.agent.version
        }),
    )?;
    let mut domain_names = std::collections::HashSet::new();
    for (id, selector) in roster {
        let intake = TemporaryDirectory::create()?;
        let acquired = match &selector {
            DeliverySelector::Name(name) => delivery_agent.get(name, &intake.0.join("submission")),
            DeliverySelector::ProviderId(provider_id) => {
                delivery_agent.get_by_id(provider_id, &intake.0.join("submission"))
            }
        }
        .map_err(|error| error.to_string())?;
        let source_kind = match config.delivery_agent.as_ref() {
            Some(DeliveryAgentConfig::Gdrive { .. }) => "gdrive",
            _ => "local",
        };
        let selector_value = match &selector {
            DeliverySelector::Name(name) => name.clone(),
            DeliverySelector::ProviderId(provider_id) => provider_id.clone(),
        };
        let acquired_at = chrono::Utc::now().to_rfc3339();
        let provenance = thot_core::sandbox_contracts::DeliveryProvenance {
            source_kind: source_kind.into(),
            selector: selector_value,
            provider_id: acquired.info.provider_id.clone(),
            revision: acquired.info.revision.clone(),
            name: acquired.info.name.clone(),
            mime_type: acquired.info.mime_type.clone(),
            source_checksum: acquired.info.source_checksum.clone(),
            source_checksum_algorithm: acquired.info.source_checksum_algorithm.clone(),
            size_bytes: acquired.info.size_bytes,
            created_at: acquired.info.created_at.clone(),
            modified_at: acquired.info.modified_at.clone(),
            acquired_at: acquired_at.clone(),
            sha256: acquired.sha256.clone(),
        };
        let persisted_input = run_repository.persist_input(&id, &acquired.path, provenance)?;
        let domain = format!(
            "{}-{}",
            p.lifecycle.student_clone.name_prefix,
            id.replace(|c: char| !c.is_ascii_alphanumeric(), "-")
        );
        if domain.len() > 55 || !domain_names.insert(domain.clone()) {
            return Err(format!(
                "student IDs produce an invalid or duplicate libvirt name `{domain}`"
            ));
        }
        let disk = root.join(format!("{domain}.qcow2"));
        if disk_in_use(&disk)? {
            let _ = run(&["virsh", "destroy", &domain]);
            let _ = run(&["virsh", "undefine", &domain, "--nvram"]);
            if disk_in_use(&disk)? {
                return Err(format!(
                    "refusing to replace student disk `{}` while domain `{domain}` still references it",
                    disk.display()
                ));
            }
        }
        let _ = fs::remove_file(&disk);
        run(&[
            "qemu-img",
            "create",
            "-f",
            "qcow2",
            "-F",
            "qcow2",
            "-b",
            baseline_disk.to_str().unwrap(),
            disk.to_str().unwrap(),
        ])?;
        let mut cleanup = StudentCleanup {
            domain: domain.clone(),
            disk: disk.clone(),
            seed: None,
            work: None,
        };
        let run_id = evaluation_id.clone();
        let unit_id = format!("{run_id}-{id}");
        let token = new_session_token()?;
        let port = 49152
            + (u16::from_le_bytes([
                *token.as_bytes().first().unwrap_or(&1),
                *token.as_bytes().get(1).unwrap_or(&2),
            ]) % 16383);
        let listener = vsock_listener(port)?;
        let work = root.join(format!("seed-{id}"));
        fs::create_dir_all(&work).map_err(|e| e.to_string())?;
        let agent_bytes = fs::read(&agent).map_err(|e| e.to_string())?;
        let mut user = String::from(
            "#cloud-config\nusers:\n  - default\n  - name: thot-student\n    uid: 1001\n    primary_group: thot-student\n    groups: [thot-student]\n    shell: /bin/ash\n    lock_passwd: true\n    sudo: []\nwrite_files:\n  - path: /usr/local/bin/thot-alpine-agent\n    permissions: '0755'\n    encoding: b64\n    content: ",
        );
        user.push_str(&base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            agent_bytes,
        ));
        user.push_str("\n  - path: /etc/thot-agent-token\n    permissions: '0400'\n    content: ");
        user.push_str(&yaml_quote(&token));
        user.push_str(
            "\nruncmd:\n  - [mkdir, -p, /work]\n  - [/usr/local/bin/thot-alpine-agent, '2', '",
        );
        user.push_str(&port.to_string());
        user.push_str("', /etc/thot-agent-token, '");
        user.push_str(&run_id);
        user.push_str("', '");
        user.push_str(&unit_id);
        user.push_str("']\n");
        fs::write(work.join("user-data"), user).map_err(|e| e.to_string())?;
        fs::write(
            work.join("meta-data"),
            format!("instance-id: {domain}\nlocal-hostname: {domain}\n"),
        )
        .map_err(|e| e.to_string())?;
        let seed = root.join(format!("seed-{id}.iso"));
        cleanup.seed = Some(seed.clone());
        cleanup.work = Some(work.clone());
        run(&[
            "cloud-localds",
            seed.to_str().unwrap(),
            work.join("user-data").to_str().unwrap(),
            work.join("meta-data").to_str().unwrap(),
        ])?;
        let seed_disk = format!("{},device=cdrom", seed.display());
        run(&[
            "virt-install",
            "--name",
            &domain,
            "--import",
            "--osinfo",
            "generic",
            "--memory",
            &p.limits.memory_mib.to_string(),
            "--vcpus",
            &p.limits.vcpu.to_string(),
            "--disk",
            disk.to_str().unwrap(),
            "--disk",
            &seed_disk,
            "--network",
            "none",
            "--vsock",
            "cid.auto=yes",
            "--graphics",
            "none",
            "--noautoconsole",
        ])?;
        let (mut stream, peer) = accept_vsock(&listener)?;
        if peer == libc::VMADDR_CID_ANY {
            return Err("unexpected wildcard VSOCK peer CID".into());
        }
        let handshake = HandshakeRequest {
            protocol_version: ALPINE_PROTOCOL_VERSION,
            run_id: run_id.clone(),
            student_unit_id: unit_id.clone(),
            expected_peer_cid: "2".into(),
            nonce: uuid::Uuid::new_v4().to_string(),
            secret: token.clone(),
        };
        write_frame(&mut stream, &AgentRequest::Handshake(handshake.clone()))
            .map_err(|e| e.to_string())?;
        let response: AgentResponse = read_frame(&mut stream).map_err(|e| e.to_string())?;
        let guest_capabilities = match response {
            AgentResponse::Handshake(answer)
                if answer.protocol_version == ALPINE_PROTOCOL_VERSION
                    && answer.nonce == handshake.nonce =>
            {
                answer.capabilities
            }
            _ => return Err("agent handshake rejected".into()),
        };
        for module in test_modules.values() {
            for required in &module.reference.capabilities {
                if !guest_capabilities.iter().any(|available| {
                    available.id == required.id && available.version == required.version
                }) {
                    return Err(format!(
                        "guest agent lacks required module capability {}@{}; refusing execution",
                        required.id, required.version
                    ));
                }
            }
        }
        let mut staging_budget = StagingBudget::new(
            config
                .delivery_limits
                .max_bytes
                .saturating_add(32 * 1024 * 1024),
            config.delivery_limits.max_files.saturating_add(100),
        );
        for input in &p.inputs {
            let source_path = if input.role == "student-deliverable" {
                acquired.path.clone()
            } else {
                let source = input
                    .path_template
                    .as_ref()
                    .map(|template| template.replace("{student_id}", &id))
                    .or(input.path.clone())
                    .ok_or("input missing source path")?;
                project_input(&project, &source)?
            };
            if input.role == "student-deliverable" || input.path_template.is_some() {
                stage_input(
                    &mut stream,
                    &source_path,
                    &input.copy_to.replace("{student_id}", &id),
                    &mut staging_budget,
                )?;
            } else {
                stage_input(
                    &mut stream,
                    &source_path,
                    &input.copy_to,
                    &mut staging_budget,
                )?;
            }
        }
        let mut staged_modules = std::collections::HashSet::new();
        for module in test_modules.values() {
            if staged_modules.insert(module.reference.package_sha256.clone()) {
                staging_budget.bytes = staging_budget
                    .bytes
                    .saturating_add(module.package_bytes.len() as u64);
                staging_budget.files = staging_budget.files.saturating_add(1);
                if staging_budget.bytes > staging_budget.max_bytes
                    || staging_budget.files > staging_budget.max_files
                {
                    return Err("submission, instructor inputs and module packages exceed the configured staging budget".into());
                }
                stage_path(&mut stream, &module.source_path, &module.guest_path)?;
            }
        }
        let mut test_results: Vec<(String, bool, serde_json::Value)> = Vec::new();
        let mut test_json = Vec::new();
        let mut infrastructure_pending = false;
        for test in &p.tests {
            let blocked = test.requires.iter().any(|dependency| {
                !test_results
                    .iter()
                    .any(|(id, ok, _)| id == dependency && *ok)
            });
            if blocked {
                let pending_dependency =
                    dependency_has_pending_result(&test.requires, &test_results);
                let (execution_status, assessment, reason) = if pending_dependency {
                    (
                        "skipped",
                        "pending",
                        "required test has infrastructure-pending evidence",
                    )
                } else {
                    ("skipped", "skipped", "required test did not pass")
                };
                let skipped = serde_json::json!({"status":execution_status,"assessment":assessment,"reason":reason});
                test_json.push(serde_json::json!({"test_id":test.id,"result":skipped}));
                test_results.push((test.id.clone(), false, skipped));
                continue;
            }
            let module_launch = test_modules.get(&test.id).map(|module| ModuleLaunch {
                module_id: module.reference.module_id.clone(),
                version: module.reference.version.clone(),
                package_sha256: module.reference.package_sha256.clone(),
                package_path: module.guest_path.clone().into(),
                capabilities: module.reference.capabilities.clone(),
            });
            let request = ExecuteTaskRequest {
                task_id: uuid::Uuid::new_v4().to_string(),
                run_id: run_id.clone(),
                student_unit_id: unit_id.clone(),
                working_dir: "/work".into(),
                argv: test.argv.clone(),
                limits: ExecutionLimits {
                    cpu_time_limit: 0,
                    memory_limit: 0,
                    output_limits: StreamOutputLimits {
                        stdout: p.limits.stdout_bytes,
                        stderr: p.limits.stderr_bytes,
                    },
                },
                timeout: test.timeout_seconds.min(p.limits.student_timeout_seconds),
                stdout_limit: p.limits.stdout_bytes,
                stderr_limit: p.limits.stderr_bytes,
                is_preparation: false,
                module: module_launch,
            };
            write_frame(&mut stream, &AgentRequest::ExecuteTask(request.clone()))
                .map_err(|e| e.to_string())?;
            let response: AgentResponse = read_frame(&mut stream).map_err(|e| e.to_string())?;
            let result = match response {
                AgentResponse::ExecuteTaskResult(r) => r,
                _ => return Err("unexpected agent response".into()),
            };
            if result.task_id != request.task_id
                || result.run_id != request.run_id
                || result.student_unit_id != request.student_unit_id
            {
                return Err("agent returned a result for a different task or student unit".into());
            }
            let passed = thot_engine::sandbox::evaluate_sandbox_test(
                result.exit_code,
                result.timed_out,
                test.expect_exit_code,
                test.expected_stdout.as_deref(),
                &result.stdout,
            );
            infrastructure_pending |= matches!(
                result.status,
                ExecutionStatus::ResourceExceeded
                    | ExecutionStatus::PlatformError
                    | ExecutionStatus::Cancelled
                    | ExecutionStatus::Interrupted
                    | ExecutionStatus::Unsupported
            );
            let assessment = if infrastructure_pending
                && result.status != ExecutionStatus::Completed
                && result.status != ExecutionStatus::TimedOut
            {
                "pending"
            } else if passed {
                "passed"
            } else {
                "failed"
            };
            let stdout_artifact = run_repository.put_artifact(
                result.stdout.as_bytes(),
                "text/plain; charset=utf-8",
                !result.truncated,
            )?;
            let stderr_artifact = run_repository.put_artifact(
                result.stderr.as_bytes(),
                "text/plain; charset=utf-8",
                !result.truncated,
            )?;
            let v = serde_json::json!({"status":execution_status_name(result.status),"assessment":assessment,"exit_code":result.exit_code,"stdout":result.stdout,"stderr":result.stderr,"stdout_artifact":stdout_artifact,"stderr_artifact":stderr_artifact,"duration_ms":result.elapsed_ms,"truncated":result.truncated,"diagnostics":result.diagnostics,"module":test_modules.get(&test.id).map(|module|serde_json::json!({"module_id":module.reference.module_id,"version":module.reference.version,"package_sha256":module.reference.package_sha256}))});
            test_json.push(serde_json::json!({"test_id":test.id,"result":v}));
            test_results.push((test.id.clone(), passed, v));
        }
        let checks = p
            .evaluation
            .checks
            .iter()
            .map(|(name, check)| (name.clone(), check.tests.clone()))
            .collect::<std::collections::HashMap<_, _>>();
        let test_passed = test_results
            .iter()
            .map(|(test_id, passed, _)| (test_id.clone(), *passed))
            .collect::<std::collections::HashMap<_, _>>();
        let (grade, grade_scale, earned, item_results, grade_pending) = if let Some(rubric) =
            &p.evaluation.rubric
        {
            let criteria = rubric
                .criteria
                .iter()
                .map(|criterion| (criterion.check.clone(), criterion.points))
                .collect::<Vec<_>>();
            let evaluated =
                thot_engine::sandbox::evaluate_sandbox_rubric(&checks, &test_passed, &criteria)?;
            let calculated_grade = evaluated
                .iter()
                .map(|criterion| criterion.earned_points)
                .sum::<u32>();
            let grade = (!infrastructure_pending).then_some(calculated_grade);
            let earned = evaluated.iter().map(|criterion| serde_json::json!({"check":criterion.check,"points":criterion.earned_points,"maximum":criterion.maximum_points})).collect::<Vec<_>>();
            (grade, rubric.scale, earned, Vec::new(), grade.is_none())
        } else {
            use thot_engine::sandbox::{SandboxItemStatus, SandboxTestAssessment};
            let assessments = test_results
                .iter()
                .map(|(test_id, _, result)| {
                    let assessment = match result["assessment"].as_str().unwrap_or("pending") {
                        "passed" => SandboxTestAssessment::Passed,
                        "failed" => SandboxTestAssessment::Failed,
                        "skipped" => SandboxTestAssessment::Skipped,
                        _ => SandboxTestAssessment::Pending,
                    };
                    (test_id.clone(), assessment)
                })
                .collect::<std::collections::HashMap<_, _>>();
            let evaluated = thot_engine::sandbox::evaluate_sandbox_items(
                &checks,
                &assessments,
                &p.evaluation.items,
            )?;
            let grade_pending = evaluated
                .iter()
                .any(|item| item.status == SandboxItemStatus::Pending);
            let calculated_grade = evaluated.iter().filter_map(|item| item.grade).sum::<u32>();
            let grade_scale = evaluated.iter().map(|item| item.scale).sum::<u32>();
            let grade = (!grade_pending).then_some(calculated_grade);
            let earned = evaluated.iter().flat_map(|item| item.criteria.iter().map(|criterion| serde_json::json!({"item_id":item.id,"check":criterion.check,"status":if criterion.pending{"pending"}else{"complete"},"points":if criterion.pending{None}else{Some(criterion.earned_points)},"maximum":criterion.maximum_points}))).collect::<Vec<_>>();
            let item_results = evaluated.iter().map(|item| serde_json::json!({"id":item.id,"title":item.title,"status":match item.status {SandboxItemStatus::Complete => "complete", SandboxItemStatus::Pending => "pending"},"grade":item.grade,"scale":item.scale,"criteria":item.criteria.iter().map(|criterion| serde_json::json!({"check":criterion.check,"status":if criterion.pending{"pending"}else{"complete"},"points":if criterion.pending{None}else{Some(criterion.earned_points)},"maximum":criterion.maximum_points})).collect::<Vec<_>>()})).collect::<Vec<_>>();
            (grade, grade_scale, earned, item_results, grade_pending)
        };
        let provenance = serde_json::json!({"source_kind":source_kind,"selector":match selector { DeliverySelector::Name(name) => name, DeliverySelector::ProviderId(provider_id) => provider_id },"provider_id":acquired.info.provider_id,"revision":acquired.info.revision,"name":acquired.info.name,"mime_type":acquired.info.mime_type,"source_checksum":acquired.info.source_checksum,"source_checksum_algorithm":acquired.info.source_checksum_algorithm,"size_bytes":acquired.info.size_bytes,"created_at":acquired.info.created_at,"modified_at":acquired.info.modified_at,"acquired_at":acquired_at,"sha256":acquired.sha256,"archived_input":persisted_input});
        let result_path = result_dir.join(format!("{id}.json"));
        let seed_path = root.join(format!("seed-{id}.iso"));
        let work_path = root.join(format!("seed-{id}"));
        drop(cleanup);
        let cleanup_clean = matches!(disk_in_use(&disk), Ok(false))
            && !disk.exists()
            && !seed_path.exists()
            && !work_path.exists();
        let cleanup_status = if cleanup_clean { "clean" } else { "failed" };
        run_repository.append_event(
            "student.cleanup_verified",
            serde_json::json!({"student_unit_id": id, "cleanup_status": cleanup_status}),
        )?;
        let report = serde_json::json!({"schema_version":if p.evaluation.rubric.is_some(){"thot.result/v2"}else{"thot.result/v3"},"student_id":id,"run_id":run_id,"platform":"kvm-alpine","deliverable":provenance,"tests":test_json,"criteria":earned,"evaluation_items":item_results,"grade":grade,"scale":grade_scale,"assessment_status":if cleanup_clean && !grade_pending{"complete"}else{"pending"},"cleanup_status":cleanup_status,"infrastructure_pending":infrastructure_pending});
        thot_engine::test_result_store::persist_report(&result_path, &report)
            .map_err(|error| error.to_string())?;
        run_repository.append_event(
            "student.result_persisted",
            serde_json::json!({"student_unit_id": id, "result_path": format!("{id}.json"), "grade": grade}),
        )?;
        if let (Some(uid), Some(gid)) = (
            std::env::var("SUDO_UID")
                .ok()
                .and_then(|x| x.parse::<u32>().ok()),
            std::env::var("SUDO_GID")
                .ok()
                .and_then(|x| x.parse::<u32>().ok()),
        ) {
            change_owner(&result_path, uid, gid)?;
        }
        if !cleanup_clean {
            run_repository.mark_failed("student_cleanup_failed")?;
            return Err(format!(
                "cleanup failed for `{id}`; run was left incomplete and no later delivery was started"
            ));
        }
        if let Some(grade) = grade {
            println!("{id}: {grade}/{grade_scale}");
        } else {
            println!("{id}: pending infrastructure review");
        }
    }
    let result_paths = roster_result_paths(&result_dir)?;
    run_repository.commit_revision("initial-evaluation", result_paths)?;
    run_repository.verify_journal()?;
    println!("Results: {}", result_dir.display());
    Ok(())
}

fn run(args: &[&str]) -> Result<String, String> {
    let output = Command::new(args[0])
        .args(&args[1..])
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "{} {:?} failed: {}",
            args[0],
            &args[1..],
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn atomic_write(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    let temporary = path.with_extension(format!("pending-{}", uuid::Uuid::new_v4()));
    fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
    fs::rename(temporary, path).map_err(|error| error.to_string())
}

fn preflight_host(guest_memory_mib: u32) -> Result<(), String> {
    if unsafe { libc::geteuid() } != 0 {
        return Err(
            "run `thot sandbox run` with sudo so libvirt can create isolated KVM guests".into(),
        );
    }
    for tool in [
        "curl",
        "qemu-img",
        "virt-install",
        "virsh",
        "cloud-localds",
        "guestfish",
    ] {
        if !find_program(tool) {
            return Err(format!(
                "required host tool `{tool}` is missing; install the KVM/libvirt and libguestfs utilities"
            ));
        }
    }
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/kvm")
        .map_err(|error| format!("KVM is unavailable at /dev/kvm: {error}"))?;
    let network = run(&["virsh", "net-info", "default"])
        .map_err(|_| "libvirt default network is missing; activate the default network before running preparation".to_string())?;
    if !network.lines().any(|line| {
        line.split_once(':').is_some_and(|(key, value)| {
            key.trim().eq_ignore_ascii_case("Active") && value.trim().eq_ignore_ascii_case("yes")
        })
    }) {
        return Err(
            "libvirt default network is inactive; start it before running preparation".into(),
        );
    }
    let available_kib = fs::read_to_string("/proc/meminfo")
        .map_err(|error| error.to_string())?
        .lines()
        .find_map(|line| {
            line.strip_prefix("MemAvailable:")?
                .split_whitespace()
                .next()?
                .parse::<u64>()
                .ok()
        })
        .ok_or("cannot determine available host memory")?;
    if available_kib < u64::from(guest_memory_mib + 512) * 1024 {
        return Err(format!(
            "insufficient host memory: need at least {} MiB available",
            guest_memory_mib + 512
        ));
    }
    Ok(())
}

fn check_disk_capacity(root: &std::path::Path) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(root.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    let mut stats: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(path.as_ptr(), &mut stats) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let free = (stats.f_bavail as u64).saturating_mul(stats.f_frsize as u64);
    if free < 3 * 1024 * 1024 * 1024 {
        return Err(format!(
            "less than 3 GiB free in sandbox storage `{}`",
            root.display()
        ));
    }
    Ok(())
}

fn find_program(name: &str) -> bool {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .map(|directory| directory.join(name))
        .any(|path| path.is_file())
}

fn ensure_image(path: &std::path::Path, url: &str, expected_sha512: &str) -> Result<(), String> {
    if path.exists() {
        let digest = sha512_file(path)?;
        if digest == expected_sha512 {
            return Ok(());
        }
        return Err(format!(
            "cached image `{}` has an invalid SHA-512 digest; move it aside and retry",
            path.display()
        ));
    }
    let temporary = path.with_extension(format!("download-{}", uuid::Uuid::new_v4()));
    let output = Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--retry",
            "3",
            "--connect-timeout",
            "15",
            "--max-time",
            "240",
            url,
            "--output",
        ])
        .arg(&temporary)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        let _ = fs::remove_file(&temporary);
        return Err(format!(
            "Alpine image download failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    if sha512_file(&temporary)? != expected_sha512 {
        let _ = fs::remove_file(&temporary);
        return Err("downloaded Alpine image failed SHA-512 verification".into());
    }
    fs::rename(temporary, path).map_err(|error| error.to_string())
}

fn sha512_file(path: &std::path::Path) -> Result<String, String> {
    use std::io::Read;
    let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut digest = Sha512::new();
    let mut buffer = [0; 128 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn disk_in_use(path: &std::path::Path) -> Result<bool, String> {
    let domains = run(&["virsh", "list", "--all", "--name"])?;
    for domain in domains.lines().filter(|domain| !domain.trim().is_empty()) {
        let output = Command::new("virsh")
            .args(["domblklist", domain, "--details"])
            .output()
            .map_err(|error| format!("cannot inspect VM `{domain}` disk attachments: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "cannot inspect VM `{domain}` disk attachments: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        if String::from_utf8_lossy(&output.stdout).contains(path.to_string_lossy().as_ref()) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn wait_for_domain_shutdown(
    name: &str,
    disk: &std::path::Path,
    timeout_seconds: u32,
) -> Result<(), String> {
    let started = Instant::now();
    loop {
        let domains = run(&["virsh", "list", "--all", "--name"])?;
        if !domains.lines().any(|domain| domain.trim() == name) {
            return Ok(());
        }
        let state = match run(&["virsh", "domstate", name]) {
            Ok(state) => state,
            Err(error) => {
                let domains = run(&["virsh", "list", "--all", "--name"])?;
                if !domains.lines().any(|domain| domain.trim() == name) {
                    return Ok(());
                }
                return Err(error);
            }
        };
        if state.eq_ignore_ascii_case("shut off") {
            return Ok(());
        }
        if started.elapsed() >= Duration::from_secs(timeout_seconds as u64) {
            if disk_in_use(disk)? {
                let _ = run(&["virsh", "destroy", name]);
                let _ = run(&["virsh", "undefine", name]);
                if disk_in_use(disk)? {
                    return Err(format!(
                        "project preparation VM `{name}` still references `{}` after cleanup",
                        disk.display()
                    ));
                }
            }
            return Err(format!(
                "project preparation VM `{name}` did not shut down within {timeout_seconds}s"
            ));
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

struct RunLock {
    _file: fs::File,
}
impl RunLock {
    fn acquire(path: PathBuf) -> Result<Self, String> {
        use std::os::fd::AsRawFd;
        let file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|error| format!("cannot open run lock `{}`: {error}", path.display()))?;
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(format!(
                "another KVM Alpine run is active (lock `{}`)",
                path.display()
            ));
        }
        Ok(Self { _file: file })
    }
}

struct OwnedDomainCleanup {
    name: String,
    disk: PathBuf,
}
impl Drop for OwnedDomainCleanup {
    fn drop(&mut self) {
        if disk_in_use(&self.disk).unwrap_or(false) {
            let _ = Command::new("virsh").args(["destroy", &self.name]).status();
            let _ = Command::new("virsh")
                .args(["undefine", &self.name])
                .status();
        }
    }
}

fn project_input(project: &std::path::Path, source: &str) -> Result<PathBuf, String> {
    let relative = std::path::Path::new(source);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(format!(
            "project input path is not a safe relative path: {source}"
        ));
    }
    let canonical = project
        .join(relative)
        .canonicalize()
        .map_err(|e| format!("cannot resolve project input `{source}`: {e}"))?;
    if !canonical.starts_with(project) {
        return Err(format!("project input escapes project directory: {source}"));
    }
    Ok(canonical)
}

struct ResolvedPreparationScript {
    path: String,
    bytes: Vec<u8>,
    sha256: String,
    timeout_seconds: u32,
}

fn resolve_preparation_scripts(
    project: &std::path::Path,
    platform: &SandboxPlatform,
) -> Result<Vec<ResolvedPreparationScript>, String> {
    let mut total_bytes = 0usize;
    let mut resolved = Vec::new();
    for script in &platform.lifecycle.preparation.scripts {
        let bytes = read_project_file_bounded(project, &script.path, 1024 * 1024)?;
        total_bytes = total_bytes.saturating_add(bytes.len());
        if bytes.len() > 1024 * 1024 || total_bytes > 8 * 1024 * 1024 {
            return Err("preparation scripts exceed the 1 MiB each / 8 MiB total limit".into());
        }
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        if script
            .sha256
            .as_ref()
            .is_some_and(|expected| expected != &sha256)
        {
            return Err(format!(
                "preparation script `{}` SHA-256 does not match its configured value",
                script.path
            ));
        }
        std::str::from_utf8(&bytes).map_err(|_| {
            format!(
                "preparation script `{}` must be UTF-8 shell text",
                script.path
            )
        })?;
        resolved.push(ResolvedPreparationScript {
            path: script.path.clone(),
            bytes,
            sha256,
            timeout_seconds: script
                .timeout_seconds
                .unwrap_or(platform.lifecycle.preparation.timeout_seconds),
        });
    }
    Ok(resolved)
}

struct ResolvedTestModule {
    reference: TestModuleReference,
    manifest_bytes: Vec<u8>,
    package_bytes: Vec<u8>,
    source_path: PathBuf,
    guest_path: String,
}

fn persist_preparation_record(
    repository: &mut thot_engine::sandbox_repository::ProjectRunRepository,
    baseline_disk: &std::path::Path,
    requested_packages: &[String],
    scripts: &[ResolvedPreparationScript],
) -> Result<(), String> {
    let package_inventory =
        read_prepared_guest_file(baseline_disk, "/var/lib/thot-preparation/packages.txt")?;
    let inventory_artifact =
        repository.put_artifact(&package_inventory, "text/plain; charset=utf-8", true)?;
    let apk_stdout = repository.put_artifact(
        &read_prepared_guest_file(baseline_disk, "/var/lib/thot-preparation/apk.stdout")?,
        "text/plain; charset=utf-8",
        true,
    )?;
    let apk_stderr = repository.put_artifact(
        &read_prepared_guest_file(baseline_disk, "/var/lib/thot-preparation/apk.stderr")?,
        "text/plain; charset=utf-8",
        true,
    )?;
    let mut script_results = Vec::new();
    for (index, script) in scripts.iter().enumerate() {
        let prefix = format!("/var/lib/thot-preparation/script-{index:03}");
        let exit_code = String::from_utf8(read_prepared_guest_file(
            baseline_disk,
            &format!("{prefix}.exit"),
        )?)
        .map_err(|error| error.to_string())?
        .trim()
        .parse::<i32>()
        .map_err(|error| {
            format!(
                "invalid preparation exit status for `{}`: {error}",
                script.path
            )
        })?;
        if exit_code != 0 {
            return Err(format!(
                "preparation script `{}` failed with exit code {exit_code}",
                script.path
            ));
        }
        let stdout = repository.put_artifact(
            &read_prepared_guest_file(baseline_disk, &format!("{prefix}.stdout"))?,
            "text/plain; charset=utf-8",
            true,
        )?;
        let stderr = repository.put_artifact(
            &read_prepared_guest_file(baseline_disk, &format!("{prefix}.stderr"))?,
            "text/plain; charset=utf-8",
            true,
        )?;
        script_results.push(serde_json::json!({
            "path": script.path,
            "sha256": script.sha256,
            "timeout_seconds": script.timeout_seconds,
            "exit_code": exit_code,
            "stdout": stdout,
            "stderr": stderr
        }));
    }
    repository.append_event(
        "platform.preparation_completed",
        serde_json::json!({
            "package_manager": "apk",
            "requested_packages": requested_packages,
            "installed_package_inventory": inventory_artifact,
            "package_stdout": apk_stdout,
            "package_stderr": apk_stderr,
            "scripts": script_results
        }),
    )
}

fn read_prepared_guest_file(
    baseline_disk: &std::path::Path,
    guest_path: &str,
) -> Result<Vec<u8>, String> {
    let output = Command::new("guestfish")
        .args(["--ro", "-a"])
        .arg(baseline_disk)
        .args(["-i", "cat", guest_path])
        .output()
        .map_err(|error| format!("cannot read preparation evidence `{guest_path}`: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "preparation evidence `{guest_path}` is unavailable: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    if output.stdout.len() > 1024 * 1024 {
        return Err(format!(
            "preparation evidence `{guest_path}` exceeds the 1 MiB capture limit"
        ));
    }
    Ok(output.stdout)
}

fn resolve_test_modules(
    project: &std::path::Path,
    platform: &SandboxPlatform,
) -> Result<std::collections::BTreeMap<String, ResolvedTestModule>, String> {
    let mut modules = std::collections::BTreeMap::new();
    for test in &platform.tests {
        let Some(configured) = &test.module else {
            continue;
        };
        let manifest_bytes = read_project_file_bounded(project, &configured.manifest, 1024 * 1024)?;
        let manifest = parse_test_module_manifest(&manifest_bytes)
            .map_err(|error| format!("test `{}` module manifest is invalid: {error}", test.id))?;
        if !manifest.assets.is_empty() {
            return Err(format!(
                "test `{}` module manifest declares assets; current posix-shell-script-v1 package supports one script file only",
                test.id
            ));
        }
        let source_path = project_input(project, &configured.package)?;
        let package_bytes =
            read_project_file_bounded(project, &configured.package, 32 * 1024 * 1024)?;
        if !package_bytes.starts_with(b"#!/bin/sh\n") {
            return Err(format!(
                "test `{}` module must use the portable `#!/bin/sh` entrypoint for posix-shell-script-v1",
                test.id
            ));
        }
        manifest
            .verify_package_bytes(&package_bytes)
            .map_err(|error| {
                format!(
                    "test `{}` module package verification failed: {error}",
                    test.id
                )
            })?;
        let reference = manifest.reference().map_err(|error| error.to_string())?;
        let guest_path = format!(
            "/work/.thot/modules/{}/entrypoint.sh",
            reference.package_sha256
        );
        let module = ResolvedTestModule {
            reference,
            manifest_bytes,
            package_bytes,
            source_path,
            guest_path,
        };
        if modules.insert(test.id.clone(), module).is_some() {
            return Err(format!(
                "test `{}` has duplicate module configuration",
                test.id
            ));
        }
    }
    Ok(modules)
}

fn execution_status_name(status: ExecutionStatus) -> &'static str {
    match status {
        ExecutionStatus::Completed => "completed",
        ExecutionStatus::TimedOut => "timed_out",
        ExecutionStatus::ResourceExceeded => "resource_exceeded",
        ExecutionStatus::PlatformError => "platform_error",
        ExecutionStatus::Cancelled => "cancelled",
        ExecutionStatus::Interrupted => "interrupted",
        ExecutionStatus::Unsupported => "unsupported",
    }
}

fn load_roster(
    project: &std::path::Path,
    config: &SandboxProject,
    student_domain_prefix: &str,
    deliveries: &[DeliveryInfo],
) -> Result<Vec<(String, DeliverySelector)>, String> {
    let read_yaml = |relative: &str| -> Result<serde_yaml::Value, String> {
        let path = project_input(project, relative)?;
        let bytes = fs::read(&path).map_err(|error| error.to_string())?;
        if bytes.len() > 1024 * 1024 {
            return Err(format!(
                "project metadata file `{relative}` exceeds the 1 MiB limit"
            ));
        }
        serde_yaml::from_slice(&bytes).map_err(|error| format!("invalid `{relative}`: {error}"))
    };
    let students = read_yaml(&config.students_file)?;
    let assignments = read_yaml(&config.assignments_file)?;
    let student_list = students
        .get("students")
        .and_then(serde_yaml::Value::as_sequence)
        .ok_or("students file must contain a students list")?;
    let assignment_list = assignments
        .get("assignments")
        .and_then(serde_yaml::Value::as_sequence)
        .ok_or("assignments file must contain an assignments list")?;
    let mut ids = std::collections::HashSet::new();
    let mut domain_names = std::collections::HashSet::new();
    for student in student_list {
        let id = student
            .get("student_id")
            .and_then(serde_yaml::Value::as_str)
            .ok_or("each student must have a student_id")?;
        if id.is_empty()
            || id.len() > 64
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            || !ids.insert(id.to_owned())
        {
            return Err(format!("student ID `{id}` is invalid or duplicated"));
        }
        let domain = format!(
            "{}-{}",
            student_domain_prefix,
            id.replace(|character: char| !character.is_ascii_alphanumeric(), "-")
        );
        if domain.len() > 55 || !domain_names.insert(domain) {
            return Err(format!(
                "student ID `{id}` creates an invalid or duplicate VM name"
            ));
        }
    }
    let mut assignment_map = std::collections::HashMap::new();
    for assignment in assignment_list {
        let id = assignment
            .get("student_id")
            .and_then(serde_yaml::Value::as_str)
            .ok_or("each assignment must have a student_id")?;
        let name = assignment
            .get("deliverable")
            .and_then(serde_yaml::Value::as_str);
        let provider_id = assignment
            .get("deliverable_id")
            .and_then(serde_yaml::Value::as_str);
        let selector = match (name, provider_id) {
            (Some(_), Some(_)) | (None, None) => {
                return Err(format!(
                    "assignment for `{id}` must specify exactly one of deliverable or deliverable_id"
                ));
            }
            (Some(name), None) => {
                let matches = deliveries
                    .iter()
                    .filter(|delivery| delivery.name == name)
                    .count();
                if matches == 0 {
                    return Err(format!(
                        "deliverable `{name}` assigned to `{id}` was not found in the configured delivery source"
                    ));
                }
                if matches > 1 {
                    return Err(format!(
                        "deliverable name `{name}` is ambiguous; specify deliverable_id from the inventory"
                    ));
                }
                DeliverySelector::Name(name.to_owned())
            }
            (None, Some(provider_id)) => {
                if !deliveries
                    .iter()
                    .any(|delivery| delivery.provider_id.as_deref() == Some(provider_id))
                {
                    return Err(format!(
                        "deliverable_id `{provider_id}` assigned to `{id}` was not found in the configured delivery source"
                    ));
                }
                DeliverySelector::ProviderId(provider_id.to_owned())
            }
        };
        if !ids.contains(id) || assignment_map.insert(id.to_owned(), selector).is_some() {
            return Err(format!("assignment for `{id}` is unknown or duplicated"));
        }
    }
    if assignment_map.len() != ids.len() {
        return Err(
            "assignments file must contain exactly one deliverable for each student".into(),
        );
    }
    student_list
        .iter()
        .map(|student| {
            let id = student
                .get("student_id")
                .and_then(serde_yaml::Value::as_str)
                .ok_or("student missing student_id")?;
            let selector = assignment_map
                .remove(id)
                .ok_or_else(|| format!("student `{id}` has no deliverable"))?;
            Ok((id.to_owned(), selector))
        })
        .collect()
}

#[derive(Debug, Clone)]
enum DeliverySelector {
    Name(String),
    ProviderId(String),
}

fn read_project_file(project: &std::path::Path, relative: &str) -> Result<Vec<u8>, String> {
    let path = project_input(project, relative)?;
    let metadata =
        fs::symlink_metadata(project.join(relative)).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "project metadata `{relative}` must be a regular file"
        ));
    }
    fs::read(path).map_err(|error| format!("cannot read project metadata `{relative}`: {error}"))
}

fn read_project_file_bounded(
    project: &std::path::Path,
    relative: &str,
    maximum: u64,
) -> Result<Vec<u8>, String> {
    let path = project_input(project, relative)?;
    let metadata = fs::symlink_metadata(project.join(relative))
        .map_err(|error| format!("cannot inspect project input `{relative}`: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "project input `{relative}` must be a regular non-symlink file"
        ));
    }
    if metadata.len() > maximum {
        return Err(format!(
            "project input `{relative}` exceeds its {maximum} byte limit"
        ));
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let file = options
        .open(path)
        .map_err(|error| format!("cannot safely open project input `{relative}`: {error}"))?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(maximum.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > maximum {
        return Err(format!(
            "project input `{relative}` grew beyond its {maximum} byte limit"
        ));
    }
    Ok(bytes)
}

fn roster_result_paths(result_dir: &std::path::Path) -> Result<Vec<String>, String> {
    let mut paths = fs::read_dir(result_dir)
        .map_err(|error| error.to_string())?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let path = entry.path();
            (entry.file_name() != "run.json"
                && entry.file_type().ok().is_some_and(|kind| kind.is_file())
                && path.extension().and_then(|extension| extension.to_str()) == Some("json"))
            .then(|| path.file_name()?.to_str().map(str::to_owned))
            .flatten()
        })
        .collect::<Vec<_>>();
    paths.sort();
    if paths.is_empty() {
        return Err("evaluation completed without any student result records".into());
    }
    Ok(paths)
}

fn yaml_quote(value: &str) -> String {
    serde_yaml::to_string(value).unwrap().trim().to_string()
}

fn new_session_token() -> Result<String, String> {
    use std::io::Read;
    let mut bytes = [0u8; 32];
    fs::File::open("/dev/urandom")
        .and_then(|mut source| source.read_exact(&mut bytes))
        .map_err(|e| format!("cannot create session secret: {e}"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn change_owner(path: &std::path::Path, uid: u32, gid: u32) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    if unsafe { libc::chown(path.as_ptr(), uid, gid) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
}

struct TemporaryDirectory(PathBuf);

impl TemporaryDirectory {
    fn create() -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!("thot-delivery-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).map_err(|error| error.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
                .map_err(|error| error.to_string())?;
        }
        Ok(Self(path))
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct StudentCleanup {
    domain: String,
    disk: PathBuf,
    seed: Option<PathBuf>,
    work: Option<PathBuf>,
}
impl Drop for StudentCleanup {
    fn drop(&mut self) {
        match disk_in_use(&self.disk) {
            Ok(false) => {}
            Ok(true) => {
                let _ = Command::new("virsh")
                    .args(["destroy", &self.domain])
                    .status();
                let _ = Command::new("virsh")
                    .args(["undefine", &self.domain, "--nvram"])
                    .status();
                match disk_in_use(&self.disk) {
                    Ok(false) => {}
                    Ok(true) => {
                        eprintln!(
                            "cleanup warning: leaving VM disk and work files because domain `{}` still references `{}`",
                            self.domain,
                            self.disk.display()
                        );
                        return;
                    }
                    Err(error) => {
                        eprintln!(
                            "cleanup warning: leaving VM disk and work files because attachment state for `{}` could not be checked: {error}",
                            self.disk.display()
                        );
                        return;
                    }
                }
            }
            Err(error) => {
                eprintln!(
                    "cleanup warning: leaving VM disk and work files because attachment state for `{}` could not be checked: {error}",
                    self.disk.display()
                );
                return;
            }
        }
        let _ = fs::remove_file(&self.disk);
        if let Some(path) = &self.seed {
            let _ = fs::remove_file(path);
        }
        if let Some(path) = &self.work {
            let _ = fs::remove_dir_all(path);
        }
    }
}

fn vsock_listener(port: u16) -> Result<std::fs::File, String> {
    use std::os::fd::FromRawFd;
    let fd = unsafe { libc::socket(libc::AF_VSOCK, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let addr = libc::sockaddr_vm {
        svm_family: libc::AF_VSOCK as libc::sa_family_t,
        svm_reserved1: 0,
        svm_port: port as u32,
        svm_cid: libc::VMADDR_CID_ANY,
        svm_zero: [0; 4],
    };
    let timeout = libc::timeval {
        tv_sec: 180,
        tv_usec: 0,
    };
    let _ = unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_RCVTIMEO,
            &timeout as *const _ as *const libc::c_void,
            std::mem::size_of_val(&timeout) as libc::socklen_t,
        )
    };
    let ok = unsafe {
        libc::bind(
            fd,
            &addr as *const _ as *const libc::sockaddr,
            std::mem::size_of_val(&addr) as libc::socklen_t,
        )
    } == 0
        && unsafe { libc::listen(fd, 1) } == 0;
    if !ok {
        let e = std::io::Error::last_os_error();
        unsafe { libc::close(fd) };
        return Err(format!("cannot listen on VSOCK: {e}"));
    }
    Ok(unsafe { std::fs::File::from_raw_fd(fd) })
}

fn accept_vsock(listener: &std::fs::File) -> Result<(std::fs::File, u32), String> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let mut addr: libc::sockaddr_vm = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of_val(&addr) as libc::socklen_t;
    let fd = unsafe {
        libc::accept4(
            listener.as_raw_fd(),
            &mut addr as *mut _ as *mut libc::sockaddr,
            &mut len,
            libc::SOCK_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok((unsafe { std::fs::File::from_raw_fd(fd) }, addr.svm_cid))
}

struct StagingBudget {
    bytes: u64,
    files: usize,
    max_bytes: u64,
    max_files: usize,
}

impl StagingBudget {
    fn new(max_bytes: u64, max_files: usize) -> Self {
        Self {
            bytes: 0,
            files: 0,
            max_bytes,
            max_files,
        }
    }
}

fn stage_input(
    stream: &mut std::fs::File,
    source: &std::path::Path,
    destination: &str,
    budget: &mut StagingBudget,
) -> Result<(), String> {
    let metadata = fs::symlink_metadata(source).map_err(|e| e.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err(format!("refusing to stage symlink `{}`", source.display()));
    }
    if metadata.is_file() {
        budget.bytes = budget.bytes.saturating_add(metadata.len());
        budget.files += 1;
        if budget.bytes > budget.max_bytes || budget.files > budget.max_files {
            return Err(format!(
                "staged project inputs exceed the configured {} byte or {} file limit",
                budget.max_bytes, budget.max_files
            ));
        }
        return stage_path(stream, source, destination);
    }
    if !metadata.is_dir() {
        return Err(format!("unsupported project input `{}`", source.display()));
    }
    let mut entries = fs::read_dir(source)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let child_destination = format!(
            "{}/{}",
            destination.trim_end_matches('/'),
            entry.file_name().to_string_lossy()
        );
        stage_input(stream, &entry.path(), &child_destination, budget)?;
    }
    Ok(())
}

fn stage_path(
    stream: &mut std::fs::File,
    source: &std::path::Path,
    destination: &str,
) -> Result<(), String> {
    use std::io::Read;
    const CHUNK_BYTES: usize = 64 * 1024;
    let metadata = fs::symlink_metadata(source).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "input `{}` is not a regular non-symlink file",
            source.display()
        ));
    }
    let file_size = metadata.len();
    let mut input = open_staging_source(source)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; CHUNK_BYTES];
    loop {
        let count = input.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    let digest = format!("{:x}", hasher.finalize());
    let mut input = open_staging_source(source)?;
    let transfer_id = uuid::Uuid::new_v4().to_string();
    let mut chunk_index = 0u64;
    let mut sent = 0u64;
    loop {
        let count = input.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 && (file_size != 0 || chunk_index > 0) {
            break;
        }
        sent = sent.saturating_add(count as u64);
        let final_chunk = sent == file_size;
        let request = StageFileChunkRequest {
            task_id: transfer_id.clone(),
            file_path: destination.to_string(),
            total_size: file_size,
            expected_sha256: digest.clone(),
            chunk_index,
            final_chunk,
            bytes: buffer[..count].to_vec(),
        };
        let task_id = request.task_id.clone();
        write_frame(stream, &AgentRequest::StageFileChunk(request)).map_err(|e| e.to_string())?;
        let response: AgentResponse = read_frame(stream).map_err(|e| e.to_string())?;
        match response {
            AgentResponse::StageFileChunkResult(r)
                if r.task_id == task_id
                    && r.complete == final_chunk
                    && (!final_chunk || r.sha256.as_deref() == Some(digest.as_str())) => {}
            _ => return Err("unexpected staging response".into()),
        }
        chunk_index += 1;
        if final_chunk {
            break;
        }
    }
    if sent != file_size {
        return Err(format!(
            "input `{}` changed size during transfer",
            source.display()
        ));
    }
    Ok(())
}

fn open_staging_source(source: &std::path::Path) -> Result<fs::File, String> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    options.open(source).map_err(|error| {
        format!(
            "cannot safely open staged input {}: {error}",
            source.display()
        )
    })
}

fn dependency_has_pending_result(
    dependencies: &[String],
    results: &[(String, bool, serde_json::Value)],
) -> bool {
    dependencies.iter().any(|dependency| {
        results
            .iter()
            .any(|(id, _, result)| id == dependency && result["assessment"] == "pending")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_project_uses_the_shared_v2_schema_and_passes_validation() {
        let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/kvm-alpine-poc")
            .canonicalize()
            .unwrap();
        let bytes = fs::read(project_root.join("configuration.thot.yaml")).unwrap();
        let project: SandboxProject = serde_yaml::from_slice(&bytes).unwrap();
        project.validate().unwrap();
        let platform = &project.sandbox_platforms[0];
        let preparation = resolve_preparation_scripts(&project_root, platform).unwrap();
        assert_eq!(preparation.len(), 1);
        assert_eq!(preparation[0].path, "extern/preparation/verify-bash.sh");
        let modules = resolve_test_modules(&project_root, platform).unwrap();
        let module = modules.get("portable-module-sum").unwrap();
        assert_eq!(module.reference.module_id, "thot.smoke.sum-check");
        assert_eq!(module.reference.package_sha256.len(), 64);
    }

    #[test]
    fn project_inputs_reject_parent_paths_and_outside_symlinks() {
        let root = std::env::temp_dir().join(format!("thot-sandbox-path-{}", uuid::Uuid::new_v4()));
        let outside =
            std::env::temp_dir().join(format!("thot-sandbox-outside-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(&outside, b"outside").unwrap();
        assert!(project_input(&root, "../escape").is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, root.join("escape")).unwrap();
            assert!(project_input(&root, "escape").is_err());
        }
        let _ = fs::remove_file(outside);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn dependency_skip_stays_pending_when_the_dependency_failed_infrastructure() {
        let results = vec![(
            "disk-inventory".into(),
            false,
            serde_json::json!({"assessment":"pending","status":"platform_error"}),
        )];
        assert!(dependency_has_pending_result(
            &["disk-inventory".into()],
            &results
        ));
        assert!(!dependency_has_pending_result(&["other".into()], &results));
    }
}
