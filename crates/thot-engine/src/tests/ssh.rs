use ssh2::{Channel, Session};
use std::net::TcpStream;
use std::time::Duration;
use serde_yaml::to_string;

fn get_host_info(host: &str, port: u16, user: &str, pass: &str, timeout: Duration) -> Result<String, String> {
    let tcp = TcpStream::connect((host, port)).map_err(|e| format!("Failed to connect: {}", e))?;
    let mut session = Session::new().map_err(|e| format!("Failed to create session: {}", e))?;
    session.set_tcp_stream(tcp);
    session.set_timeout(timeout).map_err(|e| format!("Failed to set timeout: {}", e))?;
    session.userauth_password(user, pass).map_err(|e| format!("Failed to authenticate: {}", e))?;

    let mut channel = session.channel_session().map_err(|e| format!("Failed to create channel: {}", e))?;
    channel.exec("hostname").map_err(|e| format!("Failed to execute command: {}", e))?;
    let hostname = channel.read_to_string().map_err(|e| format!("Failed to read output: {}", e))?;

    channel.exec("uname -a").map_err(|e| format!("Failed to execute command: {}", e))?;
    let os_info = channel.read_to_string().map_err(|e| format!("Failed to read output: {}", e))?;

    channel.exec("free -h").map_err(|e| format!("Failed to execute command: {}", e))?;
    let ram_info = channel.read_to_string().map_err(|e| format!("Failed to read output: {}", e))?;

    channel.exec("df -h").map_err(|e| format!("Failed to execute command: {}", e))?;
    let fs_info = channel.read_to_string().map_err(|e| format!("Failed to read output: {}", e))?;

    channel.close().map_err(|e| format!("Failed to close channel: {}", e))?;
    session.disconnect(None).map_err(|e| format!("Failed to disconnect session: {}", e))?;

    let observation = serde_yaml::Mapping::new(vec![
        (serde_yaml::Value::String("hostname".to_string()), serde_yaml::Value::String(hostname.trim().to_string())),
        (serde_yaml::Value::String("os_info".to_string()), serde_yaml::Value::String(os_info.trim().to_string())),
        (serde_yaml::Value::String("ram_info".to_string()), serde_yaml::Value::String(ram_info.trim().to_string())),
        (serde_yaml::Value::String("fs_info".to_string()), serde_yaml::Value::String(fs_info.trim().to_string())),
    ]);

    to_string(&observation).map_err(|e| format!("Failed to serialize observation: {}", e))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 6 {
        eprintln!("Usage: {} <host> <port> <user> <pass> <timeout>", args[0]);
        std::process::exit(1);
    }

    let host = &args[1];
    let port: u16 = args[2].parse().expect("Invalid port number");
    let user = &args[3];
    let pass = &args[4];
    let timeout = Duration::from_secs(args[5].parse().expect("Invalid timeout"));

    match get_host_info(host, port, user, pass, timeout) {
        Ok(observation) => println!("{}", observation),
        Err(e) => eprintln!("Error: {}", e),
    }
}
