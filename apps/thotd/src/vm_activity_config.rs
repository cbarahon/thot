//! Purpose: Loads and validates daemon and VirtualBox settings from environment variables.
use std::{env, path::PathBuf, time::Duration};

#[derive(Debug, Clone)]
pub struct VmActivityConfig {
    pub bind_host: String,
    pub bind_port: u16,
    pub database_url: String,
    pub artifact_root: PathBuf,
    pub vbox_manage_path: PathBuf,
    pub expected_vbox_version: String,
    pub vbox_user_home: PathBuf,
    pub storage_root: PathBuf,
    pub ova_size_limit: u64,
    pub free_space_reserve: u64,
    pub output_limit: usize,
    pub import_timeout: Duration,
    pub startup_timeout: Duration,
    pub ssh_timeout: Duration,
    pub shutdown_grace: Duration,
    /// Path to a root/service-account protected file containing only the guest password.
    pub ssh_secret_source: PathBuf,
    pub firewall_confirmation: bool,
}

impl VmActivityConfig {
    pub fn from_env() -> Result<Self, String> {
        let base =
            PathBuf::from(env::var("THOTD_DATA_ROOT").unwrap_or_else(|_| "/var/lib/thot".into()));
        let env_path =
            |key: &str, fallback: PathBuf| env::var_os(key).map(PathBuf::from).unwrap_or(fallback);
        let bind_host = env::var("THOTD_BIND_HOST")
            .or_else(|_| env::var("THOTD_HOST"))
            .unwrap_or_else(|_| "127.0.0.1".into());
        let bind_port = parse_env("THOTD_BIND_PORT", parse_env("THOTD_PORT", 8081u16)?)?;
        let database_url = env::var("THOTD_DATABASE_URL")
            .or_else(|_| env::var("THOTD_DATABASE"))
            .unwrap_or_else(|_| format!("sqlite:{}", base.join("thot.sqlite3").display()));
        let config = Self {
            bind_host,
            bind_port,
            database_url,
            artifact_root: env_path("THOTD_ARTIFACT_ROOT", base.join("artifacts")),
            vbox_manage_path: env_path(
                "THOTD_VBOX_MANAGE_PATH",
                PathBuf::from("/usr/bin/VBoxManage"),
            ),
            expected_vbox_version: env::var("THOTD_EXPECTED_VBOX_VERSION").unwrap_or_default(),
            vbox_user_home: env_path("THOTD_VBOX_USER_HOME", base.join("vbox-profile")),
            storage_root: env_path("THOTD_STORAGE_ROOT", base.join("vms")),
            ova_size_limit: parse_env("THOTD_OVA_SIZE_LIMIT", 10 * 1024 * 1024 * 1024u64)?,
            free_space_reserve: parse_env("THOTD_FREE_SPACE_RESERVE", 10 * 1024 * 1024 * 1024u64)?,
            output_limit: parse_env("THOTD_OUTPUT_LIMIT", 1024 * 1024usize)?,
            import_timeout: Duration::from_secs(parse_env("THOTD_IMPORT_TIMEOUT", 1800u64)?),
            startup_timeout: Duration::from_secs(parse_env("THOTD_STARTUP_TIMEOUT", 180u64)?),
            ssh_timeout: Duration::from_secs(parse_env("THOTD_SSH_TIMEOUT", 15u64)?),
            shutdown_grace: Duration::from_secs(parse_env("THOTD_SHUTDOWN_GRACE", 30u64)?),
            ssh_secret_source: env_path(
                "THOTD_SSH_SECRET_FILE",
                PathBuf::from("/etc/thot/secrets/guest-ssh-password"),
            ),
            firewall_confirmation: parse_env("THOTD_FIREWALL_CONFIRMATION", false)?,
        };
        let bind_ip: std::net::IpAddr = config
            .bind_host
            .parse()
            .map_err(|_| "THOTD_BIND_HOST must be a loopback IP address".to_string())?;
        if !bind_ip.is_loopback() {
            return Err("THOTD_BIND_HOST must be loopback-only for the POC".into());
        }
        if config.bind_port == 8080 || config.bind_port == 2222 {
            return Err(
                "THOTD_BIND_PORT must not conflict with the VM HTTP/SSH forwarding ports".into(),
            );
        }
        if config.ova_size_limit == 0
            || config.output_limit == 0
            || config.startup_timeout.is_zero()
        {
            return Err("size, output, and timeout limits must be greater than zero".into());
        }
        Ok(config)
    }
}

fn parse_env<T: std::str::FromStr>(key: &str, default: T) -> Result<T, String> {
    match env::var(key) {
        Ok(value) => value
            .parse()
            .map_err(|_| format!("invalid value for {key}")),
        Err(_) => Ok(default),
    }
}
