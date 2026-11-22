use std::{
    env,
    path::{Path, PathBuf},
    str::FromStr,
    time::Duration,
};

#[derive(Debug, Clone)]
pub struct KvmAlpineConfig {
    pub virsh_path: PathBuf,
    pub virt_install_path: PathBuf,
    pub qemu_img_path: PathBuf,
    pub storage_root: PathBuf,
    pub iso_cache: PathBuf,
    pub thot_alpine_agent_path: PathBuf,
    pub libvirt_uri: String,
    pub max_cpu: u32,
    pub max_ram: u64,
    pub max_disk: u64,
    pub min_cpu_reserve: u32,
    pub min_ram_reserve: u64,
    pub min_disk_reserve: u64,
    pub alpine_mirror: String,
    pub alpine_trusted_keyring: PathBuf,
    pub alpine_trusted_fingerprint: String,
    pub https_mirror_allowlist: String,
    pub max_iso_bytes: u64,
    pub max_input_bytes: u64,
    pub max_staged_bytes: u64,
    pub host_cpu_reserves: u32,
    pub host_ram_reserves: u64,
    pub host_disk_reserves: u64,
    pub max_vcpu_ceiling: u32,
    pub max_memory_ceiling: u64,
    pub max_disk_ceiling: u64,
    pub max_process_ceiling: u64,
    pub bind_host: String,
    pub database_url: String,
    pub artifact_root: PathBuf,
}

impl KvmAlpineConfig {
    pub fn from_env() -> Result<Self, String> {
        let base = PathBuf::from(env::var("THOTD_DATA_ROOT").unwrap_or_else(|_| "/var/lib/thot".into()));
        let env_path = |key: &str, fallback: PathBuf| env::var_os(key).map(PathBuf::from).unwrap_or(fallback);
        let parse_positive = |key: &str, fallback: u64| parse_env(key, fallback)?;
        let parse_bool = |key: &str, fallback: bool| parse_env(key, fallback)?;

        let config = Self {
            virsh_path: env_path("THOTD_VIRSH_PATH", PathBuf::from("/usr/bin/virsh")),
            virt_install_path: env_path("THOTD_VIRT_INSTALL_PATH", PathBuf::from("/usr/bin/virt-install")),
            qemu_img_path: env_path("THOTD_QEMU_IMG_PATH", PathBuf::from("/usr/bin/qemu-img")),
            storage_root: env_path("THOTD_STORAGE_ROOT", base.join("alpine-vms")),
            iso_cache: env_path("THOTD_ISO_CACHE", base.join("alpine-isos")),
            thot_alpine_agent_path: env_path("THOTD_THOT_ALPINE_AGENT_PATH", PathBuf::from("/usr/local/bin/thot-alpine-agent")),
            libvirt_uri: env::var("THOTD_LIBVIRT_URI").map_or_else(|_| Err("missing THOTD_LIBVIRT_URI"), |uri| {
                if uri.parse::<libvirt::Uri>().is_err() {
                    Err("invalid libvirt URI")
                } else {
                    Ok(uri)
                }
            })?,
            max_cpu: parse_positive("THOTD_MAX_CPU", 4u32)?,
            max_ram: parse_positive("THOTD_MAX_RAM", 4 * 1024 * 1024 * 1024u64)?,
            max_disk: parse_positive("THOTD_MAX_DISK", 50 * 1024 * 1024 * 1024u64)?,
            min_cpu_reserve: parse_positive("THOTD_MIN_CPU_RESERVE", 1u32)?,
            min_ram_reserve: parse_positive("THOTD_MIN_RAM_RESERVE", 2 * 1024 * 1024 * 1024u64)?,
            min_disk_reserve: parse_positive("THOTD_MIN_DISK_RESERVE", 10 * 1024 * 1024 * 1024u64)?,
            alpine_mirror: env::var("THOTD_ALPINE_MIRROR").unwrap_or_else(|_| "http://dl-cdn.alpinelinux.org/alpine/latest-stable".into()),
            alpine_trusted_keyring: env_path("THOTD_ALPINE_TRUSTED_KEYRING", PathBuf::from("/etc/thot/trusted-keys.gpg")),
            alpine_trusted_fingerprint: env::var("THOTD_ALPINE_TRUSTED_FINGERPRINT").map_or_else(|_| Err("missing THOTD_ALPINE_TRUSTED_FINGERPRINT"), |fp| {
                if fp.parse::<keyring::Fingerprint>().is_err() {
                    Err("invalid keyring fingerprint")
                } else {
                    Ok(fp)
                }
            })?,
            https_mirror_allowlist: env::var("THOTD_HTTPS_MIRROR_ALLOWLIST").unwrap_or_else(|_| "https://dl-cdn.alpinelinux.org/alpine/latest-stable".into()),
            max_iso_bytes: parse_positive("THOTD_MAX_ISO_BYTES", 2 * 1024 * 1024 * 1024u64)?,
            max_input_bytes: parse_positive("THOTD_MAX_INPUT_BYTES", 10 * 1024 * 1024 * 1024u64)?,
            max_staged_bytes: parse_positive("THOTD_MAX_STAGED_BYTES", 5 * 1024 * 1024 * 1024u64)?,
            host_cpu_reserves: parse_positive("THOTD_HOST_CPU_RESERVES", 0u32)?,
            host_ram_reserves: parse_positive("THOTD_HOST_RAM_RESERVES", 0u64)?,
            host_disk_reserves: parse_positive("THOTD_HOST_DISK_RESERVES", 0u64)?,
            max_vcpu_ceiling: parse_positive("THOTD_MAX_VCPU_CEILING", 4u32)?,
            max_memory_ceiling: parse_positive("THOTD_MAX_MEMORY_CEILING", 8 * 1024 * 1024 * 1024u64)?,
            max_disk_ceiling: parse_positive("THOTD_MAX_DISK_CEILING", 100 * 1024 * 1024 * 1024u64)?,
            max_process_ceiling: parse_positive("THOTD_MAX_PROCESS_CEILING", 256u64)?,
            bind_host: env::var("THOTD_BIND_HOST")
                .or_else(|_| env::var("THOTD_HOST"))
                .unwrap_or_else(|_| "127.0.0.1".into()),
            database_url: env::var("THOTD_DATABASE_URL")
                .or_else(|_| env::var("THOTD_DATABASE"))
                .unwrap_or_else(|_| format!("sqlite:{}", base.join("thot.sqlite3").display())),
            artifact_root: env_path("THOTD_ARTIFACT_ROOT", base.join("artifacts")),
        };

        validate_paths(&config)?;

        Ok(config)
    }

