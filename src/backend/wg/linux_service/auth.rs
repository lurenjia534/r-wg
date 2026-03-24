use std::ffi::CString;
use std::fs;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixStream;
use std::path::Path;

#[derive(Debug, Clone, Copy)]
pub(super) struct PeerCredentials {
    pub(super) pid: u32,
    pub(super) uid: u32,
}

pub(super) fn peer_credentials(stream: &UnixStream) -> io::Result<PeerCredentials> {
    let mut creds = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut creds as *mut libc::ucred).cast(),
            &mut len,
        )
    };
    if rc == 0 {
        Ok(PeerCredentials {
            pid: creds.pid as u32,
            uid: creds.uid,
        })
    } else {
        Err(io::Error::last_os_error())
    }
}

pub(super) fn is_peer_allowed(
    creds: PeerCredentials,
    allowed_uid: Option<u32>,
    allowed_gid: Option<u32>,
) -> bool {
    if creds.uid == 0 {
        return true;
    }
    if allowed_uid.is_some_and(|uid| uid == creds.uid) {
        return true;
    }
    if let Some(gid) = allowed_gid {
        return peer_in_group(creds.pid, gid).unwrap_or(false);
    }
    allowed_uid.is_none() && allowed_gid.is_none()
}

pub(super) fn peer_in_group(pid: u32, wanted_gid: u32) -> io::Result<bool> {
    let status = fs::read_to_string(format!("/proc/{pid}/status"))?;
    let groups = status
        .lines()
        .find(|line| line.starts_with("Groups:"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Groups line missing"))?;

    Ok(groups
        .split_whitespace()
        .skip(1)
        .filter_map(|value| value.parse::<u32>().ok())
        .any(|gid| gid == wanted_gid))
}

pub(super) fn socket_access_status(socket_path: &Path) -> io::Result<Option<bool>> {
    let socket_path = CString::new(socket_path.as_os_str().as_bytes()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "socket path contains interior NUL byte",
        )
    })?;
    let rc = unsafe { libc::access(socket_path.as_ptr(), libc::W_OK) };
    if rc == 0 {
        return Ok(Some(true));
    }

    let err = io::Error::last_os_error();
    match err.kind() {
        io::ErrorKind::NotFound => Ok(None),
        io::ErrorKind::PermissionDenied => Ok(Some(false)),
        _ => Err(err),
    }
}
