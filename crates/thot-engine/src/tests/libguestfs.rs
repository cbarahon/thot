//! Purpose: Inspects VM disk images offline with libguestfs tools and emits structured disk or Linux root filesystem data.
mod tinycore;

use serde_json::{Map, Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Instant,
};
use thot_core::test_results::TestStatus;
use thot_runner::external::{ExternalProcessResult, run_external_process};

use crate::test_execution::{TestInvocation, TestOutcome};

const DISK_EXTENSIONS: &[&str] = &["vdi", "vmdk", "qcow2", "vhd", "vhdx", "img", "raw"];

pub async fn run(invocation: &TestInvocation) -> TestOutcome {
    match invocation.test_reference.as_str() {
        "libguestfs/disk_info" => disk_info(invocation).await,
        "libguestfs/linux_rootfs_info" => linux_rootfs_info(invocation).await,
        "libguestfs/linux_tinycore_info" => tinycore::run(invocation).await,
        reference => TestOutcome::error(format!("unknown libguestfs test {reference:?}")),
    }
}

async fn disk_info(invocation: &TestInvocation) -> TestOutcome {
    let disk = match resolve_disk(invocation) {
        Ok(disk) => disk,
        Err(error) => return TestOutcome::error(error),
    };
    let started = Instant::now();
    let filesystems = match run_filesystems(invocation, &disk, started).await {
        Ok(output) if output.exit_code == Some(0) && !output.timed_out => output,
        Ok(output) => return failed_command("virt-filesystems", output),
        Err(error) => return TestOutcome::error(error),
    };
    let objects = match parse_filesystems_csv(&filesystems.stdout) {
        Ok(objects) => objects,
        Err(error) => {
            return TestOutcome::error(format!(
                "cannot parse virt-filesystems output for {}: {error}",
                disk.path.display()
            ));
        }
    };
    let mut stderr = labeled_stderr("virt-filesystems", &filesystems.stderr);
    let (usage, usage_problem, usage_timed_out) = collect_usage(invocation, &disk, started).await;
    if let Some(problem) = &usage_problem {
        append_stderr(&mut stderr, &format!("virt-df: {problem}"));
    }
    let observation = disk_observation(&disk, objects, usage);
    let mut outcome = success_outcome(observation, stderr);
    if usage_timed_out {
        outcome.status = TestStatus::TimedOut;
        outcome.exit_code = None;
        outcome.diagnostics =
            "libguestfs disk usage inspection exceeded the configured timeout".into();
    } else if let Some(problem) = usage_problem {
        outcome.diagnostics = format!("partition usage could not be collected: {problem}");
    }
    outcome
}

