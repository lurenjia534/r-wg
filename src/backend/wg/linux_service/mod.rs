//! Linux privileged backend:
//! - UI/CLI talks to a root-owned systemd service over a Unix socket.
//! - The service owns TUN, routes, and DNS lifecycle.
//! - Install/repair/remove flows stay separate from the runtime server loop.

mod auth;
mod client;
mod entry;
mod fs_ops;
mod install_model;
mod manage;
mod render;
mod server;
mod systemd;
#[cfg(test)]
mod tests;

use std::io;
use std::path::Path;

use super::EngineError;

pub use client::{probe_privileged_service, Engine};
pub use entry::maybe_run_service_mode;
pub use install_model::{PrivilegedServiceAction, PrivilegedServiceStatus};
pub use manage::manage_privileged_service;

fn remote_error(message: String) -> EngineError {
    EngineError::Remote(message)
}

fn is_missing_backend_error(err: &io::Error) -> bool {
    matches!(
        err.raw_os_error(),
        Some(libc::ENOENT | libc::ECONNREFUSED | libc::ECONNRESET)
    )
}

fn is_access_denied_error(err: &io::Error) -> bool {
    matches!(err.raw_os_error(), Some(libc::EACCES | libc::EPERM))
}

fn connect_error(socket_path: &Path, err: io::Error) -> EngineError {
    if is_access_denied_error(&err) {
        return EngineError::AccessDenied;
    }
    if matches!(
        err.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    ) || matches!(err.raw_os_error(), Some(libc::EAGAIN))
    {
        return remote_error(format!(
            "timed out waiting for Linux privileged backend reply {}: {err}",
            socket_path.display()
        ));
    }
    if is_missing_backend_error(&err) {
        return remote_error(format!(
            "Linux privileged backend is not installed or not running ({})",
            socket_path.display()
        ));
    }
    remote_error(format!(
        "failed to reach Linux privileged backend {}: {err}",
        socket_path.display()
    ))
}

fn ensure_root() -> Result<(), EngineError> {
    if is_running_as_root() {
        Ok(())
    } else {
        Err(remote_error(
            "service management commands must run as root".to_string(),
        ))
    }
}

fn is_running_as_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}
