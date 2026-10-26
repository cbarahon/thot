//! Purpose: Implements VM host preflight, delivery intake, and manifest-scoped lifecycle utilities.

use chrono::{SecondsFormat, Utc};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio::{process::Command, time::sleep};

#[derive(Debug, Clone)]
pub struct VmActivityRuntime {
    pub vbox_manage: PathBuf,
    pub expected_vbox_version: String,
    pub vbox_user_home: PathBuf,
    pub storage_root: PathBuf,
    pub artifact_root: PathBuf,
    pub max_ova_bytes: u64,
    pub free_space_reserve: u64,
    pub output_limit: usize,
    pub import_timeout: Duration,
    pub startup_timeout: Duration,
    pub ssh_timeout: Duration,
    pub shutdown_grace: Duration,
    pub ssh_secret_file: PathBuf,
    pub firewall_confirmed: bool,
}

#[derive(Debug, Clone)]
pub struct ProjectSnapshot {
    pub project_id: String,
    pub bytes: Vec<u8>,
    pub sha256: String,
    pub ssh_username: String,
    pub archive_path: PathBuf,
    pub archive_parent: PathBuf,
    pub extracted_root: PathBuf,
    pub configuration: thot_core::VMActivityProject,
}

pub async fn load_project(
    project_file: &Path,
    snapshot_dir: &Path,
) -> Result<ProjectSnapshot, String> {
    let path = project_file.canonicalize().map_err(|e| {
        format!(
            "project file {} is unavailable: {e}",
            project_file.display()
        )
    })?;
    let metadata = fs::metadata(&path)
        .map_err(|e| format!("cannot inspect project {}: {e}", path.display()))?;
    if !metadata.is_file() {
        return Err("project archive must be a regular file".into());
    }
    if !path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".thot.tgz") || name.ends_with(".thot.zip"))
    {
        return Err("project file extension must be .thot.tgz or .thot.zip".into());
    }
    let bytes = fs::read(&path)
        .map_err(|e| format!("cannot read project archive {}: {e}", path.display()))?;
    let snapshot_archive = {
        fs::create_dir_all(snapshot_dir).map_err(|e| e.to_string())?;
        let retained = snapshot_dir.join(path.file_name().unwrap());
        atomic_write(&retained, &bytes)
            .map_err(|e| format!("cannot retain project snapshot: {e}"))?;
        retained
    };
    let configuration_bytes = {
        let extracted = snapshot_dir.join("project");
        crate::project_archive::extract_project_archive(&snapshot_archive, &extracted)?;
        fs::read(extracted.join("configuration.yaml")).map_err(|e| e.to_string())?
    };
    let configuration = crate::project_config::parse_configuration(&configuration_bytes)?;
    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    let extracted_root = snapshot_dir.join("project");
    let ssh_username = configuration
        .tests
        .iter()
        .find(|test| test.test_ref == "ssh/get_host_info")
        .and_then(|test| test.parameters.into_strings().ok())
        .and_then(|params| params.get("user").cloned())
        .unwrap_or_else(|| "tvadmin".to_owned());
    let archive_parent = path
        .parent()
        .ok_or("project archive has no containing directory")?
        .canonicalize()
        .map_err(|e| format!("cannot resolve project archive directory: {e}"))?;
    let project_id = project_identifier(&path);
    Ok(ProjectSnapshot {
        project_id,
        bytes,
        sha256,
        ssh_username,
        archive_parent,
        archive_path: path,
        extracted_root,
        configuration,
    })
}

fn project_identifier(path: &Path) -> String {
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("project");
    let stem = filename
        .strip_suffix(".thot.tgz")
        .or_else(|| filename.strip_suffix(".thot.zip"))
        .unwrap_or("project");
    let slug = stem
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .take(40)
        .collect::<String>()
        .trim_matches('_')
        .to_owned();
    let slug = if slug.is_empty() { "project" } else { &slug };
    let digest = Sha256::digest(path.as_os_str().to_string_lossy().as_bytes());
    let suffix = digest[..6]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("{slug}-{suffix}")
}

pub async fn preflight(runtime: &VmActivityRuntime) -> Result<String, String> {
    if !runtime.firewall_confirmed {
        return Err("THOTD_FIREWALL_CONFIRMATION must be true after the host firewall and guest egress policy are verified".into());
    }
    if !cfg!(target_os = "linux") {
        return Err("the VirtualBox POC adapter currently supports Linux hosts only".into());
    }
    if runtime.expected_vbox_version.is_empty() {
        return Err("THOTD_EXPECTED_VBOX_VERSION must pin the qualified VirtualBox version".into());
    }
    if !runtime.vbox_user_home.is_absolute()
        || !runtime.storage_root.is_absolute()
        || !runtime.artifact_root.is_absolute()
    {
        return Err("VBOX_USER_HOME, storage, and artifact paths must be absolute".into());
    }
    for path in [
        &runtime.vbox_user_home,
        &runtime.storage_root,
        &runtime.artifact_root,
    ] {
        fs::create_dir_all(path).map_err(|e| format!("{} is unavailable: {e}", path.display()))?;
    }
    let profile = runtime
        .vbox_user_home
        .canonicalize()
        .map_err(|e| format!("dedicated VBOX_USER_HOME is unavailable: {e}"))?;
    let storage = runtime
        .storage_root
        .canonicalize()
        .map_err(|e| format!("VM storage root is unavailable: {e}"))?;
    let artifacts = runtime
        .artifact_root
        .canonicalize()
        .map_err(|e| format!("artifact root is unavailable: {e}"))?;
    if paths_overlap(&profile, &storage)
        || paths_overlap(&profile, &artifacts)
        || paths_overlap(&storage, &artifacts)
    {
        return Err("VirtualBox profile, VM storage, and artifact storage must be separate, non-overlapping paths".into());
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        let default_profiles = [home.join(".VirtualBox"), home.join(".config/VirtualBox")];
        if default_profiles
            .iter()
            .any(|path| path.canonicalize().is_ok_and(|p| p == profile))
        {
            return Err("VBOX_USER_HOME must be a dedicated profile, not the service account's default VirtualBox profile".into());
        }
    }
    if !runtime.vbox_manage.is_file() {
        return Err(format!(
            "VBoxManage not found at {}",
            runtime.vbox_manage.display()
        ));
    }
    for port in [8080, 2222] {
        TcpListener::bind(("127.0.0.1", port))
            .map_err(|_| format!("required loopback port {port} is unavailable"))?;
    }
    if let Some(entry) = fs::read_dir(&storage)
        .map_err(|e| format!("cannot inspect dedicated storage root: {e}"))?
        .next()
    {
        let entry = entry.map_err(|e| e.to_string())?;
        return Err(format!(
            "dedicated storage root contains an unaccounted entry: {}",
            entry.path().display()
        ));
    }
    let version = vbox(runtime, &["--version"], Duration::from_secs(10)).await?;
    if version.exit_code != Some(0) {
        return Err(format!("VBoxManage --version failed: {}", version.stderr));
    }
    let version_number = version.stdout.trim().split('r').next().unwrap_or_default();
    if version_number != runtime.expected_vbox_version {
        return Err(format!(
            "VirtualBox version {version_number} differs from qualified version {}",
            runtime.expected_vbox_version
        ));
    }
    let inventory = vbox(runtime, &["list", "vms"], Duration::from_secs(15)).await?;
    if inventory.exit_code != Some(0) {
        return Err(format!(
            "cannot inspect the dedicated VirtualBox registry: {}",
            inventory.stderr
        ));
    }
    if !inventory.stdout.trim().is_empty() {
        return Err("dedicated VirtualBox profile contains unaccounted VMs; resolve them before starting a run".into());
    }
    Ok(version.stdout.trim().to_owned())
}

#[derive(Debug, Clone)]
pub struct DeliveryInput {
    pub delivery_id: String,
    pub source: PathBuf,
    pub original_source: PathBuf,
    pub filename: String,
    pub size: u64,
    pub estimated_import_bytes: u64,
    pub sha256: Option<String>,
    pub validation_error: Option<String>,
}