async fn linux_rootfs_info(invocation: &TestInvocation) -> TestOutcome {
    let disk = match resolve_disk(invocation) {
        Ok(disk) => disk,
        Err(error) => return TestOutcome::error(error),
    };
    let started = Instant::now();
    let filesystems = match run_filesystems(invocation, &disk, started).await {
        Ok(output) if output.exit_code == Some(0) && !output.timed_out => output,
        Ok(output) => return failed_command("virt-filesystems", output),
        Err(error) => return TestOutcome::error(error),
    };
    let objects = match parse_filesystems_csv(&filesystems.stdout) {
        Ok(objects) => objects,
        Err(error) => {
            return TestOutcome::error(format!(
                "cannot parse virt-filesystems output for {}: {error}",
                disk.path.display()
            ));
        }
    };

    let inspector = match run_inspector(invocation, &disk, started).await {
        Ok(output) if output.exit_code == Some(0) && !output.timed_out => output,
        Ok(output) => return failed_command("virt-inspector", output),
        Err(error) => return TestOutcome::error(error),
    };
    let operating_systems = parse_operating_systems(&inspector.stdout);
    let linux = operating_systems
        .iter()
        .find(|operating_system| operating_system["type"] == "linux");
    let root_device = linux
        .and_then(|operating_system| operating_system["root_device"].as_str())
        .map(str::to_owned);

    let Some(root_device) = root_device else {
        let partitions = partition_summary(&objects);
        let stderr = format!(
            "libguestfs did not identify a Linux root filesystem. Partitions found:\n{}",
            serde_yaml::to_string(&partitions)
                .unwrap_or_else(|_| "- unable to format partition details\n".into())
        );
        return TestOutcome {
            status: TestStatus::Error,
            stdout: String::new(),
            stderr,
            exit_code: Some(1),
            diagnostics: "libguestfs did not identify a Linux root filesystem".into(),
            observation: json!({}),
            evidence: Vec::new(),
        };
    };

    let mut stderr = labeled_stderr("virt-filesystems", &filesystems.stderr);
    append_stderr(
        &mut stderr,
        &labeled_stderr("virt-inspector", &inspector.stderr),
    );
    let (usage, usage_problem, usage_timed_out) = collect_usage(invocation, &disk, started).await;
    if let Some(problem) = &usage_problem {
        append_stderr(&mut stderr, &format!("virt-df: {problem}"));
    }
    let (users, users_problem, users_timed_out) =
        collect_guest_records(invocation, &disk, started, "/etc/passwd", parse_passwd).await;
    if let Some(problem) = &users_problem {
        append_stderr(&mut stderr, &format!("virt-cat /etc/passwd: {problem}"));
    }
    let (groups, groups_problem, groups_timed_out) =
        collect_guest_records(invocation, &disk, started, "/etc/group", parse_group).await;
    if let Some(problem) = &groups_problem {
        append_stderr(&mut stderr, &format!("virt-cat /etc/group: {problem}"));
    }

    let root_object = objects.iter().find(|object| object["name"] == root_device);
    let root_partition = objects
        .iter()
        .find(|object| object["name"] == root_device && object["type"] == "partition");
    let filesystem_usage = usage.as_ref().and_then(|records| {
        records
            .iter()
            .find(|record| record["device"] == root_device)
    });
    let root_filesystem = json!({
        "device": root_device,
        "filesystem_type": root_object.map(|item| item["filesystem_type"].clone()).unwrap_or(Value::Null),
        "label": root_object.map(|item| item["label"].clone()).unwrap_or(Value::Null),
        "uuid": root_object.map(|item| item["uuid"].clone()).unwrap_or(Value::Null),
        "filesystem_size_bytes": root_object.map(|item| item["size_bytes"].clone()).unwrap_or(Value::Null),
        "partition_size_bytes": root_partition
            .and_then(|item| item["size_bytes"].as_u64())
            .map_or_else(|| root_object.map(|item| item["size_bytes"].clone()).unwrap_or(Value::Null), |size| json!(size)),
        "partition_type": root_partition.map(|item| item["mbr_type"].clone()).unwrap_or(Value::Null),
        "parent": root_object.map(|item| item["parent"].clone()).unwrap_or(Value::Null),
        "usage": filesystem_usage.cloned().unwrap_or(Value::Null),
    });
    let observation = json!({
        "disk_path": disk.path,
        "disk_name": disk.path.file_name().and_then(|name| name.to_str()).unwrap_or_default(),
        "disk_format": disk.format,
        "operating_system": linux,
        "linux_root_filesystem": root_filesystem,
        "users": users,
        "groups": groups,
        "inspection_notes": {
            "users": users_problem,
            "groups": groups_problem,
            "filesystem_usage": usage_problem,
        }
    });
    let mut outcome = success_outcome(observation, stderr);
    if usage_timed_out || users_timed_out || groups_timed_out {
        outcome.status = TestStatus::TimedOut;
        outcome.exit_code = None;
        outcome.diagnostics =
            "libguestfs Linux root filesystem inspection exceeded the configured timeout".into();
    }
    outcome
}

pub(super) fn partition_summary(objects: &[Value]) -> Vec<Value> {
    objects
        .iter()
        .filter(|object| object["type"] == "partition")
        .map(|object| {
            json!({
                "device": object["name"],
                "size_bytes": object["size_bytes"],
                "partition_type": object["mbr_type"],
            })
        })
        .collect()
}

#[derive(Debug)]
pub(super) struct ResolvedDisk {
    pub(super) path: PathBuf,
    pub(super) format: &'static str,
}

