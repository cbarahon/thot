use std::time::Instant;
use std::process::{Command, Stdio};
use std::io::{Read, Write};
use std::error::Error;

use crate::platform::helpers::SharedPlatformHelper;
use crate::platform::runners::LocalVirtualBoxRunner;

pub struct StartVmTest {
    vm_name: String,
    timeout: u64,
}

impl StartVmTest {
    pub fn new(vm_name: Option<String>, timeout: u64) -> Self {
        StartVmTest {
            vm_name: vm_name.unwrap_or_else(|| {
                eprintln!("VM name is unavailable");
                std::process::exit(1);
            }),
            timeout,
        }
    }

    pub fn run(&self) -> Result<(String, String, String, i32, f64, String), Box<dyn Error>> {
        let start_time = Instant::now();
        let runner = LocalVirtualBoxRunner::new();
        let helper = SharedPlatformHelper::new();

        let mut process = Command::new("VBoxManage")
            .arg("startvm")
            .arg(&self.vm_name)
            .arg("--type")
            .arg("headless")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let mut stdout = String::new();
        let mut stderr = String::new();
        process.stdout.take().unwrap().read_to_string(&mut stdout)?;
        process.stderr.take().unwrap().read_to_string(&mut stderr)?;

        let exit_code = process.wait()?.code().unwrap_or(1);
        let duration = start_time.elapsed().as_secs_f64();

        if exit_code != 0 {
            return Err(format!("VM start failed with exit code {}", exit_code).into());
        }

        Ok((stdout, stderr, stdout, exit_code, duration, helper.diagnostics()))
    }
}
