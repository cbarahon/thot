use std::io::{Read, Write};
use std::os::unix::io::FromRawFd;
use std::os::unix::net::UnixStream;
use std::result::Result;
use std::str::FromStr;
use std::net::SocketAddr;
use vsock::{Client, Connection, Server};

pub fn connect_to_vsock(host_cid: u32, port: u32) -> Result<Box<dyn Read + Write>, std::io::Error> {
    let client = Client::connect(host_cid, port)?;
    Ok(Box::new(client))
}