use std::fs::File;
use std::io;
use std::os::unix::io::FromRawFd;
use std::os::unix::io::AsRawFd;
use std::os::unix::io::OwnedFd;
use libc::{c_int, AF_VSOCK, SOCK_STREAM, SOCK_CLOEXEC, sockaddr_vm, sockaddr, sockaddr_vm_init, sockaddr_vm_getsockaddr, sockaddr_vm_set_family, sockaddr_vm_set_cid, sockaddr_vm_set_port};

#[cfg(target_os = "linux")]
pub fn connect_to_vsock(host_cid: u32, port: u32) -> io::Result<File> {
    if port == 0 || port > u16::MAX as u32 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Invalid port number"));
    }

    let sock = unsafe {
        libc::socket(AF_VSOCK, SOCK_STREAM | SOCK_CLOEXEC, 0)
    };

    if sock < 0 {
        return Err(io::Error::last_os_error());
    }

    let mut addr = sockaddr_vm::default();
    unsafe {
        sockaddr_vm_init(&mut addr);
        sockaddr_vm_set_family(&mut addr, AF_VSOCK);
        sockaddr_vm_set_cid(&mut addr, host_cid);
        sockaddr_vm_set_port(&mut addr, port as u16);
    }

    let addr_ptr = &addr as *const _ as *const sockaddr;
    let len = std::mem::size_of::<sockaddr_vm>() as u32;

    let res = unsafe {
        libc::connect(sock, addr_ptr, len)
    };

    if res < 0 {
        unsafe {
            libc::close(sock);
        }
        return Err(io::Error::last_os_error());
    }

    unsafe {
        Ok(File::from_raw_fd(sock))
    }
}

#[cfg(target_os = "linux")]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invalid_port() {
        assert!(connect_to_vsock(0, 0).is_err());
        assert!(connect_to_vsock(0, u16::MAX as u32 + 1).is_err());
    }

    #[test]
    fn test_invalid_cid() {
        assert!(connect_to_vsock(u32::MAX, 1234).is_err());
    }
}

#[cfg(not(target_os = "linux"))]
pub fn connect_to_vsock(_host_cid: u32, _port: u32) -> io::Result<File> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "AF_VSOCK is only supported on Linux"))
}
```