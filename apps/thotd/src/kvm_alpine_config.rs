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
    pub firewall_confirmation: bool,
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
            libvirt_uri: env::var("THOTD_LIBVIRT_URI").unwrap_or_else(|_| "qemu:///system".into()),
            max_cpu: parse_env("THOTD_MAX_CPU", 4u32)?,
            max_ram: parse_positive("THOTD_MAX_RAM", 4 * 1024 * 1024 * 1024u64)?,
            max_disk: parse_positive("THOTD_MAX_DISK", 50 * 1024 * 1024 * 1024u64)?,
            min_cpu_reserve: parse_env("THOTD_MIN_CPU_RESERVE", 1u32)?,
            min_ram_reserve: parse_positive("THOTD_MIN_RAM_RESERVE", 2 * 1024 * 1024 * 1024u64)?,
            min_disk_reserve: parse_positive("THOTD_MIN_DISK_RESERVE", 10 * 1024 * 1024 * 1024u64)?,
            alpine_mirror: env::var("THOTD_ALPINE_MIRROR").unwrap_or_else(|_| "http://dl-cdn.alpinelinux.org/alpine/latest-stable".into()),
            firewall_confirmation: parse_bool("THOTD_FIREWALL_CONFIRMATION", true),
        };

        validate_paths(&config)?;

        Ok(config)
    }
}

fn validate_paths(config: &KvmAlpineConfig) -> Result<(), String> {
    let check_path = |path: &Path| -> Result<(), String> {
        if !path.exists() || !path.is_file() {
            Err(format!("Path {} does not exist or is not a file", path.display()))
        } else {
            Ok(())
        }
    };

    check_path(&config.virsh_path)?;
    check_path(&config.virt_install_path)?;
    check_path(&config.qemu_img_path)?;
    check_path(&config.thot_alpine_agent_path)?;

    Ok(())
}

fn parse_env<T: FromStr, const DEFAULT: T>(key: &str, fallback: T) -> Result<T, String> {
    env::var(key)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(fallback)
}