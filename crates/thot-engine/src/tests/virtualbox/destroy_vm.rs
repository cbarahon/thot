use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

fn destroy_vm(manifest: &str) -> Result<(), String> {
    let manifest_path = Path::new(manifest);
    if !manifest_path.exists() {
        return Err("Manifest file does not exist".to_string());
    }

    let manifest_content = fs::read_to_string(manifest_path).map_err(|e| format!("Failed to read manifest: {}", e))?;
    let vm_uuid = manifest_content.lines().find(|line| line.starts_with("VM_UUID=")).map(|line| line.split("=").nth(1).unwrap_or_default().trim()).unwrap_or_default();
    let storage_path = manifest_content.lines().find(|line| line.starts_with("STORAGE_PATH=")).map(|line| line.split("=").nth(1).unwrap_or_default().trim()).unwrap_or_default();

    if vm_uuid.is_empty() || storage_path.is_empty() {
        return Err("VM UUID or storage path not found in manifest".to_string());
    }

    // Verify VM is unregistered
    let vm_list_output = Command::new("VBoxManage")
        .arg("list")
        .arg("vms")
        .output()
        .map_err(|e| format!("Failed to list VMs: {}", e))?;
    if vm_list_output.status.success() && vm_list_output.stdout.contains(vm_uuid) {
        return Err("VM is still registered".to_string());
    }

    // Verify storage is removed
    if Path::new(storage_path).exists() {
        return Err("Storage path still exists".to_string());
    }

    Ok(())
}

fn main() {
    let manifest = "/path/to/cleanup/manifest.txt"; // Replace with actual manifest path
    match destroy_vm(manifest) {
        Ok(_) => println!("Cleanup successful"),
        Err(e) => eprintln!("Cleanup failed: {}", e),
    }
}
