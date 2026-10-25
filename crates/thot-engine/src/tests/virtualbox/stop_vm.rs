use std::process::{Command, Stdio};
use std::time::Duration;
use std::thread;

use crate::platform::helper::SharedPlatformHelper;
use crate::virtualbox::LocalVirtualBoxRunner;

pub fn stop_vm(vm_name: &str, timeout: Duration, helper: &SharedPlatformHelper, runner: &LocalVirtualBoxRunner) -> (String, String, String, i32) {
    let mut status = String::new();
    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut exit_code = 0;

    // Request graceful ACPI shutdown
    if let Err(e) = runner.shutdown_vm(vm_name, helper) {
        status.push_str(&format!("Error shutting down VM: {}", e));
        stderr.push_str(&format!("Error shutting down VM: {}", e));
        exit_code = 1;
    } else {
        // Wait within the invocation timeout
        thread::sleep(timeout);

        // Check if VM is still running
        if runner.is_vm_running(vm_name, helper).unwrap_or(false) {
            // Use documented poweroff fallback
            if let Err(e) = runner.poweroff_vm(vm_name, helper) {
                status.push_str(&format!("Error powering off VM: {}", e));
                stderr.push_str(&format!("Error powering off VM: {}", e));
                exit_code = 1;
            } else {
                status.push_str("Poweroff fallback succeeded");
                stdout.push_str("Poweroff fallback succeeded");
            }
        } else {
            status.push_str("Graceful ACPI shutdown succeeded");
            stdout.push_str("Graceful ACPI shutdown succeeded");
        }
    }

    (status, stdout, stderr, exit_code)
}