pub fn discover_deliveries(directory: &Path, max_size: u64) -> Result<Vec<DeliveryInput>, String> {
    let mut entries = fs::read_dir(directory)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    entries.sort_by_key(|entry| entry.file_name());
    let mut found = Vec::new();
    for entry in entries {
        let path = entry.path();
        if path.extension().and_then(|v| v.to_str()) != Some("ova") {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        let mut error = None;
        let mut estimated_import_bytes = 0;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            error = Some("OVA must be a regular file, not a symlink".into());
        }
        let size = metadata.len();
        if error.is_none() && (size == 0 || size > max_size) {
            error = Some(format!("OVA size {size} is outside the configured limit"));
        }
        let digest = if !metadata.file_type().is_symlink() && metadata.is_file() {
            let mut hasher = Sha256::new();
            let mut file = File::open(&path).map_err(|e| e.to_string())?;
            let mut buffer = [0u8; 128 * 1024];
            loop {
                let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
                if count == 0 {
                    break;
                }
                hasher.update(&buffer[..count]);
            }
            Some(format!("{:x}", hasher.finalize()))
        } else {
            None
        };
        if error.is_none() {
            let mut file = File::open(&path).map_err(|e| e.to_string())?;
            let mut header = [0u8; 512];
            if file.read(&mut header).unwrap_or(0) < 512 || &header[257..262] != b"ustar" {
                error = Some("file does not contain a recognizable OVA tar header".into());
            } else {
                match estimate_ova_disk_bytes(&path, size) {
                    Ok(Some(bytes)) => estimated_import_bytes = bytes,
                    Ok(None) => error = Some("OVA archive contains no virtual disk image".into()),
                    Err(reason) => error = Some(reason),
                }
            }
        }
        let stable = format!(
            "{:x}",
            Sha256::digest(
                format!(
                    "{}\0{}\0{}\0{}",
                    directory.display(),
                    name,
                    digest.as_deref().unwrap_or("rejected"),
                    size
                )
                .as_bytes()
            )
        );
        found.push(DeliveryInput {
            delivery_id: format!("delivery-{}", &stable[..16]),
            source: path.clone(),
            original_source: path,
            filename: name,
            size,
            estimated_import_bytes,
            sha256: digest,
            validation_error: error,
        });
    }
    Ok(found)
}

fn estimate_ova_disk_bytes(path: &Path, archive_len: u64) -> Result<Option<u64>, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut offset = 0u64;
    let mut disk_bytes = 0u64;
    let mut header = [0u8; 512];
    while offset.saturating_add(512) <= archive_len {
        file.seek(SeekFrom::Start(offset))
            .map_err(|e| e.to_string())?;
        file.read_exact(&mut header)
            .map_err(|e| format!("truncated OVA tar header: {e}"))?;
        if header.iter().all(|byte| *byte == 0) {
            break;
        }
        let raw_size = &header[124..136];
        let size_text = raw_size
            .iter()
            .copied()
            .take_while(|byte| *byte != 0)
            .collect::<Vec<_>>();
        let size_text = String::from_utf8_lossy(&size_text).trim().to_owned();
        let member_size = u64::from_str_radix(
            match size_text.trim_start_matches('0') {
                "" => "0",
                value => value,
            },
            8,
        )
        .map_err(|_| "OVA contains a malformed tar member size".to_string())?;
        let member_end = offset
            .checked_add(512)
            .and_then(|value| value.checked_add(member_size))
            .ok_or_else(|| "OVA tar member size overflows the supported range".to_string())?;
        if member_end > archive_len {
            return Err("OVA tar member extends beyond the archive".into());
        }
        let name = String::from_utf8_lossy(&header[0..100])
            .trim_matches(char::from(0))
            .to_owned();
        let prefix = String::from_utf8_lossy(&header[345..500])
            .trim_matches(char::from(0))
            .to_owned();
        let full_name = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if full_name.to_ascii_lowercase().ends_with(".vmdk") && matches!(header[156], 0 | b'0') {
            disk_bytes = disk_bytes.checked_add(member_size).ok_or_else(|| {
                "estimated OVA disk footprint overflows the supported range".to_string()
            })?;
        }
        let padded_size = member_size
            .checked_add(511)
            .and_then(|value| value.checked_div(512))
            .and_then(|value| value.checked_mul(512))
            .ok_or_else(|| "OVA tar member size overflows the supported range".to_string())?;
        offset = offset
            .checked_add(512)
            .and_then(|value| value.checked_add(padded_size))
            .ok_or_else(|| "OVA tar offset overflows the supported range".to_string())?;
    }
    Ok((disk_bytes > 0).then_some(disk_bytes))
}

pub async fn retain_source(
    runtime: &VmActivityRuntime,
    input: &DeliveryInput,
) -> Result<PathBuf, String> {
    let digest = input
        .sha256
        .as_deref()
        .ok_or_else(|| "rejected source is not copied into the retained OVA store".to_string())?;
    let root = runtime.artifact_root.join("sources");
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let target = root.join(format!("{}.ova", input.delivery_id));
    if target.exists() {
        let target_meta = fs::symlink_metadata(&target).map_err(|e| e.to_string())?;
        if target_meta.file_type().is_symlink() || !target_meta.is_file() {
            return Err("retained OVA path is not a regular file".into());
        }
        let mut file = File::open(&target).map_err(|e| e.to_string())?;
        let mut hasher = Sha256::new();
        let mut block = [0u8; 131072];
        loop {
            let n = file.read(&mut block).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            hasher.update(&block[..n]);
        }
        if format!("{:x}", hasher.finalize()) == digest {
            return Ok(target);
        }
        return Err(
            "retained source artifact exists but its digest does not match the delivery".into(),
        );
    }
    let available = available_bytes(&root).await.unwrap_or(0);
    if available < input.size.saturating_add(runtime.free_space_reserve) {
        return Err("artifact storage does not have room for the immutable source OVA and configured free-space reserve".into());
    }
    let temp = root.join(format!(".{}.{}.tmp", input.delivery_id, std::process::id()));
    tokio::fs::copy(&input.original_source, &temp)
        .await
        .map_err(|e| format!("could not copy source OVA into immutable artifact storage: {e}"))?;
    let mut file = File::open(&temp).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut block = [0u8; 131072];
    loop {
        let n = file.read(&mut block).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&block[..n]);
    }
    if format!("{:x}", hasher.finalize()) != digest {
        let _ = fs::remove_file(&temp);
        return Err("source OVA changed while it was being retained".into());
    }
    let mut permissions = fs::metadata(&temp)
        .map_err(|e| e.to_string())?
        .permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&temp, permissions).map_err(|e| e.to_string())?;
    fs::rename(temp, &target).map_err(|e| e.to_string())?;
    Ok(target)
}

