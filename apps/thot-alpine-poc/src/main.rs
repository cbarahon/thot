use sha2::{Digest, Sha256, Sha512};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};
use thot_core::{sandbox_project::*, sandbox_protocol::*};
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
    let input_root = default_student_input_root(&config);
    let mut delivery_agent = build_agent(
        config.delivery_agent.as_ref(),
        &project,
        config.delivery_folder.as_deref(),
        input_root.as_deref(),
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
    let prep_key = format!(
        "{}:{}:{}:{}:{}:{}:{:?}:{:?}:{}",
        config.project_id,
        p.id,
        base_manifest,
        p.lifecycle.template_vm.vcpu,
        p.lifecycle.template_vm.memory_mib,
        p.lifecycle.project_clone.name,
        p.lifecycle.preparation.packages,
        p.lifecycle.preparation.scripts,
        p.network.preparation
    );
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
        let mut preparation_script = String::from("#!/bin/sh\nset -eu\n");
        if !p.lifecycle.preparation.packages.is_empty() {
            preparation_script.push_str("apk add --no-progress");
            for package in &p.lifecycle.preparation.packages {
                preparation_script.push(' ');
                preparation_script.push_str(package);
            }
            preparation_script.push('\n');
        }
        for script in &p.lifecycle.preparation.scripts {
            preparation_script.push_str(script);
            preparation_script.push('\n');
        }
        preparation_script.push_str("touch /var/lib/thot-preparation-complete\nsync\npoweroff\n");
        let encoded_script = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            preparation_script,
        );
        let userdata = format!(
            "#cloud-config\nwrite_files:\n  - path: /var/lib/thot-prepare.sh\n    permissions: '0700'\n    encoding: b64\n    content: {encoded_script}\nruncmd:\n  - [/bin/sh, /var/lib/thot-prepare.sh]\n"
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
    let evaluation_id = uuid::Uuid::new_v4().to_string();
    let results_root = project.join("results");
    let result_dir = results_root.join(&evaluation_id);
    fs::create_dir_all(&result_dir).map_err(|e| e.to_string())?;
    if let (Ok(uid), Ok(gid)) = (std::env::var("SUDO_UID"), std::env::var("SUDO_GID"))
        && let (Ok(uid), Ok(gid)) = (uid.parse::<u32>(), gid.parse::<u32>())
    {
        change_owner(&results_root, uid, gid)?;
        change_owner(&result_dir, uid, gid)?;
    }
    let mut domain_names = std::collections::HashSet::new();
    for (id, deliverable) in roster {
        let intake = TemporaryDirectory::create()?;
        let acquired = delivery_agent
            .get(&deliverable, &intake.0.join("submission"))
            .map_err(|error| error.to_string())?;
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
        let run_id = uuid::Uuid::new_v4().to_string();
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
        match response {
            AgentResponse::Handshake(answer)
                if answer.protocol_version == ALPINE_PROTOCOL_VERSION
                    && answer.nonce == handshake.nonce => {}
            _ => return Err("agent handshake rejected".into()),
        }
        let mut staging_budget = StagingBudget::default();
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
        let mut test_results: Vec<(String, bool, serde_json::Value)> = Vec::new();
        let mut test_json = Vec::new();
        for test in &p.tests {
            let blocked = test.requires.iter().any(|dependency| {
                !test_results
                    .iter()
                    .any(|(id, ok, _)| id == dependency && *ok)
            });
            if blocked {
                let skipped = serde_json::json!({"status":"skipped","assessment":"skipped","reason":"required test did not pass"});
                test_json.push(serde_json::json!({"test_id":test.id,"result":skipped}));
                test_results.push((test.id.clone(), false, skipped));
                continue;
            }
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
            };
            write_frame(&mut stream, &AgentRequest::ExecuteTask(request.clone()))
                .map_err(|e| e.to_string())?;
            let response: AgentResponse = read_frame(&mut stream).map_err(|e| e.to_string())?;
            let result = match response {
                AgentResponse::ExecuteTaskResult(r) => r,
                _ => return Err("unexpected agent response".into()),
            };
            let passed = thot_engine::sandbox::evaluate_sandbox_test(
                result.exit_code,
                result.timed_out,
                test.expect_exit_code,
                test.expected_stdout.as_deref(),
                &result.stdout,
            );
            let v = serde_json::json!({"status":if result.timed_out{"timed_out"}else{"completed"},"assessment":if passed{"passed"}else{"failed"},"exit_code":result.exit_code,"stdout":result.stdout,"stderr":result.stderr,"duration_ms":result.elapsed_ms,"truncated":result.truncated,"diagnostics":result.diagnostics});
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
        let criteria = p
            .evaluation
            .rubric
            .criteria
            .iter()
            .map(|criterion| (criterion.check.clone(), criterion.points))
            .collect::<Vec<_>>();
        let evaluated =
            thot_engine::sandbox::evaluate_sandbox_rubric(&checks, &test_passed, &criteria)?;
        let grade = evaluated
            .iter()
            .map(|criterion| criterion.earned_points)
            .sum::<u32>();
        let earned = evaluated.iter().map(|criterion| serde_json::json!({"check":criterion.check,"points":criterion.earned_points,"maximum":criterion.maximum_points})).collect::<Vec<_>>();
        let report = serde_json::json!({"schema_version":"thot.result/v2","student_id":id,"run_id":run_id,"platform":"kvm-alpine","deliverable":{"name":acquired.info.name,"size_bytes":acquired.info.size_bytes,"created_at":acquired.info.created_at,"modified_at":acquired.info.modified_at,"sha256":acquired.sha256},"tests":test_json,"criteria":earned,"grade":grade,"scale":p.evaluation.rubric.scale});
        let result_path = result_dir.join(format!("{id}.json"));
        fs::write(&result_path, serde_json::to_vec_pretty(&report).unwrap())
            .map_err(|e| e.to_string())?;
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
        drop(cleanup);
        println!("{id}: {grade}/10");
    }
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

fn load_roster(
    project: &std::path::Path,
    config: &SandboxProject,
    student_domain_prefix: &str,
    deliveries: &[DeliveryInfo],
) -> Result<Vec<(String, String)>, String> {
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
    let available = deliveries
        .iter()
        .map(|delivery| delivery.name.as_str())
        .collect::<Vec<_>>();
    let mut unique_names = std::collections::HashSet::new();
    if available.iter().any(|name| !unique_names.insert(*name)) {
        return Err(
            "delivery agent returned duplicate names; assignments require unique deliverable names"
                .into(),
        );
    }
    for assignment in assignment_list {
        let id = assignment
            .get("student_id")
            .and_then(serde_yaml::Value::as_str)
            .ok_or("each assignment must have a student_id")?;
        let path = assignment
            .get("deliverable")
            .and_then(serde_yaml::Value::as_str)
            .ok_or_else(|| format!("assignment for `{id}` must have a deliverable name"))?;
        if !available.contains(&path) {
            return Err(format!(
                "deliverable `{path}` assigned to `{id}` was not found in the configured delivery source"
            ));
        }
        if !ids.contains(id)
            || assignment_map
                .insert(id.to_owned(), path.to_owned())
                .is_some()
        {
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
            let path = assignment_map
                .remove(id)
                .ok_or_else(|| format!("student `{id}` has no deliverable"))?;
            Ok((id.to_owned(), path))
        })
        .collect()
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

#[derive(Default)]
struct StagingBudget {
    bytes: u64,
    files: usize,
}

fn stage_input(
    stream: &mut std::fs::File,
    source: &std::path::Path,
    destination: &str,
    budget: &mut StagingBudget,
) -> Result<(), String> {
    const MAX_TOTAL: u64 = 128 * 1024 * 1024;
    const MAX_FILES: usize = 10_000;
    let metadata = fs::symlink_metadata(source).map_err(|e| e.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err(format!("refusing to stage symlink `{}`", source.display()));
    }
    if metadata.is_file() {
        budget.bytes = budget.bytes.saturating_add(metadata.len());
        budget.files += 1;
        if budget.bytes > MAX_TOTAL || budget.files > MAX_FILES {
            return Err("staged project inputs exceed the 128 MiB or 10,000 file limit".into());
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
    const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;
    let file_size = fs::metadata(source).map_err(|e| e.to_string())?.len();
    if file_size > MAX_FILE_BYTES {
        return Err(format!(
            "input `{}` exceeds the 10 MiB per-file limit",
            source.display()
        ));
    }
    let bytes = fs::read(source).map_err(|e| e.to_string())?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let chunks: Vec<&[u8]> = bytes.chunks(64 * 1024).collect();
    let chunks = if chunks.is_empty() {
        vec![&[][..]]
    } else {
        chunks
    };
    let transfer_id = uuid::Uuid::new_v4().to_string();
    for (index, chunk) in chunks.iter().enumerate() {
        let request = StageFileChunkRequest {
            task_id: transfer_id.clone(),
            file_path: destination.to_string(),
            total_size: bytes.len() as u64,
            expected_sha256: digest.clone(),
            chunk_index: index as u64,
            final_chunk: index + 1 == chunks.len(),
            bytes: chunk.to_vec(),
        };
        let task_id = request.task_id.clone();
        write_frame(stream, &AgentRequest::StageFileChunk(request)).map_err(|e| e.to_string())?;
        let response: AgentResponse = read_frame(stream).map_err(|e| e.to_string())?;
        match response {
            AgentResponse::StageFileChunkResult(r) if r.task_id == task_id => {}
            _ => return Err("unexpected staging response".into()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_project_uses_the_shared_v2_schema_and_passes_validation() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/kvm-alpine-poc/configuration.thot.yaml");
        let bytes = fs::read(path).unwrap();
        let project: SandboxProject = serde_yaml::from_slice(&bytes).unwrap();
        project.validate().unwrap();
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
}
