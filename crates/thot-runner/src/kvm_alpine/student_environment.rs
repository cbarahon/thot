use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Deserialize, Serialize};
use serde_json::Value;

mod agent_protocol;
mod kvm_alpine;

use agent_protocol::{AgentTask, AgentResult};
use kvm_alpine::{create_linked_clone, start_vm, stop_vm, delete_vm, VMConfig};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StudentEnvironment {
    student_id: String,
    vm_config: VMConfig,
    agent_channel: Option<AgentChannel>,
    workspace_path: PathBuf,
    submission_files: Vec<(String, PathBuf)>,
    instructor_files: Vec<(String, PathBuf)>,
}

impl StudentEnvironment {
    fn new(student_id: String, vm_config: VMConfig, workspace_path: PathBuf) -> Self {
        Self {
            student_id,
            vm_config,
            agent_channel: None,
            workspace_path,
            submission_files: Vec::new(),
            instructor_files: Vec::new(),
        }
    }

    fn create(&mut self) -> io::Result<()> {
        let vm_name = self.generate_vm_name();
        let unit_id = self.generate_unit_id();

        self.vm_config.name = vm_name;
        self.vm_config.unit_id = unit_id;

        create_linked_clone(&self.vm_config)?;

        let agent_channel = start_vm(&self.vm_config)?;
        self.agent_channel = Some(agent_channel);

        Ok(())
    }

    fn generate_vm_name(&self) -> String {
        format!("student-{}-{}", self.student_id, SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs())
    }

    fn generate_unit_id(&self) -> String {
        format!("student-{}-unit", self.student_id)
    }

    fn stage_files(&mut self, submission_files: Vec<(String, PathBuf)>, instructor_files: Vec<(String, PathBuf)>) -> io::Result<()> {
        for (file_name, file_path) in submission_files {
            if self.is_safe_path(&file_path) {
                let target_path = self.workspace_path.join(&file_name);
                fs::create_dir_all(target_path.parent().unwrap())?;
                fs::copy(&file_path, &target_path)?;
                fs::set_permissions(&target_path, fs::Permissions::from_mode(0o444))?;
                self.submission_files.push((file_name, target_path));
            } else {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "Unsafe file path"));
            }
        }

        for (file_name, file_path) in instructor_files {
            if self.is_safe_path(&file_path) {
                let target_path = self.workspace_path.join(&file_name);
                fs::create_dir_all(target_path.parent().unwrap())?;
                fs::copy(&file_path, &target_path)?;
                fs::set_permissions(&target_path, fs::Permissions::from_mode(0o444))?;
                self.instructor_files.push((file_name, target_path));
            } else {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "Unsafe file path"));
            }
        }

        Ok(())
    }

    fn is_safe_path(&self, path: &Path) -> bool {
        path.is_absolute() && !path.starts_with("..") && !path.starts_with("/") && !path.is_symlink() && path.file_size().unwrap() < 1024 * 1024 * 10  // 10MB
    }

    fn dispatch_test(&mut self, task: AgentTask) -> io::Result<AgentResult> {
        if let Some(ref mut agent_channel) = self.agent_channel {
            agent_channel.send_task(task)?;
            return agent_channel.receive_result();
        }
        Err(io::Error::new(io::ErrorKind::Other, "Agent channel not initialized"))
    }

    fn destroy(&mut self) -> io::Result<()> {
        if let Some(ref mut agent_channel) = self.agent_channel {
            agent_channel.stop_vm()?;
            agent_channel.delete_vm()?;
        }

        stop_vm(&self.vm_config)?;
        delete_vm(&self.vm_config)?;

        for (_, path) in self.submission_files.iter().chain(self.instructor_files.iter()) {
            fs::remove_file(path)?;
        }

        fs::remove_dir_all(&self.workspace_path)?;

        self.agent_channel = None;

        Ok(())
    }

    fn verify_cleanup(&self) -> bool {
        // Verify that the VM and overlay are no longer present
        !fs::metadata(&self.vm_config.name).is_ok() && !fs::metadata(&self.vm_config.overlay_path).is_ok() && !self.submission_files.iter().any(|(_, path)| fs::metadata(path).is_ok()) && !self.instructor_files.iter().any(|(_, path)| fs::metadata(path).is_ok())
    }
}