pub(super) fn resolve_disk(invocation: &TestInvocation) -> Result<ResolvedDisk, String> {
    let disk_path = invocation
        .parameters
        .get("disk_path")
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or("libguestfs test requires parameter disk_path")?;
    let requested = Path::new(disk_path);
    let resolved = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        invocation.project_archive_parent.join(requested)
    };
    let disk_name = invocation
        .parameters
        .get("disk_name")
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty());
    let manifest_candidates = manifest_disks_for_path(invocation, requested);
    let metadata = match fs::symlink_metadata(&resolved) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(format!(
                "disk_path {} is unavailable: {error}",
                resolved.display()
            ));
        }
    };
    if metadata
        .as_ref()
        .is_some_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err(format!(
            "disk_path {} must not be a symlink",
            resolved.display()
        ));
    }

    let selected = if metadata.as_ref().is_some_and(|metadata| metadata.is_file()) {
        if let Some(name) = disk_name {
            if Path::new(name).file_name().and_then(|part| part.to_str()) != Some(name)
                || resolved.file_name().and_then(|part| part.to_str()) != Some(name)
            {
                return Err(format!(
                    "disk_name {name:?} does not match disk file {}",
                    resolved.display()
                ));
            }
        }
        resolved
    } else if metadata.as_ref().is_some_and(|metadata| metadata.is_dir()) {
        let mut candidates = Vec::new();
        find_disk_files(&resolved, 0, &mut candidates)?;
        if let Some(manifest_matches) = manifest_candidates.as_ref() {
            let manifest_matches = manifest_matches
                .iter()
                .cloned()
                .collect::<std::collections::HashSet<_>>();
            let matches = candidates
                .iter()
                .filter(|path| {
                    path.canonicalize()
                        .is_ok_and(|canonical| manifest_matches.contains(&canonical))
                })
                .cloned()
                .collect::<Vec<_>>();
            if !matches.is_empty() {
                candidates = matches;
            }
        }
        if candidates.is_empty() {
            if let Some(manifest_matches) = manifest_candidates.clone() {
                choose_candidate(manifest_matches, disk_name, &resolved)?
            } else {
                return Err(format!(
                    "no compatible disk file was found under {}; specify disk_name or provide a .vdi, .vmdk, .qcow2, .vhd, .vhdx, .img, or .raw file",
                    resolved.display()
                ));
            }
        } else {
            choose_candidate(candidates, disk_name, &resolved)?
        }
    } else if let Some(candidates) = manifest_candidates {
        choose_candidate(candidates, disk_name, &resolved)?
    } else if metadata.is_none() {
        return Err(format!(
            "disk_path {} is unavailable and no matching imported VM disk was found in this delivery's import manifest",
            resolved.display()
        ));
    } else {
        return Err(format!(
            "disk_path {} is not a regular file or directory",
            resolved.display()
        ));
    };
    let format = disk_format(&selected).ok_or_else(|| {
        format!(
            "disk file {} has an unsupported extension; supported formats are {}",
            selected.display(),
            DISK_EXTENSIONS.join(", ")
        )
    })?;
    let canonical = selected
        .canonicalize()
        .map_err(|error| format!("cannot resolve disk file {}: {error}", selected.display()))?;
    Ok(ResolvedDisk {
        path: canonical,
        format,
    })
}

