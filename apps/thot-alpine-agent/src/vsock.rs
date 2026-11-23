use std::{fs::File, io, os::fd::FromRawFd};

#[cfg(target_os = "linux")]
pub fn connect_to_vsock(host_cid: u32, port: u32) -> io::Result<File> {
    if port == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "port must be nonzero",
        ));
    }
    let fd = unsafe { libc::socket(libc::AF_VSOCK, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let address = libc::sockaddr_vm {
        svm_family: libc::AF_VSOCK as libc::sa_family_t,
        svm_reserved1: 0,
        svm_port: port,
        svm_cid: host_cid,
        svm_zero: [0; 4],
    };
    let result = unsafe {
        libc::connect(
            fd,
            &address as *const _ as *const libc::sockaddr,
            std::mem::size_of::<libc::sockaddr_vm>() as libc::socklen_t,
        )
    };
    if result < 0 {
        let error = io::Error::last_os_error();
        unsafe {
            libc::close(fd);
        }
        return Err(error);
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

#[cfg(not(target_os = "linux"))]
pub fn connect_to_vsock(_: u32, _: u32) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "AF_VSOCK is only supported on Linux",
    ))
}