    pub fn to_runner_settings(&self) -> Result<thot_runner::kvm_alpine::KvmAlpineSettings, String> {
        thot_runner::kvm_alpine::KvmAlpineSettings {
            virsh_path: self.virsh_path.clone(),
            virt_install_path: self.virt_install_path.clone(),
            qemu_img_path: self.qemu_img_path.clone(),
            storage_root: self.storage_root.clone(),
            iso_cache: self.iso_cache.clone(),
            thot_alpine_agent_path: self.thot_alpine_agent_path.clone(),
            libvirt_uri: self.libvirt_uri.clone(),
            max_cpu: self.max_cpu,
            max_ram: self.max_ram,
            max_disk: self.max_disk,
            min_cpu_reserve: self.min_cpu_reserve,
            min_ram_reserve: self.min_ram_reserve,
            min_disk_reserve: self.min_disk_reserve,
            alpine_mirror: self.alpine_mirror.clone(),
            alpine_trusted_keyring: self.alpine_trusted_keyring.clone(),
            alpine_trusted_fingerprint: self.alpine_trusted_fingerprint.clone(),
            https_mirror_allowlist: self.https_mirror_allowlist.clone(),
            max_iso_bytes: self.max_iso_bytes,
            max_input_bytes: self.max_input_bytes,
            max_staged_bytes: self.max_staged_bytes,
            host_cpu_reserves: self.host_cpu_reserves,
            host_ram_reserves: self.host_ram_reserves,
            host_disk_reserves: self.host_disk_reserves,
            max_vcpu_ceiling: self.max_vcpu_ceiling,
            max_memory_ceiling: self.max_memory_ceiling,
            max_disk_ceiling: self.max_disk_ceiling,
            max_process_ceiling: self.max_process_ceiling,
            bind_host: self.bind_host.clone(),
            database_url: self.database_url.clone(),
            artifact_root: self.artifact_root.clone(),
        }
        .map_err(|err| err.to_string())
    }
}