fn choose_candidate(
    mut candidates: Vec<PathBuf>,
    disk_name: Option<&str>,
    requested_path: &Path,
) -> Result<PathBuf, String> {
    if let Some(name) = disk_name {
        if Path::new(name).file_name().and_then(|part| part.to_str()) != Some(name) {
            return Err("disk_name must be a file name without directory components".into());
        }
        candidates
            .retain(|candidate| candidate.file_name().and_then(|part| part.to_str()) == Some(name));
    }
    candidates.sort();
    match candidates.as_slice() {
        [] => Err(format!(
            "no compatible disk file was found under {}; specify disk_name or provide a supported disk image",
            requested_path.display()
        )),
        [only] => Ok(only.clone()),
        many => Err(format!(
            "more than one compatible disk file was found under {} ({}); set disk_name to select one",
            requested_path.display(),
            many.iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

fn manifest_disks_for_path(
    invocation: &TestInvocation,
    requested_path: &Path,
) -> Option<Vec<PathBuf>> {
    if requested_path.is_absolute() {
        return None;
    }
    let manifest_path = invocation.report_dir.join("cleanup-manifest.json");
    let manifest: Value = serde_json::from_slice(&fs::read(manifest_path).ok()?).ok()?;
    if manifest["run_id"].as_str() != Some(invocation.run_id.as_str())
        || manifest["delivery_id"].as_str() != Some(invocation.delivery.delivery_id.as_str())
    {
        return None;
    }
    let vm_name = manifest["vm_name"].as_str()?;
    let project_vm_path = Path::new(manifest["project_vm_path"].as_str()?);
    let configured_path = normalize_relative_path(requested_path)?;
    let vm_root = normalize_relative_path(project_vm_path)?;
    let vm_directory = vm_root.join(vm_name);
    if configured_path != vm_root && configured_path != vm_directory {
        return None;
    }
    let storage_path = PathBuf::from(manifest["storage_path"].as_str()?)
        .canonicalize()
        .ok()?;
    let paths = manifest["disk_paths"].as_array()?;
    Some(
        paths
            .iter()
            .filter_map(Value::as_str)
            .map(PathBuf::from)
            .filter(|path| disk_format(path).is_some())
            .filter(|path| {
                fs::symlink_metadata(path)
                    .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
            })
            .filter_map(|path| path.canonicalize().ok())
            .filter(|path| path.starts_with(&storage_path))
            .collect(),
    )
}

fn normalize_relative_path(path: &Path) -> Option<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(part) => normalized.push(part),
            _ => return None,
        }
    }
    Some(normalized)
}

fn find_disk_files(directory: &Path, depth: usize, found: &mut Vec<PathBuf>) -> Result<(), String> {
    if depth > 32 {
        return Ok(());
    }
    let entries = fs::read_dir(directory).map_err(|error| {
        format!(
            "cannot search disk directory {}: {error}",
            directory.display()
        )
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("cannot read disk directory entry: {error}"))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            find_disk_files(&path, depth + 1, found)?;
        } else if metadata.is_file() && disk_format(&path).is_some() {
            found.push(path);
        }
    }
    Ok(())
}

fn disk_format(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "vdi" => Some("vdi"),
        "vmdk" => Some("vmdk"),
        "qcow2" => Some("qcow2"),
        "vhd" => Some("vpc"),
        "vhdx" => Some("vhdx"),
        "img" | "raw" => Some("raw"),
        _ => None,
    }
}

pub(super) async fn run_filesystems(
    invocation: &TestInvocation,
    disk: &ResolvedDisk,
    started: Instant,
) -> Result<ExternalProcessResult, String> {
    run_tool(
        invocation,
        "virt-filesystems",
        vec![
            format!("--format={}", disk.format),
            "--all".into(),
            "--long".into(),
            "--uuid".into(),
            "--csv".into(),
            "-a".into(),
            disk.path.to_string_lossy().into_owned(),
        ],
        started,
    )
    .await
}

async fn run_inspector(
    invocation: &TestInvocation,
    disk: &ResolvedDisk,
    started: Instant,
) -> Result<ExternalProcessResult, String> {
    run_tool(
        invocation,
        "virt-inspector",
        vec![
            format!("--format={}", disk.format),
            "--no-applications".into(),
            "--no-icon".into(),
            "-a".into(),
            disk.path.to_string_lossy().into_owned(),
        ],
        started,
    )
    .await
}

