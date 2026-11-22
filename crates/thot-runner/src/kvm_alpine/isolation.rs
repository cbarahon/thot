use crate::kvm_alpine::isolation::{GuestPhase, AppliedPolicy, ProviderError};
use crate::settings::KvmAlpineSettings;
use crate::limits::Limits;
use crate::libvirt::domain::Domain;
use crate::cgroup::CgroupManager;
use crate::network::NetworkManager;

#[derive(Debug, PartialEq)]
enum GuestPhase {
    TemplateInstall,
    ProjectPreparation,
    StudentTest,
}

pub(crate) async fn apply_domain_policy(settings: &KvmAlpineSettings, domain_name: &str, phase: GuestPhase, limits: &Limits) -> Result<AppliedPolicy, ProviderError> {
    let mut policy = AppliedPolicy::default();
    
    // Configure cgroup limits
    let cgroup_manager = CgroupManager::new();
    cgroup_manager.set_cpu_limit(domain_name, limits.cpu_limit);
    cgroup_manager.set_memory_limit(domain_name, limits.memory_limit);
    cgroup_manager.set_pid_limit(domain_name, limits.pid_limit);
    
    // Configure libvirt domain
    let mut domain = Domain::new(domain_name);
    domain.set_vcpus(limits.vcpus);
    domain.set_memory(limits.memory_bytes);
    domain.set_disk_quota(limits.disk_quota);
    
    // Apply network policy
    let network_manager = NetworkManager::new();
    match phase {
        GuestPhase::StudentTest => network_manager.detach_network(domain_name),
        _ => network_manager.attach_network(domain_name, settings.network_filter),
    }
    
    // Verify domain XML and active cgroup values
    if !domain.verify_xml() || !cgroup_manager.verify_cgroup_values(domain_name) {
        return Err(ProviderError::VerificationFailed);
    }
    
    // Apply policy
    policy.cpu_limit = limits.cpu_limit;
    policy.memory_limit = limits.memory_limit;
    policy.pid_limit = limits.pid_limit;
    policy.vcpus = limits.vcpus;
    policy.memory_bytes = limits.memory_bytes;
    policy.disk_quota = limits.disk_quota;
    
    Ok(policy)
}

fn verify_domain_policy(domain: &Domain, policy: &AppliedPolicy) -> Result<(), ProviderError> {
    if domain.cpu_limit != policy.cpu_limit
        || domain.memory_limit != policy.memory_limit
        || domain.pid_limit != policy.pid_limit
        || domain.vcpus != policy.vcpus
        || domain.memory_bytes != policy.memory_bytes
        || domain.disk_quota != policy.disk_quota {
        return Err(ProviderError::PolicyMismatch);
    }
    
    Ok(())
}