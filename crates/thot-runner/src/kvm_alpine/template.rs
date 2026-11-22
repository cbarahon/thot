use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use sha2::{Digest, Sha256};
use tokio::fs as async_fs;
use tokio::process::Command as AsyncCommand;

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

        let config = self.load_config(template_name)?;
        let iso_url = config.template_image.iso_url;
        let digest = self.calculate_iso_digest(&iso_url).unwrap();
        let answer_hash = self.calculate_answer_hash(&config).unwrap();
        let package_inventory = config.template_packages.clone();
        let agent_version = config.agent.version.clone();
        let binary_hash = self.calculate_agent_binary_hash(&config).unwrap();
        let virtual_hardware = config.template_vm.vcpu.to_string() + "-" + &config.template_vm.memory_mib.to_string() + "M-" + &config.template_vm.disk_gib.to_string() + "G";
        let disk_format = "qcow2".to_string();
        let libvirt_version = Command::new("virsh").arg("--version").output().unwrap().stdout.split_whitespace().nth(1).unwrap().to_string();
        let domain_uuid = Command::new("virsh").arg("domuuid").arg(template_name).output().unwrap().stdout.trim().to_string();

        let manifest = TemplateManifest {
            release: config.template_image.release.clone(),
            architecture: config.template_image.architecture.clone(),
            iso_url,
            digest,
            answer_hash,
            package_inventory,
            agent_version,
            binary_hash,
            virtual_hardware,
            disk_format,
            libvirt_version,
            domain_uuid,
        };

        fs::write(format!("{}/{}.json", self.registry_path, template_name), serde_json::to_string_pretty(&manifest).unwrap()).unwrap();

        Ok(())
    }

    fn is_manifest_valid(&self, manifest: &TemplateManifest) -> bool {
        // TODO: Add validation logic
        true
    }

    fn load_config(&self, template_name: &str) -> Result<Configuration, String> {
        let config_path = format!("{}/{}.yaml", self.registry_path, template_name);
        if Path::new(&config_path).exists() {
            let config_str = fs::read_to_string(&config_path).unwrap();
            serde_yaml::from_str(&config_str).map_err(|e| e.to_string())
        } else {
            Err(format!("Config for template {} not found", template_name))
        }
    }

    fn calculate_iso_digest(&self, iso_url: &str) -> Result<String, String> {
        let mut output = Command::new("sha256sum")
            .arg(iso_url)
            .output()
            .map_err(|e| e.to_string())?;
        let digest = output.stdout.split_whitespace().next().unwrap().to_string();
        Ok(digest)
    }

    fn calculate_answer_hash(&self, config: &Configuration) -> Result<String, String> {
        let mut file = fs::File::create("/tmp/answer-file.cfg").unwrap();
        file.write_all(b"")?;
        fs::write("/tmp/answer-file.cfg", format!(
            "disk --device virtio --size {} --format qcow2 --name {}",
            config.template_vm.disk_gib, template_name
        )).unwrap();
        let mut output = Command::new("sha256sum")
            .arg("/tmp/answer-file.cfg")
            .output()
            .map_err(|e| e.to_string())?;
        let answer_hash = output.stdout.split_whitespace().next().unwrap().to_string();
        fs::remove_file("/tmp/answer-file.cfg").unwrap();
        Ok(answer_hash)
    }

    fn calculate_agent_binary_hash(&self, config: &Configuration) -> Result<String, String> {
        let agent_binary = self.download_agent_binary(&config).unwrap();
        let mut file = fs::File::create(&agent_binary).unwrap();
        file.write_all(&config.agent.install_from.binary_data)?;
        let mut output = Command::new("sha256sum")
            .arg(&agent_binary)
            .output()
            .map_err(|e| e.to_string())?;
        let binary_hash = output.stdout.split_whitespace().next().unwrap().to_string();
        fs::remove_file(&agent_binary).unwrap();
        Ok(binary_hash)
    }

    fn download_agent_binary(&self, config: &Configuration) -> Result<String, String> {
        let agent_binary = format!("{}/agent-binary", self.registry_path);
        if !Path::new(&agent_binary).exists() {
            let mut file = fs::File::create(&agent_binary).unwrap();
            file.write_all(&config.agent.install_from.binary_data)?;
        }
        Ok(agent_binary)
    }
}

fn main() {
    let store = TemplateStore::new("/path/to/template/registry".to_string());
    store.create_template("my-template", "12345678-1234-1234-1234-123456781234").unwrap();
    store.verify_template("my-template").unwrap();
}

#[derive(Debug, Deserialize)]
struct Configuration {
    template_image: TemplateImage,
    template_vm: TemplateVM,
    agent: Agent,
}

#[derive(Debug, Deserialize)]
struct TemplateImage {
    iso_url: String,
    release: String,
    architecture: String,
}

#[derive(Debug, Deserialize)]
struct TemplateVM {
    vcpu: u32,
    memory_mib: u64,
    disk_gib: u64,
}

#[derive(Debug, Deserialize)]
struct Agent {
    version: String,
    install_from: InstallFrom,
}

#[derive(Debug, Deserialize)]
struct InstallFrom {
    binary_data: Vec<u8>,
}