pub(super) async fn run_tool(
    invocation: &TestInvocation,
    executable: &str,
    arguments: Vec<String>,
    started: Instant,
) -> Result<ExternalProcessResult, String> {
    let remaining = invocation.timeout.saturating_sub(started.elapsed());
    if remaining.is_zero() {
        return Ok(ExternalProcessResult {
            exit_code: None,
            stdout: String::new(),
            stderr: format!("{executable} was not started because the test timeout expired"),
            elapsed: started.elapsed(),
            timed_out: true,
            stdout_truncated: false,
            stderr_truncated: false,
        });
    }
    run_external_process(
        Path::new(executable),
        &arguments,
        &invocation.project_root,
        &[],
        remaining,
        invocation.runtime.output_limit,
    )
    .await
    .map_err(|error| {
        if error.contains("No such file or directory") {
            format!(
                "{executable} is unavailable; install libguestfs-tools on the thotd server and make sure the executable is on thotd's PATH"
            )
        } else {
            format!("cannot run {executable}: {error}")
        }
    })
}

pub(super) async fn collect_usage(
    invocation: &TestInvocation,
    disk: &ResolvedDisk,
    started: Instant,
) -> (Option<Vec<Value>>, Option<String>, bool) {
    match run_tool(
        invocation,
        "virt-df",
        vec![
            format!("--format={}", disk.format),
            "--csv".into(),
            "-a".into(),
            disk.path.to_string_lossy().into_owned(),
        ],
        started,
    )
    .await
    {
        Ok(output) if output.exit_code == Some(0) && !output.timed_out => {
            match parse_usage_csv(&output.stdout) {
                Ok(records) => (Some(records), None, false),
                Err(error) => (
                    None,
                    Some(format!("cannot parse CSV output: {error}")),
                    false,
                ),
            }
        }
        Ok(output) if output.timed_out => (None, Some("command timed out".into()), true),
        Ok(output) => (
            None,
            Some(format!(
                "command exited with {:?}: {}",
                output.exit_code,
                output.stderr.trim()
            )),
            false,
        ),
        Err(error) => (None, Some(error), false),
    }
}

async fn collect_guest_records(
    invocation: &TestInvocation,
    disk: &ResolvedDisk,
    started: Instant,
    guest_path: &str,
    parse: fn(&str) -> Vec<Value>,
) -> (Option<Vec<Value>>, Option<String>, bool) {
    match run_tool(
        invocation,
        "virt-cat",
        vec![
            format!("--format={}", disk.format),
            "-a".into(),
            disk.path.to_string_lossy().into_owned(),
            guest_path.into(),
        ],
        started,
    )
    .await
    {
        Ok(output) if output.exit_code == Some(0) && !output.timed_out => {
            (Some(parse(&output.stdout)), None, false)
        }
        Ok(output) if output.timed_out => (None, Some("command timed out".into()), true),
        Ok(output) => (
            None,
            Some(format!(
                "command exited with {:?}: {}",
                output.exit_code,
                output.stderr.trim()
            )),
            false,
        ),
        Err(error) => (None, Some(error), false),
    }
}

fn disk_observation(disk: &ResolvedDisk, objects: Vec<Value>, usage: Option<Vec<Value>>) -> Value {
    let disks = objects
        .iter()
        .filter(|item| item["type"] == "device")
        .cloned()
        .collect::<Vec<_>>();
    let partitions = objects
        .iter()
        .filter(|item| item["type"] == "partition")
        .cloned()
        .collect::<Vec<_>>();
    let filesystems = objects
        .iter()
        .filter(|item| item["type"] == "filesystem")
        .cloned()
        .collect::<Vec<_>>();
    json!({
        "disk_path": disk.path,
        "disk_name": disk.path.file_name().and_then(|name| name.to_str()).unwrap_or_default(),
        "disk_format": disk.format,
        "image_file_size_bytes": fs::metadata(&disk.path).map(|metadata| metadata.len()).ok(),
        "disks": disks,
        "partitions": partitions,
        "filesystems": filesystems,
        "storage_objects": objects,
        "filesystem_usage": usage,
    })
}