pub async fn execute_delivery(
    runtime: &VmActivityRuntime,
    project: &ProjectSnapshot,
    virtualbox_version: &str,
    run_id: &str,
    expedient_id: &str,
    input: &DeliveryInput,
    report_dir: &Path,
    report_path: &Path,
) -> Value {
    let ids = [
        "OVA-001",
        "VBOX-001",
        "VBOX-002",
        "WEB-001",
        "WEB-002",
        "WEB-003",
        "VBOX-003",
        "SSH-001",
        "CLEAN-001",
    ]
    .iter()
    .map(|test| (*test, format!("result-{}", uuid::Uuid::new_v4().simple())))
    .collect::<std::collections::HashMap<_, _>>();
    let now = now_string();
    let mut report = json!({
        "schema_version":1,"project_id":project.project_id,"delivery_id":input.delivery_id,"expedient_id":expedient_id,
        "run_id":run_id,"execution_context":{"project_schema_version":1,"project_snapshot_ref":"../configuration/project.yaml",
          "project_config_sha256":project.sha256,"engine_version":env!("CARGO_PKG_VERSION"),"host_os":std::env::consts::OS,"virtualbox_version":virtualbox_version},
        "source":{"filename":input.filename,"size_bytes":input.size,"estimated_import_bytes":input.estimated_import_bytes,"sha256":input.sha256,"source_ref":input.source.display().to_string(),"original_source_ref":input.original_source.display().to_string()},
        "vm":{"name":null,"uuid":null,"group":"/thot","ram_mb":null,"cpus":null,"disks":[],"network_adapters":[]},
        "web_startup_trials":[],"screenshot":{"status":"not_run","path":null,"captured_at":null},
        "guest_filesystems":{"status":"not_run","command":"LC_ALL=C df -B1","filesystems":[]},
        "cleanup":{"status":"not_run","vm_unregistered":false,"run_storage_removed":false,"details":[]},
        "test_results":[],"overall_status":"running","started_at":now,"finished_at":null
    });
    for test in [
        "OVA-001",
        "VBOX-001",
        "VBOX-002",
        "WEB-001",
        "WEB-002",
        "WEB-003",
        "VBOX-003",
        "SSH-001",
        "CLEAN-001",
    ] {
        report["test_results"].as_array_mut().unwrap().push(json!({"test_result_id":ids[test],"test_id":test,"status":"not_run","start_time":null,"end_time":null,"duration_ms":null,"diagnostics":null,"observation":{},"evidence":[]}));
    }
    let vm_name = format!(
        "thot-{}-{}",
        &run_id[..run_id.len().min(8)],
        &input.delivery_id[input.delivery_id.len().saturating_sub(12)..]
    );
    let vm_folder = runtime.storage_root.join(run_id).join(&input.delivery_id);
    let _ = fs::create_dir_all(&vm_folder);
    let _ = fs::create_dir_all(report_dir);
    let mut vm_uuid: Option<String> = None;
    let mut disk_paths = Vec::<String>::new();
    let mut import_attempted = false;
    let mut cleanup_errors = Vec::new();
    let _ = fs::create_dir_all(report_dir.join("logs"));
    let manifest_path = report_dir.join("cleanup-manifest.json");
    let _ = atomic_write(&manifest_path,&serde_json::to_vec_pretty(&json!({"schema_version":1,"run_id":run_id,"delivery_id":input.delivery_id,"vm_name":vm_name,"vm_uuid":null,"disk_paths":[],"may_be_registered":false,"storage_path":vm_folder.display().to_string()})).unwrap_or_default());
    start_result(&mut report, "OVA-001");
    set_result(
        &mut report,
        "OVA-001",
        if input.validation_error.is_some() {
            "failed"
        } else {
            "passed"
        },
        json!({"source_sha256":input.sha256,"source_ref":input.source.display().to_string()}),
        input.validation_error.clone(),
    );
    persist(&report, report_path);

    if input.validation_error.is_none() {
        let disk_available = available_bytes(&vm_folder).await.unwrap_or(0);
        let required_space = input
            .estimated_import_bytes
            .saturating_add(runtime.free_space_reserve);
        if disk_available < required_space {
            start_result(&mut report, "VBOX-001");
            set_result(
                &mut report,
                "VBOX-001",
                "failed",
                json!({}),
                Some(format!(
                    "available disk space is below estimated imported disk size plus the configured reserve ({required_space} bytes required)"
                )),
            );
        } else {
            start_result(&mut report, "VBOX-001");
            import_attempted = true;
            let _ = atomic_write(
                &manifest_path,
                &serde_json::to_vec_pretty(&json!({"schema_version":1,"run_id":run_id,"delivery_id":input.delivery_id,"vm_name":vm_name,"vm_uuid":null,"disk_paths":[],"may_be_registered":true,"storage_path":vm_folder.display().to_string()})).unwrap_or_default(),
            );
            let imported = vbox(
                runtime,
                &[
                    "import",
                    input.source.to_str().unwrap_or_default(),
                    "--vsys",
                    "0",
                    "--vmname",
                    &vm_name,
                    "--basefolder",
                    vm_folder.to_str().unwrap_or_default(),
                ],
                runtime.import_timeout,
            )
            .await;
            match imported {
                Ok(output) if output.exit_code == Some(0) => {
                    write_command_log(report_dir, "import", &output);
                    let safety_result = disable_guest_features(runtime, &vm_name).await;
                    match vbox(
                        runtime,
                        &["showvminfo", &vm_name, "--machinereadable"],
                        Duration::from_secs(30),
                    )
                    .await
                    {
                        Ok(info) if info.exit_code == Some(0) => {
                            start_result(&mut report, "VBOX-002");
                            write_command_log(report_dir, "inspect", &info);
                            let parsed = parse_machine_info(&info.stdout);
                            let uuid = parsed.get("UUID").cloned();
                            vm_uuid = uuid.clone();
                            disk_paths = attached_media_paths(&parsed);
                            let _ = atomic_write(&manifest_path,&serde_json::to_vec_pretty(&json!({"schema_version":1,"run_id":run_id,"delivery_id":input.delivery_id,"vm_name":vm_name,"vm_uuid":vm_uuid,"disk_paths":disk_paths,"may_be_registered":false,"storage_path":vm_folder.display().to_string()})).unwrap_or_default());
                            report["vm"]["name"] = json!(vm_name);
                            report["vm"]["uuid"] = json!(uuid);
                            if let Err(reason) = safety_result {
                                set_result(
                                    &mut report,
                                    "VBOX-001",
                                    "failed",
                                    json!({"vm_name":vm_name,"vm_uuid":uuid}),
                                    Some(reason),
                                );
                                set_result(
                                    &mut report,
                                    "VBOX-002",
                                    "not_run",
                                    json!({}),
                                    Some("guest isolation settings could not be verified".into()),
                                );
                            } else {
                                report["vm"]["ram_mb"] =
                                    json!(parsed.get("memory").and_then(|v| v.parse::<u64>().ok()));
                                report["vm"]["cpus"] =
                                    json!(parsed.get("cpus").and_then(|v| v.parse::<u64>().ok()));
                                let disks =
                                    enrich_disks(runtime, disk_observations(&parsed, &vm_folder))
                                        .await;
                                report["vm"]["disks"] = json!(disks);
                                let adapters = adapter_observations(&parsed);
                                report["vm"]["network_adapters"] = json!(adapters);
                                let network_ok = validate_network(&adapters);
                                let paths_ok = disks_in_root(&parsed, &vm_folder);
                                let import_ok = uuid.is_some();
                                if import_ok {
                                    set_result(
                                        &mut report,
                                        "VBOX-001",
                                        "passed",
                                        json!({"vm_name":vm_name,"vm_uuid":uuid,"duration_ms":output.duration_ms}),
                                        None,
                                    );
                                }
                                let cpu_memory_ok = report["vm"]["ram_mb"].as_u64().is_some()
                                    && report["vm"]["cpus"].as_u64().is_some();
                                let disk_capacity_ok =
                                    disks.iter().all(|disk| {
                                        disk["kind"] != "disk"
                                            || disk["capacity_bytes"].as_u64().is_some()
                                    }) && disks.iter().any(|disk| disk["kind"] == "disk");
                                let inspection_ok = import_ok
                                    && network_ok
                                    && paths_ok
                                    && cpu_memory_ok
                                    && disk_capacity_ok;
                                let inspection = json!({"ram_mb":report["vm"]["ram_mb"].clone(),"cpus":report["vm"]["cpus"].clone(),"disks":report["vm"]["disks"].clone(),"network_adapters":adapters,"duration_ms":info.duration_ms});
                                if inspection_ok {
                                    set_result(&mut report, "VBOX-002", "passed", inspection, None);
                                    persist(&report, report_path);
                                    for trial in 1..=3 {
                                        start_result(&mut report, &format!("WEB-{trial:03}"));
                                        let outcome =
                                            run_http_trial(runtime, &vm_name, trial, report_dir)
                                                .await;
                                        let test_id = format!("WEB-{trial:03}");
                                        let elapsed = outcome
                                            .get("elapsed_ms")
                                            .cloned()
                                            .unwrap_or(Value::Null);
                                        let status = outcome
                                            .get("status")
                                            .and_then(Value::as_str)
                                            .unwrap_or("failed");
                                        report["web_startup_trials"]
                                            .as_array_mut()
                                            .unwrap()
                                            .push(outcome.clone());
                                        set_result(
                                            &mut report,
                                            &test_id,
                                            status,
                                            json!({"trial":trial,"elapsed_ms":elapsed,"http_status":outcome.get("http_status"),"html_evidence":outcome.get("html_evidence")}),
                                            outcome
                                                .get("error")
                                                .and_then(Value::as_str)
                                                .map(str::to_owned),
                                        );
                                        persist(&report, report_path);
                                        if outcome["shutdown"]["powered_off"] == false {
                                            for remaining in trial + 1..=3 {
                                                let remaining_id = format!("WEB-{remaining:03}");
                                                set_result(
                                                    &mut report,
                                                    &remaining_id,
                                                    "not_run",
                                                    json!({"trial":remaining}),
                                                    Some("previous trial VM did not power off; further starts were stopped for host safety".into()),
                                                );
                                            }
                                            persist(&report, report_path);
                                            break;
                                        }
                                    }
                                    diagnostic_boot(
                                        runtime,
                                        &vm_name,
                                        project,
                                        report_dir,
                                        &mut report,
                                    )
                                    .await;
                                } else {
                                    let reason = if !import_ok {
                                        "VirtualBox did not return a VM UUID"
                                    } else if !network_ok {
                                        "VM networking is outside the fixed loopback NAT policy"
                                    } else if !paths_ok {
                                        "one or more attached VM media paths resolve outside run-owned storage"
                                    } else if !cpu_memory_ok {
                                        "VirtualBox did not report RAM and CPU configuration"
                                    } else {
                                        "VirtualBox did not report the capacity of every attached virtual disk"
                                    };
                                    if !import_ok {
                                        set_result(
                                            &mut report,
                                            "VBOX-001",
                                            "failed",
                                            json!({"vm_name":vm_name}),
                                            Some(reason.into()),
                                        );
                                    }
                                    set_result(
                                        &mut report,
                                        "VBOX-002",
                                        "failed",
                                        inspection,
                                        Some(reason.into()),
                                    );
                                }
                            }
                        }
                        Ok(info) => {
                            set_result(
                                &mut report,
                                "VBOX-001",
                                "failed",
                                json!({}),
                                Some(info.stderr),
                            );
                        }
                        Err(error) => {
                            set_result(&mut report, "VBOX-001", "failed", json!({}), Some(error));
                        }
                    }
                }
                Ok(output) => {
                    write_command_log(report_dir, "import", &output);
                    set_result(
                        &mut report,
                        "VBOX-001",
                        "failed",
                        json!({"exit_code":output.exit_code}),
                        Some(output.stderr),
                    );
                }
                Err(error) => {
                    set_result(&mut report, "VBOX-001", "failed", json!({}), Some(error));
                }
            }
        }
    } else if !import_attempted {
        for test in [
            "VBOX-001", "VBOX-002", "WEB-001", "WEB-002", "WEB-003", "VBOX-003", "SSH-001",
        ] {
            set_result(
                &mut report,
                test,
                "not_run",
                json!({}),
                Some("delivery failed OVA intake validation".into()),
            );
        }
    }
    if report["screenshot"]["status"] == "not_run" {
        set_result(
            &mut report,
            "VBOX-003",
            "not_run",
            json!({}),
            Some("diagnostic boot was not reached".into()),
        );
    }
    if report["guest_filesystems"]["status"] == "not_run" {
        set_result(
            &mut report,
            "SSH-001",
            "not_run",
            json!({}),
            Some("diagnostic boot was not reached".into()),
        );
    }

    // The manifest is written before destructive cleanup and uses the unique
    // per-delivery storage directory plus the VM UUID, never a name prefix.
    start_result(&mut report, "CLEAN-001");
    let manifest = json!({"schema_version":1,"run_id":run_id,"delivery_id":input.delivery_id,"vm_name":vm_name,"vm_uuid":vm_uuid,"disk_paths":disk_paths,"may_be_registered":vm_uuid.is_none()&&import_attempted,"storage_path":vm_folder.display().to_string()});
    let _ = atomic_write(
        &manifest_path,
        &serde_json::to_vec_pretty(&manifest).unwrap_or_default(),
    );
    let mut shutdown_observation =
        json!({"powered_off":true,"graceful":true,"forced_power_off":false,"details":[]});
    let mut partial_cleanup_passed = false;
    if let Some(uuid) = vm_uuid.as_deref() {
        let state = vbox(
            runtime,
            &["showvminfo", uuid, "--machinereadable"],
            Duration::from_secs(15),
        )
        .await;
        if let Ok(state) = state {
            let parsed = parse_machine_info(&state.stdout);
            if state.exit_code != Some(0) || !is_owned_vm(&parsed, &vm_name, uuid, &vm_folder) {
                cleanup_errors.push("VM identity or configuration path does not match this delivery's cleanup manifest".into());
            } else {
                shutdown_observation = shutdown_vm(runtime, uuid).await;
                if let Some(errors) = shutdown_observation["details"].as_array() {
                    cleanup_errors.extend(
                        errors
                            .iter()
                            .filter_map(Value::as_str)
                            .map(ToOwned::to_owned),
                    );
                }
                if shutdown_observation["powered_off"] != true {
                    cleanup_errors.push("VM did not power off; refusing to unregister it".into());
                } else {
                    match vbox(runtime, &["unregistervm", uuid], Duration::from_secs(120)).await {
                        Ok(output) if output.exit_code == Some(0) => {
                            match close_owned_media(runtime, &disk_paths, &vm_folder).await {
                                Ok(()) => {
                                    let _ = fs::remove_dir_all(&vm_folder);
                                }
                                Err(e) => cleanup_errors.push(e),
                            }
                        }
                        Ok(output) => cleanup_errors.push(output.stderr),
                        Err(error) => cleanup_errors.push(error),
                    }
                }
                let _ = fs::remove_file(report_dir.join("known_hosts"));
            }
        } else {
            cleanup_errors.push("could not query VM state before manifest-scoped cleanup".into());
        }
    } else if import_attempted {
        let recovered = cleanup_from_manifest(runtime, report_dir, &input.delivery_id).await;
        shutdown_observation = recovered["shutdown"].clone();
        partial_cleanup_passed = recovered["status"] == "passed";
        if !partial_cleanup_passed {
            cleanup_errors.extend(
                recovered["details"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(ToOwned::to_owned),
            );
        }
    } else {
        // If import produced an unregistered partial directory, remove only
        // the unique directory created for this delivery.
        if vm_folder.starts_with(&runtime.storage_root) {
            let _ = fs::remove_dir_all(&vm_folder);
        }
    }
    let storage_removed = !vm_folder.exists();
    let unregistered = if let Some(uuid) = vm_uuid.as_deref() {
        vm_is_registered(runtime, uuid).await == Some(false)
    } else {
        !import_attempted || partial_cleanup_passed
    };
    if storage_removed {
        let _ = vm_folder.parent().and_then(|p| fs::remove_dir(p).ok());
    }
    report["cleanup"] = json!({"status":if cleanup_errors.is_empty() && storage_removed && unregistered {"passed"} else {"failed"},"vm_unregistered":unregistered,"run_storage_removed":storage_removed,"shutdown":shutdown_observation,"details":cleanup_errors,"manifest":"cleanup-manifest.json"});
    let cleanup_status = report["cleanup"]["status"]
        .as_str()
        .unwrap_or("failed")
        .to_owned();
    let cleanup_observation = report["cleanup"].clone();
    set_result(
        &mut report,
        "CLEAN-001",
        &cleanup_status,
        cleanup_observation,
        None,
    );
    report["finished_at"] = json!(now_string());
    let statuses = report["test_results"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    report["overall_status"] = json!(if statuses.iter().all(|r| r["status"] == "passed") {
        "completed"
    } else {
        "completed_with_errors"
    });
    persist(&report, report_path);
    report
}

pub async fn cleanup_from_manifest(
    runtime: &VmActivityRuntime,
    report_dir: &Path,
    delivery_id: &str,
) -> Value {
    let path = report_dir.join("cleanup-manifest.json");
    let manifest: Value = match fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
    {
        Some(v) => v,
        None => {
            return json!({"status":"failed","vm_unregistered":false,"run_storage_removed":false,"details":["cleanup manifest is missing or invalid"]});
        }
    };
    if manifest["delivery_id"] != delivery_id {
        return json!({"status":"failed","vm_unregistered":false,"run_storage_removed":false,"details":["cleanup manifest delivery ID mismatch"]});
    }
    let Some(storage) = manifest["storage_path"].as_str().map(PathBuf::from) else {
        return json!({"status":"failed","vm_unregistered":false,"run_storage_removed":false,"details":["manifest has no storage path"]});
    };
    let Some(run_id) = manifest["run_id"].as_str() else {
        return json!({"status":"failed","vm_unregistered":false,"run_storage_removed":false,"details":["manifest has no run ID"]});
    };
    if !valid_path_id(run_id) || !valid_path_id(delivery_id) || !storage.is_absolute() {
        return json!({"status":"failed","vm_unregistered":false,"run_storage_removed":false,"details":["manifest run, delivery, or storage path is invalid"]});
    }
    let Some(archive_parent) = manifest["archive_parent"].as_str().map(PathBuf::from) else {
        return json!({"status":"failed","vm_unregistered":false,"run_storage_removed":false,"details":["manifest has no project archive parent"]});
    };
    let Ok(archive_parent) = archive_parent.canonicalize() else {
        return json!({"status":"failed","vm_unregistered":false,"run_storage_removed":false,"details":["project archive parent is unavailable"]});
    };
    let Some(vm_name) = manifest["vm_name"]
        .as_str()
        .filter(|name| valid_vm_name(name))
    else {
        return json!({"status":"failed","vm_unregistered":false,"run_storage_removed":false,"details":["manifest VM name is missing or invalid"]});
    };
    let expected_suffix = Path::new(run_id).join(delivery_id);
    if !storage.starts_with(&archive_parent) || !storage.ends_with(&expected_suffix) {
        return json!({"status":"failed","vm_unregistered":false,"run_storage_removed":false,"details":["manifest storage path is not inside the project archive directory or does not match run ownership"]});
    }
    if storage.exists()
        && !storage
            .canonicalize()
            .is_ok_and(|actual| actual.starts_with(&archive_parent))
    {
        return json!({"status":"failed","vm_unregistered":false,"run_storage_removed":false,"details":["canonical manifest storage path escapes the project archive directory"]});
    }
    let expected_name = vm_name.to_owned();
    let mut details = Vec::new();
    let mut shutdown =
        json!({"powered_off":true,"graceful":true,"forced_power_off":false,"details":[]});
    let mut uuid = manifest["vm_uuid"].as_str().map(str::to_owned);
    let mut disk_paths = manifest["disk_paths"]
        .as_array()
        .map(|paths| {
            paths
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if manifest["may_be_registered"] == true && uuid.is_none() {
        match vbox(
            runtime,
            &["showvminfo", &expected_name, "--machinereadable"],
            Duration::from_secs(20),
        )
        .await
        {
            Ok(info) if info.exit_code == Some(0) => {
                let parsed = parse_machine_info(&info.stdout);
                let Some(found) = parsed.get("UUID").cloned() else {
                    return json!({"status":"failed","vm_unregistered":false,"run_storage_removed":false,"details":["registered VM has no UUID"]});
                };
                if !is_owned_vm(&parsed, &expected_name, &found, &storage) {
                    return json!({"status":"failed","vm_unregistered":false,"run_storage_removed":false,"details":["VM name, UUID, and configuration path do not prove ownership by the recorded run storage"]});
                }
                disk_paths = attached_media_paths(&parsed);
                uuid = Some(found);
            }
            Ok(_) => {
                let inventory = vbox(runtime, &["list", "vms"], Duration::from_secs(15)).await;
                if !inventory.is_ok_and(|out| {
                    out.exit_code == Some(0)
                        && !out.stdout.lines().any(|line| {
                            line.trim_start()
                                .starts_with(&format!("\"{expected_name}\""))
                        })
                }) {
                    return json!({"status":"failed","vm_unregistered":false,"run_storage_removed":false,"details":["could not prove the partially imported VM is absent from the dedicated registry"]});
                }
            }
            Err(e) => {
                return json!({"status":"failed","vm_unregistered":false,"run_storage_removed":false,"details":[e]});
            }
        }
    }
    let recovered_manifest = json!({"schema_version":2,"run_id":run_id,"delivery_id":delivery_id,"vm_name":expected_name,"vm_uuid":uuid,"disk_paths":disk_paths,"may_be_registered":false,"storage_path":storage.display().to_string(),"archive_parent":archive_parent.display().to_string()});
    let _ = atomic_write(
        &path,
        &serde_json::to_vec_pretty(&recovered_manifest).unwrap_or_default(),
    );
    if let Some(uuid) = uuid.as_deref() {
        if !valid_uuid(uuid) {
            return json!({"status":"failed","vm_unregistered":false,"run_storage_removed":false,"details":["manifest VM UUID is malformed"]});
        }
        match vbox(
            runtime,
            &["showvminfo", uuid, "--machinereadable"],
            Duration::from_secs(15),
        )
        .await
        {
            Ok(out) if out.exit_code != Some(0) => match vm_is_registered(runtime, uuid).await {
                Some(false) => {}
                Some(true) => {
                    return json!({"status":"failed","vm_unregistered":false,"run_storage_removed":false,"details":["VM UUID is registered but cannot be inspected; refusing cleanup"]});
                }
                None => {
                    return json!({"status":"failed","vm_unregistered":false,"run_storage_removed":false,"details":["could not verify VM registration state"]});
                }
            },
            Ok(_) => {
                let info = vbox(
                    runtime,
                    &["showvminfo", uuid, "--machinereadable"],
                    Duration::from_secs(15),
                )
                .await;
                let parsed = match info {
                    Ok(v) if v.exit_code == Some(0) => parse_machine_info(&v.stdout),
                    _ => {
                        return json!({"status":"failed","vm_unregistered":false,"run_storage_removed":false,"details":["cannot inspect VM ownership before cleanup"]});
                    }
                };
                if !is_owned_vm(&parsed, &expected_name, uuid, &storage) {
                    return json!({"status":"failed","vm_unregistered":false,"run_storage_removed":false,"details":["VM name, UUID, and configuration path do not match the cleanup manifest ownership"]});
                }
                disk_paths = attached_media_paths(&parsed);
                shutdown = shutdown_vm(runtime, &expected_name).await;
                if let Some(errors) = shutdown["details"].as_array() {
                    details.extend(
                        errors
                            .iter()
                            .filter_map(Value::as_str)
                            .map(ToOwned::to_owned),
                    );
                }
                if shutdown["powered_off"] != true {
                    return json!({"status":"failed","vm_unregistered":false,"run_storage_removed":false,"shutdown":shutdown,"details":["VM did not power off; refusing to unregister it"]});
                }
                match vbox(runtime, &["unregistervm", uuid], Duration::from_secs(120)).await {
                    Ok(out) if out.exit_code == Some(0) => {}
                    Ok(out) => details.push(out.stderr),
                    Err(e) => details.push(e),
                }
            }
            Err(e) => details.push(e),
        }
    }
    let unregistered = if let Some(uuid) = uuid.as_deref() {
        vm_is_registered(runtime, uuid).await == Some(false)
    } else {
        true
    };
    if !unregistered {
        return json!({"status":"failed","vm_unregistered":false,"run_storage_removed":false,"details":["VM remains registered after the cleanup attempt"]});
    }
    if let Err(error) = close_owned_media(runtime, &disk_paths, &storage).await {
        details.push(error);
    }
    if details.is_empty() {
        let _ = fs::remove_file(report_dir.join("known_hosts"));
        let _ = fs::remove_dir_all(&storage);
        let _ = storage.parent().and_then(|p| fs::remove_dir(p).ok());
    }
    let removed = !storage.exists();
    let final_manifest = json!({"schema_version":2,"run_id":run_id,"delivery_id":delivery_id,"vm_name":expected_name,"vm_uuid":uuid,"disk_paths":disk_paths,"may_be_registered":false,"storage_path":storage.display().to_string(),"archive_parent":archive_parent.display().to_string()});
    let _ = atomic_write(
        &path,
        &serde_json::to_vec_pretty(&final_manifest).unwrap_or_default(),
    );
    json!({"status":if unregistered&&removed&&details.is_empty(){"passed"}else{"failed"},"vm_unregistered":unregistered,"run_storage_removed":removed,"shutdown":shutdown,"details":details,"manifest":"cleanup-manifest.json"})
}

fn valid_path_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
fn valid_vm_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
fn valid_uuid(id: &str) -> bool {
    id.len() == 36 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}

async fn vm_is_registered(runtime: &VmActivityRuntime, uuid: &str) -> Option<bool> {
    let inventory = vbox(runtime, &["list", "vms", "--long"], Duration::from_secs(20))
        .await
        .ok()?;
    if inventory.exit_code != Some(0) {
        return None;
    }
    Some(
        inventory
            .stdout
            .lines()
            .any(|line| line.trim() == format!("UUID: {uuid}")),
    )
}

async fn diagnostic_boot(
    runtime: &VmActivityRuntime,
    vm: &str,
    project: &ProjectSnapshot,
    report_dir: &Path,
    report: &mut Value,
) {
    start_result(report, "VBOX-003");
    match vbox(
        runtime,
        &["startvm", vm, "--type", "headless"],
        Duration::from_secs(60),
    )
    .await
    {
        Ok(out) if out.exit_code == Some(0) => {
            let shot = report_dir.join("screenshot.png");
            let screenshot_start = Instant::now();
            match vbox(
                runtime,
                &[
                    "controlvm",
                    vm,
                    "screenshotpng",
                    shot.to_str().unwrap_or_default(),
                ],
                Duration::from_secs(30),
            )
            .await
            {
                Ok(out) if out.exit_code == Some(0) => {
                    report["screenshot"] = json!({"status":"passed","path":"evidence/screenshot.png","captured_at":now_string(),"duration_ms":screenshot_start.elapsed().as_millis()});
                    let _ = fs::create_dir_all(report_dir.join("evidence"));
                    let _ = fs::rename(&shot, report_dir.join("evidence/screenshot.png"));
                }
                Ok(out) => report["screenshot"] = json!({"status":"failed","error":out.stderr}),
                Err(e) => report["screenshot"] = json!({"status":"failed","error":e}),
            }
            start_result(report, "SSH-001");
            let ssh = collect_df(
                runtime,
                &project.ssh_username,
                &report_dir.join("known_hosts"),
                report_dir,
            )
            .await;
            report["guest_filesystems"] = ssh;
            let _ = vbox(
                runtime,
                &["controlvm", vm, "acpipowerbutton"],
                Duration::from_secs(15),
            )
            .await;
        }
        Ok(out) => {
            report["screenshot"] = json!({"status":"failed","error":out.stderr});
            report["guest_filesystems"] =
                json!({"status":"not_run","filesystems":[],"error":"diagnostic boot failed"});
        }
        Err(e) => {
            report["screenshot"] = json!({"status":"failed","error":e});
            report["guest_filesystems"] =
                json!({"status":"not_run","filesystems":[],"error":"diagnostic boot failed"});
        }
    }
    let shot_status = report["screenshot"]["status"]
        .as_str()
        .unwrap_or("failed")
        .to_owned();
    set_result(
        report,
        "VBOX-003",
        &shot_status,
        report["screenshot"].clone(),
        report["screenshot"]["error"].as_str().map(str::to_owned),
    );
    let ssh_status = report["guest_filesystems"]["status"]
        .as_str()
        .unwrap_or("failed")
        .to_owned();
    set_result(
        report,
        "SSH-001",
        &ssh_status,
        report["guest_filesystems"].clone(),
        report["guest_filesystems"]["error"]
            .as_str()
            .map(str::to_owned),
    );
}

pub(crate) async fn disable_guest_features(
    runtime: &VmActivityRuntime,
    vm: &str,
) -> Result<(), String> {
    let info = vbox(
        runtime,
        &["showvminfo", vm, "--machinereadable"],
        Duration::from_secs(30),
    )
    .await?;
    if info.exit_code != Some(0) {
        return Err(format!(
            "cannot inspect VM before applying isolation settings: {}",
            info.stderr
        ));
    }
    let machine = parse_machine_info(&info.stdout);
    let settings = vbox(
        runtime,
        &[
            "modifyvm",
            vm,
            "--clipboard",
            "disabled",
            "--draganddrop",
            "disabled",
            "--usb",
            "off",
        ],
        Duration::from_secs(30),
    )
    .await?;
    if settings.exit_code != Some(0) {
        return Err(format!(
            "VirtualBox could not disable clipboard, drag-and-drop, USB, and apply the run group: {}",
            settings.stderr
        ));
    }
    for (key, value) in &machine {
        if key.starts_with("SharedFolderNameMachineMapping")
            || key.starts_with("SharedFolderNameTransientMapping")
        {
            let name = value.as_str();
            if name.is_empty() {
                continue;
            }
            let removed = vbox(
                runtime,
                &["sharedfolder", "remove", vm, "--name", name],
                Duration::from_secs(20),
            )
            .await?;
            if removed.exit_code != Some(0) {
                return Err(format!(
                    "could not remove imported shared folder {name}: {}",
                    removed.stderr
                ));
            }
        }
    }
    let verify = vbox(
        runtime,
        &["showvminfo", vm, "--machinereadable"],
        Duration::from_secs(30),
    )
    .await?;
    if verify.exit_code != Some(0) {
        return Err(format!(
            "cannot verify VM isolation settings: {}",
            verify.stderr
        ));
    }
    let after = parse_machine_info(&verify.stdout);
    if !isolation_settings_are_disabled(&after) {
        let state = |key: &str| after.get(key).map(String::as_str).unwrap_or("<missing>");
        let shared_folder_count = after
            .keys()
            .filter(|key| {
                key.starts_with("SharedFolderNameMachineMapping")
                    || key.starts_with("SharedFolderNameTransientMapping")
            })
            .count();
        return Err(format!(
            "VirtualBox isolation verification failed: clipboard={}, draganddrop={}, usb={}, shared_folder_entries={shared_folder_count}",
            state("clipboard"),
            state("draganddrop"),
            state("usb")
        ));
    }
    Ok(())
}

fn isolation_settings_are_disabled(info: &std::collections::HashMap<String, String>) -> bool {
    info.get("clipboard").map(String::as_str) == Some("disabled")
        && info.get("draganddrop").map(String::as_str) == Some("disabled")
        && info.get("usb").map(String::as_str) == Some("off")
        && !info.keys().any(|key| {
            key.starts_with("SharedFolderNameMachineMapping")
                || key.starts_with("SharedFolderNameTransientMapping")
        })
}

async fn run_http_trial(
    runtime: &VmActivityRuntime,
    vm: &str,
    trial: u8,
    report_dir: &Path,
) -> Value {
    let _ = vbox(runtime, &["discardstate", vm], Duration::from_secs(20)).await;
    let started = Instant::now();
    let started_at = now_string();
    let start_result = vbox(
        runtime,
        &["startvm", vm, "--type", "headless"],
        Duration::from_secs(60),
    )
    .await;
    if !start_result.as_ref().is_ok_and(|o| o.exit_code == Some(0)) {
        let err = match start_result {
            Ok(output) => format!("VBoxManage startvm failed: {}", output.stderr),
            Err(error) => error,
        };
        let shutdown = shutdown_vm(runtime, vm).await;
        return json!({"trial":trial,"status":"failed","elapsed_ms":started.elapsed().as_millis(),"started_at":started_at,"error":err,"shutdown":shutdown});
    }
    let client = match reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(2))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            let shutdown = shutdown_vm(runtime, vm).await;
            return json!({"trial":trial,"status":"error","elapsed_ms":started.elapsed().as_millis(),"started_at":started_at,"error":e.to_string(),"shutdown":shutdown});
        }
    };
    let deadline = Instant::now() + runtime.startup_timeout;
    let mut polls = 0;
    while Instant::now() < deadline {
        polls += 1;
        if let Ok(response) = client.get("http://127.0.0.1:8080/").send().await {
            let elapsed = started.elapsed().as_millis();
            let ready_at = now_string();
            let status = response.status().as_u16();
            let mut stream = response.bytes_stream();
            use futures_util::StreamExt;
            let mut body = Vec::new();
            let max = 10 * 1024 * 1024usize;
            let mut truncated = false;
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        let keep = (max - body.len()).min(bytes.len());
                        body.extend_from_slice(&bytes[..keep]);
                        if keep < bytes.len() {
                            truncated = true;
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            let filename = format!("web-trial-{trial}.html");
            let _ = atomic_write(&report_dir.join("evidence").join(&filename), &body);
            let shutdown = shutdown_vm(runtime, vm).await;
            return json!({"trial":trial,"status":if shutdown["powered_off"]==true{"passed"}else{"failed"},"started_at":started_at,"ready_at":ready_at,"elapsed_ms":elapsed,"poll_count":polls,"http_status":status,"response_bytes":body.len(),"truncated":truncated,"html_evidence":format!("evidence/{filename}"),"error":if shutdown["powered_off"]==true{Value::Null}else{json!("VM failed to power off after HTTP trial")},"shutdown":shutdown});
        }
        sleep(Duration::from_secs(1)).await;
    }
    let elapsed = started.elapsed().as_millis();
    let shutdown = shutdown_vm(runtime, vm).await;
    json!({"trial":trial,"status":"timed_out","started_at":started_at,"elapsed_ms":elapsed,"poll_count":polls,"error":"HTTP readiness deadline expired","shutdown":shutdown})
}

async fn shutdown_vm(runtime: &VmActivityRuntime, vm: &str) -> Value {
    let mut graceful = false;
    let mut forced = false;
    let mut details = Vec::new();
    let state = vbox(
        runtime,
        &["showvminfo", vm, "--machinereadable"],
        Duration::from_secs(10),
    )
    .await;
    if let Ok(state) = state {
        if state.stdout.contains("VMState=\"poweroff\"") {
            graceful = true;
        } else {
            let _ = vbox(
                runtime,
                &["controlvm", vm, "acpipowerbutton"],
                Duration::from_secs(15),
            )
            .await;
            let deadline = Instant::now() + runtime.shutdown_grace;
            while Instant::now() < deadline {
                if let Ok(info) = vbox(
                    runtime,
                    &["showvminfo", vm, "--machinereadable"],
                    Duration::from_secs(10),
                )
                .await
                {
                    if info.stdout.contains("VMState=\"poweroff\"") {
                        graceful = true;
                        break;
                    }
                }
                sleep(Duration::from_secs(1)).await;
            }
            if !graceful {
                match vbox(
                    runtime,
                    &["controlvm", vm, "poweroff"],
                    Duration::from_secs(15),
                )
                .await
                {
                    Ok(out) if out.exit_code == Some(0) => forced = true,
                    Ok(out) => details.push(out.stderr),
                    Err(e) => details.push(e),
                }
            }
        }
    } else {
        details.push("could not query VM state before shutdown".into());
    }
    let powered_off = if let Ok(info) = vbox(
        runtime,
        &["showvminfo", vm, "--machinereadable"],
        Duration::from_secs(10),
    )
    .await
    {
        info.stdout.contains("VMState=\"poweroff\"")
    } else {
        false
    };
    json!({"powered_off":powered_off,"graceful":graceful,"forced_power_off":forced,"details":details})
}

async fn collect_df(
    runtime: &VmActivityRuntime,
    username: &str,
    known_hosts: &Path,
    report_dir: &Path,
) -> Value {
    let password = match fs::read_to_string(&runtime.ssh_secret_file) {
        Ok(p) => p.trim_end().to_owned(),
        Err(e) => {
            return json!({"status":"failed","command":"LC_ALL=C df -B1","filesystems":[],"error":format!("cannot read configured SSH secret file: {e}")});
        }
    };
    let target = known_hosts.to_path_buf();
    let username = username.to_owned();
    let password_copy = password.clone();
    let timeout = runtime.ssh_timeout;
    let output_limit = runtime.output_limit;
    let result = tokio::task::spawn_blocking(move || {
        ssh_df(&username, &password_copy, timeout, output_limit, &target)
    })
    .await;
    match result {
        Ok(Ok((
            stdout,
            stderr,
            connect_ms,
            command_ms,
            exit,
            stdout_truncated,
            stderr_truncated,
        ))) => {
            let _ = atomic_write(
                &report_dir.join("logs/ssh-df.stdout.txt"),
                stdout.as_bytes(),
            );
            let _ = atomic_write(
                &report_dir.join("logs/ssh-df.stderr.txt"),
                stderr.as_bytes(),
            );
            let rows = parse_df(&stdout);
            json!({"status":if exit==0{"passed"}else{"failed"},"command":"LC_ALL=C df -B1","raw_output_path":"logs/ssh-df.stdout.txt","stderr_path":"logs/ssh-df.stderr.txt","connect_duration_ms":connect_ms,"command_duration_ms":command_ms,"exit_code":exit,"stdout_truncated":stdout_truncated,"stderr_truncated":stderr_truncated,"filesystems":rows})
        }
        Ok(Err(e)) => {
            json!({"status":"failed","command":"LC_ALL=C df -B1","filesystems":[],"error":e})
        }
        Err(e) => {
            json!({"status":"failed","command":"LC_ALL=C df -B1","filesystems":[],"error":e.to_string()})
        }
    }
}

fn ssh_df(
    username: &str,
    password: &str,
    timeout: Duration,
    output_limit: usize,
    known_hosts: &Path,
) -> Result<(String, String, u128, u128, i32, bool, bool), String> {
    let started = Instant::now();
    let address: SocketAddr = "127.0.0.1:2222".parse().unwrap();
    let tcp = TcpStream::connect_timeout(&address, timeout)
        .map_err(|e| format!("SSH connect failed: {e}"))?;
    tcp.set_read_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    tcp.set_write_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    let connect_ms = started.elapsed().as_millis();
    let mut session = ssh2::Session::new().map_err(|e| e.to_string())?;
    session.set_tcp_stream(tcp);
    session.set_timeout(timeout.as_millis().min(u32::MAX as u128) as u32);
    session
        .handshake()
        .map_err(|e| format!("SSH handshake failed: {e}"))?;
    let (host_key, _) = session
        .host_key()
        .ok_or("SSH server did not provide a host key")?;
    let fingerprint = format!("{:x}", Sha256::digest(host_key));
    if known_hosts.exists() {
        let prior = fs::read_to_string(known_hosts).map_err(|e| e.to_string())?;
        if prior.trim() != fingerprint {
            return Err("SSH host key changed during this VM run".into());
        }
    } else {
        atomic_write(known_hosts, fingerprint.as_bytes()).map_err(|e| e.to_string())?;
    }
    session
        .userauth_password(username, password)
        .map_err(|e| format!("SSH authentication failed: {e}"))?;
    let command_start = Instant::now();
    let mut channel = session.channel_session().map_err(|e| e.to_string())?;
    channel.exec("LC_ALL=C df -B1").map_err(|e| e.to_string())?;
    let (stdout, stdout_truncated) =
        read_bounded_sync(&mut channel, output_limit).map_err(|e| e.to_string())?;
    let mut stderr_stream = channel.stderr();
    let (stderr, stderr_truncated) =
        read_bounded_sync(&mut stderr_stream, output_limit).map_err(|e| e.to_string())?;
    channel.wait_close().map_err(|e| e.to_string())?;
    let command_ms = command_start.elapsed().as_millis();
    let exit = channel.exit_status().unwrap_or(-1);
    Ok((
        stdout,
        stderr,
        connect_ms,
        command_ms,
        exit,
        stdout_truncated,
        stderr_truncated,
    ))
}

fn read_bounded_sync(reader: &mut impl Read, limit: usize) -> std::io::Result<(String, bool)> {
    let mut saved = Vec::with_capacity(limit.min(8192));
    let mut buffer = [0u8; 8192];
    let mut truncated = false;
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        let keep = limit.saturating_sub(saved.len()).min(n);
        saved.extend_from_slice(&buffer[..keep]);
        if keep < n {
            truncated = true;
        }
    }
    let mut value = String::from_utf8_lossy(&saved).into_owned();
    if truncated {
        value.push_str("\n[output truncated]");
    }
    Ok((value, truncated))
}

fn parse_df(output: &str) -> Vec<Value> {
    output.lines().skip(1).filter_map(|line|{let mut parts=line.split_whitespace();let device=parts.next()?;let total=parts.next()?.parse::<u64>().ok()?;let used=parts.next()?.parse::<u64>().ok()?;let available=parts.next()?.parse::<u64>().ok()?;let percent=parts.next().unwrap_or_default().trim_end_matches('%').parse::<u8>().ok();let mount=parts.collect::<Vec<_>>().join(" ");Some(json!({"device":device,"total_bytes":total,"used_bytes":used,"available_bytes":available,"use_percent":percent,"mount_point":mount}))}).collect()
}

pub(crate) fn parse_machine_info(output: &str) -> std::collections::HashMap<String, String> {
    output
        .lines()
        .filter_map(|line| {
            let (k, v) = line.split_once('=')?;
            Some((k.to_owned(), v.trim_matches('"').to_owned()))
        })
        .collect()
}
fn disk_observations(info: &std::collections::HashMap<String, String>, root: &Path) -> Vec<Value> {
    info.iter().filter(|(_,v)|{let lower=v.to_ascii_lowercase();lower.ends_with(".vdi")||lower.ends_with(".vmdk")||lower.ends_with(".vhd")||lower.ends_with(".iso")}).map(|(key,path)|{let lower=path.to_ascii_lowercase();let kind=if lower.ends_with(".iso"){"dvd"}else{"disk"};json!({"attachment":key,"kind":kind,"path":path,"inside_run_storage":Path::new(path).canonicalize().is_ok_and(|p|p.starts_with(root))})}).collect()
}

pub(crate) fn attached_media_paths(
    info: &std::collections::HashMap<String, String>,
) -> Vec<String> {
    info.values()
        .filter(|path| {
            let lower = path.to_ascii_lowercase();
            lower.ends_with(".vdi")
                || lower.ends_with(".vmdk")
                || lower.ends_with(".vhd")
                || lower.ends_with(".iso")
        })
        .cloned()
        .collect()
}

pub(crate) fn is_owned_vm(
    info: &std::collections::HashMap<String, String>,
    name: &str,
    uuid: &str,
    storage: &Path,
) -> bool {
    let Ok(storage_root) = storage.canonicalize() else {
        return false;
    };
    info.get("name")
        .or_else(|| info.get("Name"))
        .is_some_and(|actual| actual == name)
        && info.get("UUID").is_some_and(|actual| actual == uuid)
        && info.get("CfgFile").is_some_and(|cfg| {
            Path::new(cfg)
                .canonicalize()
                .is_ok_and(|path| path.starts_with(&storage_root))
        })
}

async fn close_owned_media(
    runtime: &VmActivityRuntime,
    _attached_paths: &[String],
    storage: &Path,
) -> Result<(), String> {
    if !storage.exists() {
        return Ok(());
    }
    let root = storage.canonicalize().map_err(|e| e.to_string())?;
    for (list_kind, close_kind) in [("hdds", "disk"), ("dvds", "dvd"), ("floppies", "floppy")] {
        let output = vbox(
            runtime,
            &["list", list_kind, "--long"],
            Duration::from_secs(30),
        )
        .await?;
        if output.exit_code != Some(0) {
            return Err(format!(
                "cannot list VirtualBox {list_kind}: {}",
                output.stderr
            ));
        }
        for (uuid, location) in parse_media_list(&output.stdout) {
            if !valid_uuid(&uuid) || !path_is_within(&location, &root) {
                continue;
            }
            let closed = vbox(
                runtime,
                &["closemedium", close_kind, &uuid],
                Duration::from_secs(30),
            )
            .await?;
            if closed.exit_code != Some(0) {
                return Err(format!(
                    "cannot close run-owned {close_kind} medium at {location}: {}",
                    closed.stderr
                ));
            }
        }
    }
    Ok(())
}

fn parse_media_list(output: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut uuid = None;
    let mut location = None;
    let flush = |found: &mut Vec<(String, String)>,
                 uuid: &mut Option<String>,
                 location: &mut Option<String>| {
        if let (Some(uuid), Some(location)) = (uuid.take(), location.take()) {
            found.push((uuid, location));
        }
    };
    for line in output.lines() {
        if line.trim().is_empty() {
            flush(&mut found, &mut uuid, &mut location);
            continue;
        }
        if let Some(value) = line.strip_prefix("UUID:") {
            uuid = Some(value.trim().to_owned());
        }
        if let Some(value) = line.strip_prefix("Location:") {
            location = Some(value.trim().to_owned());
        }
    }
    flush(&mut found, &mut uuid, &mut location);
    found
}

fn path_is_within(path: &str, root: &Path) -> bool {
    let candidate = Path::new(path);
    if candidate
        .canonicalize()
        .is_ok_and(|canonical| canonical.starts_with(root))
    {
        return true;
    }
    candidate.is_absolute()
        && !candidate
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
        && candidate.starts_with(root)
}
fn disks_in_root(info: &std::collections::HashMap<String, String>, root: &Path) -> bool {
    let disks = disk_observations(info, root);
    !disks.is_empty() && disks.iter().all(|d| d["inside_run_storage"] == true)
}
async fn enrich_disks(runtime: &VmActivityRuntime, mut disks: Vec<Value>) -> Vec<Value> {
    for disk in &mut disks {
        if disk["kind"] != "disk" {
            continue;
        }
        if let Some(path) = disk["path"].as_str() {
            if let Ok(info) = vbox(
                runtime,
                &["showmediuminfo", path, "--machinereadable"],
                Duration::from_secs(30),
            )
            .await
            {
                disk["capacity_bytes"] = json!(
                    parse_machine_info(&info.stdout)
                        .get("Capacity")
                        .and_then(|v| v.parse::<u64>().ok())
                );
                disk["medium_type"] =
                    json!(parse_machine_info(&info.stdout).get("Format").cloned());
                disk["inspection_exit_code"] = json!(info.exit_code);
                if info.exit_code != Some(0) {
                    disk["inspection_error"] = json!(info.stderr);
                }
            } else {
                disk["inspection_error"] = json!("VBoxManage showmediuminfo failed");
            }
        }
    }
    disks
}
pub(crate) fn adapter_observations(info: &std::collections::HashMap<String, String>) -> Vec<Value> {
    let mut forwards = info
        .iter()
        .filter(|(k, _)| k.starts_with("Forwarding(") || k.starts_with("natpf"))
        .map(|(_, v)| v.clone())
        .collect::<Vec<_>>();
    forwards.sort();
    (1..=8)
        .filter_map(|i| {
            let key = format!("nic{i}");
            info.get(&key).map(|mode| {
                let adapter_forwards = if mode == "nat" {
                    forwards.clone()
                } else {
                    Vec::new()
                };
                json!({"slot":i,"enabled":mode!="none","attachment":mode,"forwarding":adapter_forwards})
            })
        })
        .collect()
}
fn valid_forwarding(entry: &str) -> Option<(u16, u16)> {
    let fields = entry.split(',').collect::<Vec<_>>();
    if fields.len() < 6 || fields[1] != "tcp" || fields[2] != "127.0.0.1" {
        return None;
    }
    Some((fields[3].parse().ok()?, fields[5].parse().ok()?))
}
pub(crate) fn validate_network(adapters: &[Value]) -> bool {
    let enabled = adapters
        .iter()
        .filter(|a| a["enabled"] == true)
        .collect::<Vec<_>>();
    if enabled.is_empty() || enabled.iter().any(|a| a["attachment"] != "nat") {
        return false;
    }
    let forwarding = enabled
        .iter()
        .flat_map(|a| a["forwarding"].as_array().into_iter().flatten())
        .filter_map(|v| v.as_str())
        .collect::<Vec<_>>();
    let rules = forwarding
        .iter()
        .filter_map(|entry| valid_forwarding(entry))
        .collect::<Vec<_>>();
    rules.len() == 2
        && forwarding.len() == rules.len()
        && rules.contains(&(8080, 80))
        && rules.contains(&(2222, 22))
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    left.starts_with(right) || right.starts_with(left)
}

async fn available_bytes(path: &Path) -> Option<u64> {
    let output = Command::new("df")
        .args(["-B1", "--output=avail", "--"])
        .arg(path)
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .nth(1)?
        .trim()
        .parse()
        .ok()
}

struct CommandOutput {
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    duration_ms: u128,
}
async fn vbox(
    runtime: &VmActivityRuntime,
    args: &[&str],
    limit: Duration,
) -> Result<CommandOutput, String> {
    let args = args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
    let runner = thot_runner::vbox::LocalVirtualBoxRunner::new(
        runtime.vbox_manage.clone(),
        runtime.vbox_user_home.clone(),
        limit,
        runtime.storage_root.clone(),
        thot_runner::vbox::OutputLimits {
            stdout_bytes: runtime.output_limit,
            stderr_bytes: runtime.output_limit,
        },
    );
    let output = runner.run(&args).await?;
    Ok(CommandOutput {
        exit_code: output.exit_code,
        stdout: output.stdout,
        stderr: output.stderr,
        duration_ms: output.duration.as_millis(),
    })
}
fn write_command_log(dir: &Path, name: &str, output: &CommandOutput) {
    let _ = atomic_write(
        &dir.join(format!("logs/{name}.stdout.txt")),
        output.stdout.as_bytes(),
    );
    let _ = atomic_write(
        &dir.join(format!("logs/{name}.stderr.txt")),
        output.stderr.as_bytes(),
    );
    let _ = atomic_write(
        &dir.join(format!("logs/{name}.json")),
        &serde_json::to_vec_pretty(
            &json!({"exit_code":output.exit_code,"duration_ms":output.duration_ms}),
        )
        .unwrap_or_default(),
    );
}
fn start_result(report: &mut Value, test_id: &str) {
    if let Some(result) = report["test_results"]
        .as_array_mut()
        .and_then(|v| v.iter_mut().find(|r| r["test_id"] == test_id))
    {
        result["start_time"] = json!(now_string());
    }
}
fn set_result(
    report: &mut Value,
    test_id: &str,
    status: &str,
    observation: Value,
    error: Option<String>,
) {
    if let Some(results) = report["test_results"].as_array_mut() {
        if let Some(result) = results.iter_mut().find(|r| r["test_id"] == test_id) {
            if result["start_time"].is_null() {
                result["start_time"] = json!(now_string());
            }
            result["status"] = json!(status);
            result["end_time"] = json!(now_string());
            result["duration_ms"] = observation
                .get("duration_ms")
                .or_else(|| observation.get("elapsed_ms"))
                .cloned()
                .unwrap_or(Value::Null);
            let mut evidence = Vec::new();
            for key in ["html_evidence", "path", "raw_output_path", "stderr_path"] {
                if let Some(v) = observation.get(key).and_then(Value::as_str) {
                    evidence.push(json!(v));
                }
            }
            result["evidence"] = json!(evidence);
            result["observation"] = observation;
            if let Some(error) = error {
                result["diagnostics"] = json!(error);
            }
        }
    }
}
fn persist(report: &Value, path: &Path) {
    if let Ok(bytes) = serde_json::to_vec_pretty(report) {
        let _ = atomic_write(path, &bytes);
    }
}
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension(format!("tmp-{}", std::process::id()));
    let mut f = File::create(&temp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    fs::rename(temp, path)
}
fn now_string() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod project_path_tests {
    use super::*;
    use flate2::{Compression, write::GzEncoder};

    #[tokio::test]
    async fn loads_project_archive_from_an_arbitrary_absolute_path() {
        let root = std::env::temp_dir().join(format!("thot-project-path-{}", uuid::Uuid::new_v4()));
        let archive_dir = root.join("instructor-files").join("term-1");
        let archive_path = archive_dir.join("linux competition.thot.tgz");
        let snapshot_dir = root.join("artifacts").join("project-snapshot");
        fs::create_dir_all(&archive_dir).unwrap();

        let config = b"activity_name: Sample\ndelivery_folder: /srv/submissions\nconcurrency: 1\ntimeout: 30\ntests:\n  - name: start\n    test: VirtualBox/start_vm\n";
        let file = File::create(&archive_path).unwrap();
        let encoder = GzEncoder::new(file, Compression::default());
        let mut tar = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_size(config.len() as u64);
        header.set_mode(0o640);
        header.set_cksum();
        tar.append_data(&mut header, "configuration.yaml", config.as_slice())
            .unwrap();
        tar.into_inner().unwrap().finish().unwrap();

        let project = load_project(&archive_path, &snapshot_dir).await.unwrap();
        assert_eq!(project.archive_path, archive_path.canonicalize().unwrap());
        assert!(project.project_id.starts_with("linux_competition-"));
        assert_eq!(project.configuration.activity_name, "Sample");

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ownership_check_accepts_vbox_lowercase_name_and_nested_config_path() {
        let root = std::env::temp_dir().join(format!("thot-vm-owner-{}", uuid::Uuid::new_v4()));
        let storage = root.join("run").join("delivery");
        let config_dir = storage.join("linux_competition").join("linux_competition");
        fs::create_dir_all(&config_dir).unwrap();
        let config_path = config_dir.join("linux_competition.vbox");
        fs::write(&config_path, "test config").unwrap();
        let info = parse_machine_info(&format!(
            "name=\"linux_competition\"\nUUID=\"067df5f1-9d83-4046-a9ba-2b4b1c22773a\"\nCfgFile=\"{}\"\n",
            config_path.display()
        ));

        assert!(is_owned_vm(
            &info,
            "linux_competition",
            "067df5f1-9d83-4046-a9ba-2b4b1c22773a",
            &storage,
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn isolation_verifier_uses_virtualbox_usb_machine_field() {
        let machine = parse_machine_info(
            "clipboard=\"disabled\"\ndraganddrop=\"disabled\"\nusb=\"off\"\nehci=\"off\"\nxhci=\"off\"\n",
        );
        assert!(isolation_settings_are_disabled(&machine));

        let still_enabled = parse_machine_info(
            "clipboard=\"disabled\"\ndraganddrop=\"disabled\"\nusb=\"on\"\nehci=\"on\"\nxhci=\"off\"\n",
        );
        assert!(!isolation_settings_are_disabled(&still_enabled));
    }

    #[test]
    fn adapter_observations_only_attach_forwarding_to_nat_adapters() {
        let info = parse_machine_info(
            "nic1=\"nat\"\nnic2=\"none\"\nForwarding(0)=\"HTTP,tcp,,8080,,80\"\n",
        );
        let adapters = adapter_observations(&info);
        assert_eq!(adapters.len(), 2);
        assert_eq!(adapters[0]["forwarding"][0], "HTTP,tcp,,8080,,80");
        assert!(adapters[1]["forwarding"].as_array().unwrap().is_empty());
    }
}
