use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use serde_yaml;

#[derive(Debug, Serialize)]
struct VMInfo {
    vm_name: String,
    vm_uuid: Option<String>,
    vm_path: String,
}

fn import_ova(vm_name: Option<&str>, vbox_group: Option<&str>, vm_path: Option<&str>) -> (String, String, String, i32, VMInfo) {
    let ova_path = "/path/to/current/delivery.ova"; // Replace with actual OVA path
    let mut command = Command::new("VBoxManage")
        .arg("import")
        .arg(ova_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start VBoxManage import");

    let output = command.wait_with_output().expect("Failed to wait on VBoxManage import");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let exit_code = output.status.code().unwrap_or(1);

    let mut vm_info = VMInfo {
        vm_name: vm_name.unwrap_or("default_vm").to_string(),
        vm_uuid: None,
        vm_path: vm_path.unwrap_or("/default/path").to_string(),
    };

    if exit_code == 0 {
        // Assuming VM UUID is printed in the output
        if let Some(uuid) = stdout.lines().find(|line| line.contains("UUID:")) {
            vm_info.vm_uuid = Some(uuid.split_whitespace().nth(1).unwrap().to_string());
        }
    }

    let cleanup_manifest = serde_yaml::to_string(&vm_info).expect("Failed to serialize VM info for cleanup manifest");
    fs::write("/path/to/cleanup_manifest.yaml", &cleanup_manifest).expect("Failed to write cleanup manifest");

    (stdout.to_string(), stderr.to_string(), output.status.to_string(), exit_code, vm_info)
}
