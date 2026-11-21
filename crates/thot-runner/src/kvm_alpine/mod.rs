// crates/thot-runner/src/kvm_alpine/mod.rs

mod command;
mod preflight;
mod image;
mod template;
mod baseline;
mod agent_channel;
mod student_environment;
mod provider_orchestration;

pub use self::command::*;
pub use self::preflight::*;
pub use self::image::*;
pub use self::template::*;
pub use self::baseline::*;
pub use self::agent_channel::*;
pub use self::student_environment::*;
pub use self::provider_orchestration::*;

pub struct KvmAlpineSettings {
    host_paths: Vec<String>,
    libvirt_uri: String,
    resource_ceilings: ResourceCeilings,
    image_cache: ImageCache,
    agent_binary: String,
}

pub struct ResourceCeilings {
    cpu: u32,
    memory: u64,
    disk: u64,
}

pub struct ImageCache {
    path: String,
}

pub struct OpaqueHandle {
    // Define the fields of your opaque handle here
}