pub(super) fn parse_filesystems_csv(output: &str) -> Result<Vec<Value>, String> {
    let rows = parse_csv(output)?;
    let Some(headers) = rows.first() else {
        return Ok(Vec::new());
    };
    let index = |name: &str| {
        headers
            .iter()
            .position(|header| header.trim().eq_ignore_ascii_case(name))
    };
    let name_index = index("Name").ok_or("CSV output has no Name column")?;
    let type_index = index("Type").ok_or("CSV output has no Type column")?;
    Ok(rows
        .iter()
        .skip(1)
        .filter(|row| !row.iter().all(|field| field.trim().is_empty()))
        .map(|row| {
            let field = |column: Option<usize>| {
                column
                    .and_then(|i| row.get(i))
                    .map(String::as_str)
                    .filter(|value| !value.is_empty() && *value != "-")
            };
            let size = field(index("Size")).and_then(|value| value.parse::<u64>().ok());
            let mut item = Map::new();
            item.insert(
                "name".into(),
                json!(row.get(name_index).cloned().unwrap_or_default()),
            );
            item.insert(
                "type".into(),
                json!(row.get(type_index).cloned().unwrap_or_default()),
            );
            insert_optional(&mut item, "filesystem_type", field(index("VFS")));
            insert_optional(&mut item, "label", field(index("Label")));
            insert_optional(&mut item, "parent", field(index("Parent")));
            insert_optional(&mut item, "uuid", field(index("UUID")));
            insert_optional(&mut item, "mbr_type", field(index("MBR")));
            item.insert(
                "size_bytes".into(),
                size.map_or(Value::Null, |size| json!(size)),
            );
            Value::Object(item)
        })
        .collect())
}

fn parse_usage_csv(output: &str) -> Result<Vec<Value>, String> {
    let rows = parse_csv(output)?;
    let Some(headers) = rows.first() else {
        return Ok(Vec::new());
    };
    let index = |name: &str| {
        headers
            .iter()
            .position(|header| header.trim().eq_ignore_ascii_case(name))
    };
    let device_index = index("Filesystem").ok_or("CSV output has no Filesystem column")?;
    let total_index = index("1K-blocks").or_else(|| index("Size"));
    let used_index = index("Used");
    let available_index = index("Available");
    let percent_index = index("Use%");
    Ok(rows
        .iter()
        .skip(1)
        .filter(|row| !row.iter().all(|field| field.trim().is_empty()))
        .map(|row| {
            let value = |column: Option<usize>| column.and_then(|i| row.get(i)).map(String::as_str);
            let parse_u64 =
                |column| value(column).and_then(|text| text.replace(',', "").parse::<u64>().ok());
            let device = value(Some(device_index)).unwrap_or_default();
            let device = device.rsplit_once(':').map_or(device, |(_, device)| device);
            let mut item = Map::new();
            item.insert("device".into(), json!(device));
            if let Some(total) = parse_u64(total_index) {
                item.insert("total_kib".into(), json!(total));
            }
            if let Some(used) = parse_u64(used_index) {
                item.insert("used_kib".into(), json!(used));
            }
            if let Some(available) = parse_u64(available_index) {
                item.insert("available_kib".into(), json!(available));
            }
            if let Some(percent) =
                value(percent_index).and_then(|text| text.trim_end_matches('%').parse::<f64>().ok())
            {
                item.insert("used_percent".into(), json!(percent));
            }
            Value::Object(item)
        })
        .collect())
}

