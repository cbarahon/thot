use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default)]
    pub auth_token: Option<String>,
    pub ssh: SshConfig,
    #[serde(default = "default_output_roots")]
    pub allowed_output_roots: Vec<PathBuf>,
    #[serde(default)]
    pub allowed_delivery_roots: Vec<PathBuf>,
    #[serde(default)]
    pub allowed_vm_storage_roots: Vec<PathBuf>,
    #[serde(default)]
    pub path_mappings: Vec<PathMapping>,
    #[serde(default = "default_output_limit")]
    pub output_limit: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SshConfig {
    pub host: String,
    pub user: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_key")]
    pub private_key: PathBuf,
    #[serde(default)]
    pub known_hosts: Option<PathBuf>,
    #[serde(default = "default_vboxmanage")]
    pub vboxmanage: String,
    pub vbox_user_home: PathBuf,
    pub expected_vbox_version: String,
    #[serde(default = "default_connect_timeout")]
    pub connect_timeout_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PathMapping {
    pub local: PathBuf,
    pub remote: PathBuf,
}

fn default_bind() -> String {
    "127.0.0.1:8090".into()
}
fn default_port() -> u16 {
    22
}
fn default_key() -> PathBuf {
    PathBuf::from("/etc/thot-virtualbox-agent/ssh/id_ed25519")
}
fn default_vboxmanage() -> String {
    "/usr/bin/VBoxManage".into()
}
fn default_connect_timeout() -> u64 {
    10
}
fn default_output_limit() -> usize {
    1024 * 1024
}
fn default_output_roots() -> Vec<PathBuf> {
    Vec::new()
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes =
            fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        let config: Self = serde_yaml::from_slice(&bytes)
            .map_err(|error| format!("invalid agent configuration: {error}"))?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), String> {
        let address: std::net::SocketAddr = self
            .bind
            .parse()
            .map_err(|_| "bind must be a socket address".to_string())?;
        if !address.ip().is_loopback() && self.auth_token.as_deref().unwrap_or_default().is_empty()
        {
            return Err(
                "auth_token is required when the agent listens on a non-loopback address".into(),
            );
        }
        if self.ssh.host.is_empty() || self.ssh.user.is_empty() || self.ssh.port == 0 {
            return Err("ssh.host, ssh.user, and a non-zero ssh.port are required".into());
        }
        if !self.ssh.vbox_user_home.is_absolute() || self.ssh.expected_vbox_version.is_empty() {
            return Err("ssh.vbox_user_home must be absolute and expected_vbox_version must pin a qualified host version".into());
        }
        if self.allowed_output_roots.is_empty()
            || self.allowed_delivery_roots.is_empty()
            || self.allowed_vm_storage_roots.is_empty()
        {
            return Err("allowed_output_roots, allowed_delivery_roots, and allowed_vm_storage_roots must each contain an approved path".into());
        }
        if self.output_limit == 0 || self.ssh.connect_timeout_seconds == 0 {
            return Err("output_limit and ssh connect timeout must be greater than zero".into());
        }
        Ok(())
    }

    pub fn socket_addr(&self) -> Result<std::net::SocketAddr, String> {
        self.bind
            .parse()
            .map_err(|_| "invalid bind address".to_string())
    }

    pub fn ensure_key_pair(&self) -> Result<String, String> {
        let key = expand_home(&self.ssh.private_key);
        if let Some(parent) = key.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let public = PathBuf::from(format!("{}.pub", key.display()));
        if !key.exists() {
            let status = std::process::Command::new("ssh-keygen")
                .args(["-q", "-t", "ed25519", "-N", "", "-f"])
                .arg(&key)
                .status()
                .map_err(|e| format!("could not generate SSH key pair: {e}"))?;
            if !status.success() {
                return Err("ssh-keygen failed to generate the VirtualBox agent key".into());
            }
        }
        if !public.is_file() {
            let output = std::process::Command::new("ssh-keygen")
                .args(["-y", "-f"])
                .arg(&key)
                .output()
                .map_err(|e| format!("cannot derive SSH public key: {e}"))?;
            if !output.status.success() {
                return Err(String::from_utf8_lossy(&output.stderr).into_owned());
            }
            fs::write(&public, output.stdout)
                .map_err(|e| format!("cannot write SSH public key: {e}"))?;
        }
        fs::read_to_string(public).map_err(|e| format!("cannot read SSH public key: {e}"))
    }
}

pub fn expand_home(path: &Path) -> PathBuf {
    if path == Path::new("~") {
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
    }
    if let Ok(tail) = path.strip_prefix("~") {
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join(tail);
    }
    path.to_path_buf()
}

pub fn path_is_within(path: &Path, roots: &[PathBuf]) -> Result<PathBuf, String> {
    let candidate = path
        .canonicalize()
        .map_err(|e| format!("path {} is unavailable: {e}", path.display()))?;
    for root in roots {
        let root = expand_home(root)
            .canonicalize()
            .map_err(|e| format!("approved root {} is unavailable: {e}", root.display()))?;
        if candidate.starts_with(&root) {
            return Ok(candidate);
        }
    }
    Err(format!(
        "path {} is outside configured agent roots",
        candidate.display()
    ))
}

pub fn path_is_within_after_create(path: &Path, roots: &[PathBuf]) -> Result<PathBuf, String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(path)
    };
    let mut current = PathBuf::new();
    for component in absolute.components() {
        current.push(component);
        if fs::symlink_metadata(&current).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return Err(format!(
                "test directory path must not traverse a symlink: {}",
                current.display()
            ));
        }
    }
    let mut parent = path;
    while !parent.exists() {
        parent = parent
            .parent()
            .ok_or("test directory has no existing parent")?;
    }
    let canonical_parent = path_is_within(parent, roots)?;
    let tail = path
        .strip_prefix(parent)
        .map_err(|_| "test directory parent mismatch")?;
    Ok(canonical_parent.join(tail))
}