fn validate_paths(config: &KvmAlpineConfig) -> Result<(), String> {
    if !config.storage_root.is_absolute() {
        return Err("storage root must be an absolute path");
    }
    if !config.iso_cache.is_absolute() {
        return Err("ISO cache must be an absolute path");
    }
    if !config.thot_alpine_agent_path.is_absolute() {
        return Err("thot-alpine-agent must be an absolute path");
    }
    if !config.alpine_trusted_keyring.is_absolute() {
        return Err("alpine trusted keyring must be an absolute path");
    }
    if !config.artifact_root.is_absolute() {
        return Err("artifact root must be an absolute path");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_env() {
        std::env::set_var("THOTD_DATA_ROOT", "/var/lib/thot");
        std::env::set_var("THOTD_VIRSH_PATH", "/usr/bin/virsh");
        std::env::set_var("THOTD_VIRT_INSTALL_PATH", "/usr/bin/virt-install");
        std::env::set_var("THOTD_QEMU_IMG_PATH", "/usr/bin/qemu-img");
        std::env::set_var("THOTD_STORAGE_ROOT", "/var/lib/thot/alpine-vms");
        std::env::set_var("THOTD_ISO_CACHE", "/var/lib/thot/alpine-isos");
        std::env::set_var("THOTD_THOT_ALPINE_AGENT_PATH", "/usr/local/bin/thot-alpine-agent");
        std::env::set_var("THOTD_LIBVIRT_URI", "qemu:///system");
        std::env::set_var("THOTD_MAX_CPU", "4");
        std::env::set_var("THOTD_MAX_RAM", "4G");
        std::env::set_var("THOTD_MAX_DISK", "50G");
        std::env::set_var("THOTD_MIN_CPU_RESERVE", "1");
        std::env::set_var("THOTD_MIN_RAM_RESERVE", "2G");
        std::env::set_var("THOTD_MIN_DISK_RESERVE", "10G");
        std::env::set_var("THOTD_ALPINE_MIRROR", "http://dl-cdn.alpinelinux.org/alpine/latest-stable");
        std::env::set_var("THOTD_ALPINE_TRUSTED_KEYRING", "/etc/thot/trusted-keys.gpg");
        std::env::set_var("THOTD_ALPINE_TRUSTED_FINGERPRINT", "ABCDEF1234567890");
        std::env::set_var("THOTD_HTTPS_MIRROR_ALLOWLIST", "https://dl-cdn.alpinelinux.org/alpine/latest-stable");
        std::env::set_var("THOTD_MAX_ISO_BYTES", "2G");
        std::env::set_var("THOTD_MAX_INPUT_BYTES", "10G");
        std::env::set_var("THOTD_MAX_STAGED_BYTES", "5G");
        std::env::set_var("THOTD_HOST_CPU_RESERVES", "0");
        std::env::set_var("THOTD_HOST_RAM_RESERVES", "0G");
        std::env::set_var("THOTD_HOST_DISK_RESERVES", "0G");
        std::env::set_var("THOTD_MAX_VCPU_CEILING", "4");
        std::env::set_var("THOTD_MAX_MEMORY_CEILING", "8G");
        std::env::set_var("THOTD_MAX_DISK_CEILING", "100G");
        std::env::set_var("THOTD_MAX_PROCESS_CEILING", "256");
        std::env::set_var("THOTD_BIND_HOST", "127.0.0.1");
        std::env::set_var("THOTD_DATABASE_URL", "sqlite:/var/lib/thot/thot.sqlite3");
        std::env::set_var("THOTD_ARTIFACT_ROOT", "/var/lib/thot/artifacts");

        let config = KvmAlpineConfig::from_env().unwrap();
        assert_eq!(config.virsh_path, PathBuf::from("/usr/bin/virsh"));
        assert_eq!(config.virt_install_path, PathBuf::from("/usr/bin/virt-install"));
        assert_eq!(config.qemu_img_path, PathBuf::from("/usr/bin/qemu-img"));
        assert_eq!(config.storage_root, PathBuf::from("/var/lib/thot/alpine-vms"));
        assert_eq!(config.iso_cache, PathBuf::from("/var/lib/thot/alpine-isos"));
        assert_eq!(config.thot_alpine_agent_path, PathBuf::from("/usr/local/bin/thot-alpine-agent"));
        assert_eq!(config.libvirt_uri, "qemu:///system");
        assert_eq!(config.max_cpu, 4);
        assert_eq!(config.max_ram, 4 * 1024 * 1024 * 1024);
        assert_eq!(config.max_disk, 50 * 1024 * 1024 * 1024);
        assert_eq!(config.min_cpu_reserve, 1);
        assert_eq!(config.min_ram_reserve, 2 * 1024 * 1024 * 1024);
        assert_eq!(config.min_disk_reserve, 10 * 1024 * 1024 * 1024);
        assert_eq!(config.alpine_mirror, "http://dl-cdn.alpinelinux.org/alpine/latest-stable");
        assert_eq!(config.alpine_trusted_keyring, PathBuf::from("/etc/thot/trusted-keys.gpg"));
        assert_eq!(config.alpine_trusted_fingerprint, "ABCDEF1234567890");
        assert_eq!(config.https_mirror_allowlist, "https://dl-cdn.alpinelinux.org/alpine/latest-stable");
        assert_eq!(config.max_iso_bytes, 2 * 1024 * 1024 * 1024);
        assert_eq!(config.max_input_bytes, 10 * 1024 * 1024 * 1024);
        assert_eq!(config.max_staged_bytes, 5 * 1024 * 1024 * 1024);
        assert_eq!(config.host_cpu_reserves, 0);
        assert_eq!(config.host_ram_reserves, 0 * 1024 * 1024 * 1024);
        assert_eq!(config.host_disk_reserves, 0 * 1024 * 1024 * 1024);
        assert_eq!(config.max_vcpu_ceiling, 4);
        assert_eq!(config.max_memory_ceiling, 8 * 1024 * 1024 * 1024);
        assert_eq!(config.max_disk_ceiling, 100 * 1024 * 1024 * 1024);
        assert_eq!(config.max_process_ceiling, 256);
        assert_eq!(config.bind_host, "127.0.0.1");
        assert_eq!(config.database_url, "sqlite:/var/lib/thot/thot.sqlite3");
        assert_eq!(config.artifact_root, PathBuf::from("/var/lib/thot/artifacts"));
    }
}