fn parse_operating_systems(xml: &str) -> Vec<Value> {
    let Ok(block_regex) = regex::Regex::new(r"(?s)<operatingsystem>(.*?)</operatingsystem>") else {
        return Vec::new();
    };
    let Ok(root_mount_regex) =
        regex::Regex::new(r#"(?s)<mountpoint\s+dev="([^"]+)">/</mountpoint>"#)
    else {
        return Vec::new();
    };
    block_regex
        .captures_iter(xml)
        .filter_map(|capture| {
            let block = capture.get(1)?.as_str();
            let mut os = Map::new();
            for tag in [
                "name",
                "distro",
                "product_name",
                "arch",
                "major_version",
                "minor_version",
            ] {
                if let Some(value) = xml_tag(block, tag) {
                    os.insert(
                        if tag == "name" { "type" } else { tag }.into(),
                        json!(value),
                    );
                }
            }
            if let Some(root) = xml_tag(block, "root").or_else(|| {
                root_mount_regex
                    .captures(block)
                    .and_then(|capture| capture.get(1))
                    .map(|value| xml_unescape(value.as_str()))
            }) {
                os.insert("root_device".into(), json!(root));
            }
            (!os.is_empty()).then_some(Value::Object(os))
        })
        .collect()
}

fn xml_tag(xml: &str, tag: &str) -> Option<String> {
    let pattern = format!(r"(?s)<{tag}>(.*?)</{tag}>");
    let regex = regex::Regex::new(&pattern).ok()?;
    regex
        .captures(xml)
        .and_then(|capture| capture.get(1))
        .map(|value| xml_unescape(value.as_str().trim()))
}

fn xml_unescape(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

pub(super) fn parse_passwd(text: &str) -> Vec<Value> {
    text.lines()
        .filter_map(|line| {
            let fields = line.split(':').collect::<Vec<_>>();
            (fields.len() >= 7).then(|| {
                json!({
                    "username": fields[0],
                    "uid": fields[2].parse::<u32>().ok(),
                    "gid": fields[3].parse::<u32>().ok(),
                    "home": fields[5],
                    "shell": fields[6],
                })
            })
        })
        .collect()
}

pub(super) fn parse_group(text: &str) -> Vec<Value> {
    text.lines()
        .filter_map(|line| {
            let fields = line.split(':').collect::<Vec<_>>();
            (fields.len() >= 4).then(|| {
                json!({
                    "group": fields[0],
                    "gid": fields[2].parse::<u32>().ok(),
                    "members": fields[3].split(',').filter(|member| !member.is_empty()).collect::<Vec<_>>(),
                })
            })
        })
        .collect()
}

pub(super) fn parse_csv(input: &str) -> Result<Vec<Vec<String>>, String> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut chars = input.chars().peekable();
    let mut quoted = false;
    while let Some(character) = chars.next() {
        match character {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => row.push(std::mem::take(&mut field)),
            '\n' if !quoted => {
                row.push(std::mem::take(&mut field));
                if row.last().is_some_and(|value| value.ends_with('\r')) {
                    if let Some(last) = row.last_mut() {
                        last.pop();
                    }
                }
                rows.push(std::mem::take(&mut row));
            }
            _ => field.push(character),
        }
    }
    if quoted {
        return Err("unterminated quoted field".into());
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    Ok(rows)
}

fn insert_optional(map: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    map.insert(key.into(), value.map_or(Value::Null, |value| json!(value)));
}

pub(super) fn success_outcome(observation: Value, stderr: String) -> TestOutcome {
    let stdout = match serde_yaml::to_string(&observation) {
        Ok(output) => output,
        Err(error) => {
            return TestOutcome::error(format!(
                "cannot serialize libguestfs result as YAML: {error}"
            ));
        }
    };
    TestOutcome {
        status: TestStatus::Passed,
        stdout,
        stderr,
        exit_code: Some(0),
        diagnostics: String::new(),
        observation,
        evidence: Vec::new(),
    }
}

pub(super) fn failed_command(name: &str, output: ExternalProcessResult) -> TestOutcome {
    let timed_out = output.timed_out;
    let diagnostics = if timed_out {
        format!("{name} exceeded the configured test timeout")
    } else {
        format!("{name} exited with {:?}", output.exit_code)
    };
    TestOutcome {
        status: if timed_out {
            TestStatus::TimedOut
        } else {
            TestStatus::Failed
        },
        stdout: output.stdout,
        stderr: output.stderr,
        exit_code: output.exit_code,
        diagnostics,
        observation: json!({}),
        evidence: Vec::new(),
    }
}

pub(super) fn labeled_stderr(name: &str, stderr: &str) -> String {
    if stderr.trim().is_empty() {
        String::new()
    } else {
        format!("{name}: {}", stderr.trim())
    }
}

pub(super) fn append_stderr(target: &mut String, addition: &str) {
    if addition.trim().is_empty() {
        return;
    }
    if !target.is_empty() {
        target.push('\n');
    }
    target.push_str(addition.trim());
}
