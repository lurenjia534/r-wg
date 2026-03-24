use std::env;
use std::io;
use std::os::fd::FromRawFd;
use std::os::unix::net::UnixListener;
use std::process::Command;

use super::super::EngineError;
use super::remote_error;

pub(super) fn inherited_listener() -> Result<Option<UnixListener>, EngineError> {
    let listen_pid = env::var("LISTEN_PID")
        .ok()
        .and_then(|value| value.parse::<u32>().ok());
    let listen_fds = env::var("LISTEN_FDS")
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(0);

    if listen_pid != Some(std::process::id()) || listen_fds <= 0 {
        return Ok(None);
    }

    let fd = 3;
    let rc = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if rc < 0 {
        return Err(remote_error(format!(
            "systemd passed invalid listener fd: {}",
            io::Error::last_os_error()
        )));
    }

    let listener = unsafe { UnixListener::from_raw_fd(fd) };
    Ok(Some(listener))
}

pub(super) fn systemd_unit_is_active(unit: &str) -> bool {
    systemctl_success(["is-active", "--quiet", unit])
}

pub(super) fn systemctl_success<const N: usize>(args: [&str; N]) -> bool {
    Command::new("systemctl")
        .args(args)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

pub(super) fn run_command<const N: usize>(
    program: &str,
    args: [&str; N],
) -> Result<(), EngineError> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|err| remote_error(format!("failed to run {program}: {err}")))?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        format!("exit status {}", output.status)
    };
    Err(remote_error(format!("{program} failed: {detail}")))
}
