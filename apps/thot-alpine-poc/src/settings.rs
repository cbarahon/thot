//! Expand professor-facing YAML before the same validation used by the full schema.
use serde::Deserialize;
use serde_yaml::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use thot_core::sandbox_project::SandboxProject;

#[derive(Debug, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Runtime {
    pub storage_root: PathBuf,
    pub temp_dir: Option<PathBuf>,
    pub host_command_timeout_seconds: u32,
    pub agent_idle_seconds: u32,
    pub guest_boot_timeout_seconds: u32,
    pub transfer_timeout_seconds: u32,
}

fn merge(base: &mut Value, overlay: Value) {
    if let (Some(base), Some(over)) = (base.as_mapping_mut(), overlay.as_mapping()) {
        for (key, value) in over {
            merge(
                base.entry(key.clone()).or_insert(Value::Null),
                value.clone(),
            );
        }
    } else {
        *base = overlay;
    }
}

pub fn load(project: &Path) -> Result<(SandboxProject, Runtime), String> {
    let config = fs::read(project.join("configuration.thot.yaml")).map_err(|e| e.to_string())?;
    if config.len() > 1024 * 1024 {
        return Err("project configuration exceeds 1 MiB".into());
    }
    let mut raw: Value = serde_yaml::from_slice(&config).map_err(|e| e.to_string())?;
    let mut defaults: Value =
        serde_yaml::from_str(include_str!("../../../deploy/sandbox-defaults.yaml"))
            .map_err(|e| e.to_string())?;
    let path = Path::new("/etc/thot/sandbox-defaults.yaml");
    match fs::read(path) {
        Ok(data) => merge(
            &mut defaults,
            serde_yaml::from_slice(&data).map_err(|e| format!("{}: {e}", path.display()))?,
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    }
    if let Some(config_home) = std::env::var_os("XDG_CONFIG_HOME") {
        let path = PathBuf::from(config_home).join("thot/sandbox-defaults.yaml");
        match fs::read(&path) {
            Ok(data) => merge(
                &mut defaults,
                serde_yaml::from_slice(&data).map_err(|e| format!("{}: {e}", path.display()))?,
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        }
    } else if let Some(home) = std::env::var_os("HOME") {
        let path = PathBuf::from(home).join(".config/thot/sandbox-defaults.yaml");
        match fs::read(&path) {
            Ok(data) => merge(
                &mut defaults,
                serde_yaml::from_slice(&data).map_err(|e| format!("{}: {e}", path.display()))?,
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        }
    }
    let mut runtime = defaults["runtime"].clone();
    let mapping = raw
        .as_mapping_mut()
        .ok_or("expected a YAML project mapping")?;
    if let Some(over) = mapping.remove("runtime") {
        merge(&mut runtime, over);
    }
    let mut runtime: Runtime = serde_yaml::from_value(runtime).map_err(|e| e.to_string())?;
    let mut vars = BTreeMap::new();
    let env_file = project.join(".env");
    match dotenvy::from_path_iter(&env_file) {
        Ok(entries) => {
            for entry in entries {
                let (key, value) = entry.map_err(|e| format!(".env: {e}"))?;
                vars.insert(key, value);
            }
        }
        Err(dotenvy::Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(format!(".env: {e}")),
    }
    let setting = |name: &str| std::env::var(name).ok().or_else(|| vars.get(name).cloned());
    if let Some(v) = setting("THOT_KVM_ALPINE_ROOT") {
        runtime.storage_root = v.into();
    }
    if let Some(v) = setting("TMPDIR") {
        runtime.temp_dir = Some(v.into());
    }
    for (key, value) in [
        (
            "THOT_HOST_COMMAND_TIMEOUT_SECONDS",
            &mut runtime.host_command_timeout_seconds,
        ),
        ("THOT_AGENT_IDLE_SECONDS", &mut runtime.agent_idle_seconds),
        (
            "THOT_TRANSFER_TIMEOUT_SECONDS",
            &mut runtime.transfer_timeout_seconds,
        ),
        (
            "THOT_GUEST_BOOT_TIMEOUT_SECONDS",
            &mut runtime.guest_boot_timeout_seconds,
        ),
    ] {
        if let Some(v) = setting(key) {
            *value = v.parse().map_err(|_| format!("invalid {key}"))?;
        }
        if !(1..=86400).contains(value) {
            return Err(format!("{key} must be 1..86400 seconds"));
        }
    }
    if runtime.agent_idle_seconds <= 5 {
        return Err("agent_idle_seconds must exceed the five-second heartbeat interval".into());
    }
    if !runtime.storage_root.is_absolute()
        || runtime.temp_dir.as_ref().is_some_and(|p| !p.is_absolute())
    {
        return Err("storage_root and temp_dir must be absolute paths".into());
    }
    for (key, value) in [
        ("students_file", "students.yaml"),
        ("assignments_file", "assignments.yaml"),
    ] {
        mapping.entry(Value::from(key)).or_insert(value.into());
    }
    let project_id = mapping
        .get("project_id")
        .and_then(Value::as_str)
        .ok_or("missing project_id")?
        .to_owned();
    let platforms = mapping
        .get_mut("sandbox_platforms")
        .and_then(Value::as_sequence_mut)
        .ok_or("expected sandbox_platforms list")?;
    for platform in platforms {
        let pm = platform
            .as_mapping_mut()
            .ok_or("expected platform mapping")?;
        let profile = pm.remove("profile");
        let resources = pm.remove("resources");
        let preparation = pm.remove("preparation");
        let mut expanded = defaults["platform"].clone();
        let mut test_defaults = defaults["test"].clone();
        if let Some(key) = profile {
            let profile = defaults["profiles"]
                .get(&key)
                .ok_or_else(|| format!("unknown sandbox profile {key:?}"))?;
            merge(&mut expanded, profile["platform"].clone());
            if !profile["test"].is_null() {
                merge(&mut test_defaults, profile["test"].clone());
            }
        }
        merge(&mut expanded, std::mem::take(platform));
        if expanded["id"].is_null() {
            expanded["id"] = "kvm-alpine".into();
        }
        let identity = format!(
            "{:x}",
            Sha256::digest(format!(
                "{project_id}-{:?}-{:?}",
                expanded["id"], expanded["lifecycle"]
            ))
        );
        if !expanded["lifecycle"].is_mapping()
            || ["agent", "template_vm", "project_clone", "student_clone"]
                .iter()
                .any(|k| !expanded["lifecycle"][*k].is_mapping())
        {
            return Err("lifecycle and its VM, agent and clone settings must be mappings".into());
        }
        let resource_defaults = expanded["lifecycle"]["template_vm"].clone();
        let life = &mut expanded["lifecycle"];
        if life["agent"]["version"] == "installed" {
            life["agent"]["version"] = env!("CARGO_PKG_VERSION").into();
        }
        if life["template_name"].is_null() {
            life["template_name"] = format!(
                "thot-alpine-{}g",
                life["template_vm"]["disk_gib"].as_u64().unwrap_or(0)
            )
            .into();
        }
        if life["project_clone"]["name"].is_null() {
            life["project_clone"]["name"] = format!("thot-project-{}", &identity[..12]).into();
        }
        if life["student_clone"]["name_prefix"].is_null() {
            life["student_clone"]["name_prefix"] =
                format!("thot-student-{}", &identity[..12]).into();
        }
        if let Some(tests) = expanded["tests"].as_sequence_mut() {
            for test in tests {
                let mut settings = test_defaults.clone();
                merge(&mut settings, std::mem::take(test));
                *test = settings;
            }
        }
        // A professor declares VM sizing once, in template_vm. The older
        // normalized Rust contract still carries provider limits separately;
        // derive those fields here so one project value cannot contradict
        // itself. Host administrator ceilings are checked by provider preflight.
        for key in ["vcpu", "memory_mib", "disk_gib"] {
            let value = resource_defaults[key].clone();
            expanded["limits"][key] = value;
        }
        if expanded["limits"]["preparation_timeout_seconds"].is_null() {
            expanded["limits"]["preparation_timeout_seconds"] =
                expanded["lifecycle"]["preparation"]["timeout_seconds"].clone();
        }
        if let Some(resources) = resources {
            let resources = resources
                .as_mapping()
                .ok_or("platform resources must be a mapping")?;
            for (source, target) in [
                ("cpus", "vcpu"),
                ("vcpu", "vcpu"),
                ("memory_mib", "memory_mib"),
                ("disk_gib", "disk_gib"),
            ] {
                if let Some(value) = resources.get(Value::from(source)) {
                    if !value.is_u64()
                        || resources.contains_key(Value::from(match source {
                            "cpus" => "vcpu",
                            "vcpu" => "cpus",
                            _ => "__never__",
                        }))
                    {
                        return Err(format!(
                            "platform resources.{source} must be an integer and aliases must not be duplicated"
                        ));
                    }
                    expanded["lifecycle"]["template_vm"][target] = value.clone();
                }
            }
            for key in resources.keys().filter_map(Value::as_str) {
                if !matches!(key, "cpus" | "vcpu" | "memory_mib" | "disk_gib") {
                    return Err(format!("unknown platform resource `{key}`"));
                }
            }
        }
        if let Some(preparation) = preparation {
            merge(&mut expanded["lifecycle"]["preparation"], preparation);
        }
        for key in ["vcpu", "memory_mib", "disk_gib"] {
            expanded["limits"][key] = expanded["lifecycle"]["template_vm"][key].clone();
        }
        expanded["limits"]["preparation_timeout_seconds"] =
            expanded["lifecycle"]["preparation"]["timeout_seconds"].clone();
        *platform = expanded;
    }
    let config: SandboxProject = serde_yaml::from_value(raw).map_err(|e| e.to_string())?;
    config.validate()?;
    Ok((config, runtime))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compact_ova_project_preserves_grading_and_resolves_technical_defaults() {
        let project =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/ova-screening-kvm-alpine");
        let (cfg, _) = load(&project).unwrap();
        let platform = &cfg.sandbox_platforms[0];
        assert_eq!(platform.tests.len(), 4);
        assert_eq!(platform.lifecycle.template_vm.disk_gib, 100);
        assert_eq!(platform.tests[0].timeout_seconds, 3600);
        assert_eq!(platform.lifecycle.template_vm.vcpu, platform.limits.vcpu);
        assert_eq!(
            platform.lifecycle.template_vm.memory_mib,
            platform.limits.memory_mib
        );
        assert!(
            platform
                .tests
                .iter()
                .all(|t| t.module.is_some() && t.expect_exit_code == 0)
        );
        assert_eq!(platform.evaluation.rubric.as_ref().unwrap().scale, 10);
        assert_eq!(platform.lifecycle.agent.version, env!("CARGO_PKG_VERSION"));
    }
}
