use std::path::{Path, PathBuf};
use std::fs;
use std::time::Duration;
use std::io;
use std::error::Error;
use tokio::process::Command;
use std::env;

#[derive(Debug, Clone)]
pub struct HostCapabilities {
    pub max_cpu: u32,
    pub max_ram: u64,
    pub max_disk: u64,
    pub min_cpu_reserve: u32,
    pub min_ram_reserve: u64,
    pub min_disk_reserve: u64,
    pub libvirt_uri: String,
    pub firewall_confirmation: bool,
}

#[derive(Debug)]
pub struct PlatformError {
    message: String,
}

impl PlatformError {
    pub fn new(message: &str) -> Self {
        PlatformError {
            message: message.to_string(),
        }
    }
}

pub async fn preflight(settings: &Settings, platform_config: &PlatformConfig) -> Result<HostCapabilities, PlatformError> {
    if !cfg!(target_os = "linux") {
        return Err(PlatformError::new("The host must be Linux"));
    }

    if !Path::new("/dev/kvm").exists() {
        return Err(PlatformError::new("/dev/kvm is not available"));
    }

    if !Command::new("qemu-system-x86_64").arg("--version").output().await.is_ok() {
        return Err(PlatformError::new("qemu-system-x86_64 is not available"));
    }

    if !Command::new("qemu-img").arg("--version").output().await.is_ok() {
        return Err(PlatformError::new("qemu-img is not available"));
    }

    if !Command::new("virt-install").arg("--version").output().await.is_ok() {
        return Err(PlatformError::new("virt-install is not available"));
    }

    let config = KvmAlpineConfig::from_env().map_err(|e| PlatformError::new(&e))?;
    let online_cpu = Command::new("lscpu").arg("--cpu").output().await.map_err(|e| PlatformError::new(&e.stderr))? 
        .stdout.lines().count() as u32;
    let memory = Command::new("free").arg("-m").output().await.map_err(|e| PlatformError::new(&e.stderr))? 
        .stdout.lines().nth(1).map(|line| line.split_whitespace().nth(2).unwrap_or("0").parse().unwrap_or(0)).unwrap_or(0) * 1024;

    if online_cpu < config.min_cpu_reserve {
        return Err(PlatformError::new("Insufficient online CPU headroom"));
    }

    if memory < config.min_ram_reserve {
        return Err(PlatformError::new("Insufficient RAM headroom"));
    }

    let storage = fs::read_dir(config.storage_root).map_err(|e| PlatformError::new(&e.to_string()))?;
    let free_storage = storage.filter_map(|entry| entry.map_err(|e| PlatformError::new(&e.to_string())).ok())
        .filter_map(|entry| entry.metadata().map_err(|e| PlatformError::new(&e.to_string())).ok())
        .map(|metadata| metadata.len() / 1024 / 1024)
        .sum::<u64>();

    if free_storage < config.min_disk_reserve {
        return Err(PlatformError::new("Insufficient free disk space"));
    }

    if !config.firewall_confirmation {
        return Err(PlatformError::new("Firewall confirmation is required"));
    }

    Ok(HostCapabilities {
        max_cpu: config.max_cpu,
        max_ram: config.max_ram,
        max_disk: config.max_disk,
        min_cpu_reserve: config.min_cpu_reserve,
        min_ram_reserve: config.min_ram_reserve,
        min_disk_reserve: config.min_disk_reserve,
        libvirt_uri: config.libvirt_uri,
        firewall_confirmation: config.firewall_confirmation,
    })
}