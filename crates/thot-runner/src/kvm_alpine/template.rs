use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;
use sha2::{Digest, Sha256};

#[derive(Debug)]
struct TemplateManifest {
    release: String,
    architecture: String,
    iso_url: String,
    digest: String,
    answer_hash: String,
    package_inventory: Vec<String>,
    agent_version: String,
    binary_hash: String,
    virtual_hardware: String,
    disk_format: String,
    libvirt_version: String,
    domain_uuid: String,
}

struct TemplateStore {
    registry_path: String,
    ownership_metadata: HashMap<String, String>,
}

impl TemplateStore {
    fn new(registry_path: String) -> Self {
        TemplateStore {
            registry_path,
            ownership_metadata: HashMap::new(),
        }
    }

    fn lookup_template(&self, template_name: &str) -> Option<TemplateManifest> {
        let manifest_path = format!("{}/{}.json", self.registry_path, template_name);
        if Path::new(&manifest_path).exists() {
            let manifest_str = fs::read_to_string(&manifest_path).unwrap();
            serde_json::from_str(&manifest_str).ok()
        } else {
            None
        }
    }

    fn create_template(&self, template_name: &str, ownership_uuid: &str) -> Result<(), String> {
        if self.ownership_metadata.contains_key(template_name) {
            return Err(format!("Template {} already owned", template_name));
        }
        self.ownership_metadata.insert(template_name.to_string(), ownership_uuid.to_string());
        self.rebuild_template(template_name)
    }

    fn verify_template(&self, template_name: &str) -> Result<(), String> {
        let manifest = self.lookup_template(template_name).unwrap();
        if !self.is_manifest_valid(&manifest) {
            return Err("Template manifest is invalid".to_string());
        }
        Ok(())
    }

    fn rebuild_template(&self, template_name: &str) -> Result<(), String> {
        let template_path = format!("{}/{}", self.registry_path, template_name);
        fs::create_dir_all(&template_path).unwrap();

        let iso_url = "https://dl-cdn.alpinelinux.org/alpine/v3.14/releases/x86_64/alpine-virt-3.14.0-x86_64.iso";
        let digest = "sha256:1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        let answer_hash = "sha256:1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        let package_inventory = vec!["bash".to_string(), "musl".to_string()];
        let agent_version = "1.0.0";
        let binary_hash = "sha256:1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        let virtual_hardware = "x86_64".to_string();
        let disk_format = "qcow2".to_string();
        let libvirt_version = "6.6.0".to_string();
        let domain_uuid = "12345678-1234-1234-1234-123456781234".to_string();

        let manifest = TemplateManifest {
            release: "3.14.0".to_string(),
            architecture: "x86_64".to_string(),
            iso_url: iso_url.to_string(),
            digest: digest.to_string(),
            answer_hash: answer_hash.to_string(),
            package_inventory,
            agent_version: agent_version.to_string(),
            binary_hash: binary_hash.to_string(),
            virtual_hardware: virtual_hardware.to_string(),
            disk_format: disk_format.to_string(),
            libvirt_version: libvirt_version.to_string(),
            domain_uuid: domain_uuid.to_string(),
        };

        fs::write(format!("{}/{}.json", self.registry_path, template_name), serde_json::to_string_pretty(&manifest).unwrap()).unwrap();

        Ok(())
    }

    fn is_manifest_valid(&self, manifest: &TemplateManifest) -> bool {
        // TODO: Add validation logic
        true
    }
}

fn main() {
    let store = TemplateStore::new("/path/to/template/registry".to_string());
    store.create_template("my-template", "12345678-1234-1234-1234-123456781234").unwrap();
    store.verify_template("my-template").unwrap();
}