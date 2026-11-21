// crates/thot-runner/src/kvm_alpine/baseline.rs

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use serde::Deserialize;
use tokio::fs as async_fs;

use crate::kvm::Kvm;
use crate::manifest::{TemplateManifest, PackageInventory};
use crate::provenance::{Provenance, ProvenanceChecker};
use crate::vm::{Vm, VmConfig, VmState};

#[derive(Debug, Deserialize)]
struct PreparationConfig {
    packages: Vec<String>,
    scripts: Vec<String>,
    deadline: u64,
    output_cap: u64,
}

#[derive(Debug)]
struct ProjectBaseline {
    vm_name: String,
    baseline_identity: String,
    template_hash: String,
    config_hash: String,
    package_hash: String,
    provider_version: String,
}

impl ProjectBaseline {
    fn new(project_id: &str, template_manifest: &TemplateManifest, config: &PreparationConfig, package_inventory: &PackageInventory, provider_version: &str) -> Result<Self> {
        let vm_name = format!("project-{}-baseline", project_id);
        let baseline_identity = format!("{}-{}-{}-{}-{}", project_id, template_manifest.hash(), config_hash(&config), package_hash(&package_inventory), provider_version);
        let template_hash = template_manifest.hash();
        let config_hash = config_hash(&config);
        let package_hash = package_hash(&package_inventory);
        Ok(ProjectBaseline { vm_name, baseline_identity, template_hash, config_hash, package_hash, provider_version })
    }

    async fn prepare(&self, kvm: &Kvm) -> Result<()> {
        let vm = kvm.get_vm(&self.vm_name).await?;
        if vm.state() != VmState::PoweredOff {
            return Err(anyhow!("VM is not powered off"));
        }

        if kvm.vm_exists(&self.baseline_identity) {
            return Err(anyhow!("VM with the same identity already exists"));
        }

        let linked_clone = kvm.create_linked_clone(&self.template_hash, &self.baseline_identity).await?;
        let mut vm = kvm.get_vm(&self.baseline_identity).await?;

        for package in &self.config.packages {
            vm.install_package(package).await?;
        }

        for script in &self.config.scripts {
            vm.run_script(script, self.config.deadline, self.config.output_cap).await?;
        }

        vm.sync().await?;
        vm.shutdown().await?;
        if vm.state() != VmState::PoweredOff {
            return Err(anyhow!("VM did not power off cleanly"));
        }

        kvm.create_snapshot(&self.baseline_identity, "disk").await?;
        Ok(())
    }
}

fn config_hash(config: &PreparationConfig) -> String {
    use serde_json::json;
    use blake2::{Blake2s256, Digest};

    let config_json = json!(config);
    let mut hasher = Blake2s256::new();
    hasher.update(config_json.to_string());
    format!("{:x}", hasher.finalize())
}

fn package_hash(package_inventory: &PackageInventory) -> String {
    use blake2::{Blake2s256, Digest};

    let mut hasher = Blake2s256::new();
    hasher.update(package_inventory.hash());
    format!("{:x}", hasher.finalize())
}