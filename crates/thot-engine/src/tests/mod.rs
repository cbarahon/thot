use std::error::Error;

pub mod external {
    pub mod adapter {
        pub fn dispatch(command: &str) -> Result<(), Box<dyn Error>> {
            match command {
                "VirtualBox/import_ova" => import_ova(),
                "VirtualBox/start_vm" => start_vm(),
                "VirtualBox/stop_vm" => stop_vm(),
                "VirtualBox/destroy_vm" => destroy_vm(),
                _ => Err("Unknown command".into()),
            }
        }

        fn import_ova() -> Result<(), Box<dyn Error>> {
            // Implementation for importing OVA
            Ok(())
        }

        fn start_vm() -> Result<(), Box<dyn Error>> {
            // Implementation for starting VM
            Ok(())
        }

        fn stop_vm() -> Result<(), Box<dyn Error>> {
            // Implementation for stopping VM
            Ok(())
        }

        fn destroy_vm() -> Result<(), Box<dyn Error>> {
            // Implementation for destroying VM
            Ok(())
        }
    }
}

pub mod ssh {
    pub fn get_host_info() -> Result<(), Box<dyn Error>> {
        // Implementation for getting host info
        Ok(())
    }
}

pub fn dispatch(command: &str) -> Result<(), Box<dyn Error>> {
    match command {
        "ssh/get_host_info" => ssh::get_host_info(),
        _ => Err("Unknown command".into()),
    }
}
