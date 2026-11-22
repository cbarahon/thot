mod command;
mod preflight;
mod image;
mod template;
mod baseline;
mod agent_channel;
mod student_environment;

pub use self::command::*;
pub use self::preflight::*;
pub use self::image::*;
pub use self::template::*;
pub use self::baseline::*;
pub use self::agent_channel::*;
pub use self::student_environment::*;

pub struct KvmAlpineSettings {
    pub libvirt_uri: String,
    pub virsh_path: String,
    pub virt_install_path: String,
    pub qemu_img_path: String,
    pub virt_customize_path: String,
    pub storage_root: String,
    pub iso_cache: String,
    pub registry_root: String,
    pub agent_binary: String,
    pub alpine_signing_keyring: String,
    pub alpine_signing_fingerprint: String,
    pub allowed_mirror_hosts: Vec<String>,
    pub max_iso_bytes: u64,
    pub resource_ceilings: ResourceCeilings,
}

pub struct ResourceCeilings {
    pub max_vcpus: u32,
    pub guest_memory_bytes: u64,
    pub guest_disk_bytes: u64,
    pub project_input_bytes: u64,
    pub total_staged_bytes: u64,
    pub host_cpu_reserves: u32,
    pub host_ram_reserves: u64,
    pub host_disk_reserves: u64,
    pub max_guest_process_count: u32,
}

impl KvmAlpineSettings {
    pub fn validate(&self) -> Result<(), ProviderError> {
        if self.libvirt_uri.is_empty() || self.virsh_path.is_empty() || self.virt_install_path.is_empty() || self.qemu_img_path.is_empty() || self.virt_customize_path.is_empty() || self.storage_root.is_empty() || self.iso_cache.is_empty() || self.agent_binary.is_empty() || self.alpine_signing_keyring.is_empty() || self.alpine_signing_fingerprint.is_empty() {
            return Err(ProviderError::InvalidPath);
        }
        if self.max_iso_bytes == 0 || self.resource_ceilings.max_vcpus == 0 || self.resource_ceilings.guest_memory_bytes == 0 || self.resource_ceilings.guest_disk_bytes == 0 || self.resource_ceilings.project_input_bytes == 0 || self.resource_ceilings.total_staged_bytes == 0 || self.resource_ceilings.host_cpu_reserves == 0 || self.resource_ceilings.host_ram_reserves == 0 || self.resource_ceilings.host_disk_reserves == 0 || self.resource_ceilings.max_guest_process_count == 0 {
            return Err(ProviderError::ZeroLimit);
        }
        if !self.alpine_signing_fingerprint.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
            return Err(ProviderError::InvalidFingerprint);
        }
        Ok(())
    }
}

pub struct OpaqueHandle {
    // Define the fields of your opaque handle here
}

pub struct KvmAlpineProvider {
    // Define the fields and methods of your provider here
}