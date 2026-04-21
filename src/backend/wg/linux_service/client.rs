use std::io::{self, BufReader};
use std::path::PathBuf;
use std::sync::Arc;

use std::os::unix::net::UnixStream;

use super::super::ipc::{
    read_json_line, write_json_line, BackendCommand, BackendReply, IPC_PROTOCOL_VERSION,
};
use super::super::ipc_client::{self, BackendTransport};
use super::super::{
    EngineError, EngineRuntimeSnapshot, EngineStats, EngineStatus, RelayInventoryStatusSnapshot,
    StartRequest,
};
use super::auth::socket_access_status;
use super::install_model::{
    control_socket_path, installation_exists, PrivilegedServiceStatus, SERVICE_IO_TIMEOUT,
    SERVICE_UNIT_NAME,
};
use super::systemd::systemd_unit_is_active;
use super::{connect_error, is_access_denied_error, is_missing_backend_error};

#[derive(Clone)]
pub struct Engine {
    inner: Arc<RemoteEngine>,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub(super) struct RemoteEngine {
    socket_path: Arc<PathBuf>,
}

pub fn probe_privileged_service() -> PrivilegedServiceStatus {
    if !installation_exists() {
        return PrivilegedServiceStatus::NotInstalled;
    }

    match socket_access_status(control_socket_path().as_path()) {
        Ok(Some(false)) => return PrivilegedServiceStatus::AccessDenied,
        Ok(Some(true)) | Ok(None) => {}
        Err(err) => {
            return PrivilegedServiceStatus::Unreachable(format!(
                "failed to inspect Linux privileged backend socket access: {err}"
            ))
        }
    }

    if systemd_unit_is_active(SERVICE_UNIT_NAME) {
        let engine = RemoteEngine::new();
        return match engine.send_command_raw(BackendCommand::Info) {
            Ok(BackendReply::Info { protocol_version }) => {
                if protocol_version == IPC_PROTOCOL_VERSION {
                    match engine.status() {
                        Ok(_) => PrivilegedServiceStatus::Running,
                        Err(EngineError::AccessDenied) => PrivilegedServiceStatus::AccessDenied,
                        Err(err) => PrivilegedServiceStatus::Unreachable(format!(
                            "Linux privileged backend service is running but its worker channel is unavailable: {err}"
                        )),
                    }
                } else {
                    PrivilegedServiceStatus::VersionMismatch {
                        expected: IPC_PROTOCOL_VERSION,
                        actual: protocol_version,
                    }
                }
            }
            Ok(other) => {
                PrivilegedServiceStatus::Unreachable(format!("unexpected backend reply: {other:?}"))
            }
            Err(err) if is_access_denied_error(&err) => PrivilegedServiceStatus::AccessDenied,
            Err(err) => PrivilegedServiceStatus::Unreachable(format!(
                "failed to reach Linux privileged backend: {err}"
            )),
        };
    }

    PrivilegedServiceStatus::Installed
}

impl Engine {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RemoteEngine::new()),
        }
    }

    pub fn start(&self, request: StartRequest) -> Result<(), EngineError> {
        self.inner.start(request)
    }

    pub fn stop(&self) -> Result<(), EngineError> {
        self.inner.stop()
    }

    pub fn status(&self) -> Result<EngineStatus, EngineError> {
        self.inner.status()
    }

    pub fn stats(&self) -> Result<EngineStats, EngineError> {
        self.inner.stats()
    }

    pub fn apply_report(
        &self,
    ) -> Result<Option<crate::core::route_plan::RouteApplyReport>, EngineError> {
        self.inner.apply_report()
    }

    pub fn runtime_snapshot(&self) -> Result<EngineRuntimeSnapshot, EngineError> {
        self.inner.runtime_snapshot()
    }

    pub fn relay_inventory_status(&self) -> Result<RelayInventoryStatusSnapshot, EngineError> {
        self.inner.relay_inventory_status()
    }

    pub fn refresh_relay_inventory(&self) -> Result<RelayInventoryStatusSnapshot, EngineError> {
        self.inner.refresh_relay_inventory()
    }
}

impl RemoteEngine {
    pub(super) fn new() -> Self {
        Self {
            socket_path: Arc::new(control_socket_path()),
        }
    }

    pub(super) fn start(&self, request: StartRequest) -> Result<(), EngineError> {
        ipc_client::start(self, request)
    }

    pub(super) fn stop(&self) -> Result<(), EngineError> {
        ipc_client::stop(self, EngineError::NotRunning)
    }

    pub(super) fn status(&self) -> Result<EngineStatus, EngineError> {
        ipc_client::status(self)
    }

    pub(super) fn stats(&self) -> Result<EngineStats, EngineError> {
        ipc_client::stats(self, EngineError::NotRunning)
    }

    pub(super) fn apply_report(
        &self,
    ) -> Result<Option<crate::core::route_plan::RouteApplyReport>, EngineError> {
        ipc_client::apply_report(self, EngineError::NotRunning)
    }

    pub(super) fn runtime_snapshot(&self) -> Result<EngineRuntimeSnapshot, EngineError> {
        ipc_client::runtime_snapshot(self, EngineError::NotRunning)
    }

    pub(super) fn relay_inventory_status(
        &self,
    ) -> Result<RelayInventoryStatusSnapshot, EngineError> {
        ipc_client::relay_inventory_status(self, EngineError::ChannelClosed)
    }

    pub(super) fn refresh_relay_inventory(
        &self,
    ) -> Result<RelayInventoryStatusSnapshot, EngineError> {
        ipc_client::refresh_relay_inventory(self, EngineError::ChannelClosed)
    }

    pub(super) fn send_command_raw(
        &self,
        command: BackendCommand,
    ) -> Result<BackendReply, io::Error> {
        let mut stream = UnixStream::connect(self.socket_path.as_path())?;
        let _ = stream.set_read_timeout(Some(SERVICE_IO_TIMEOUT));
        let _ = stream.set_write_timeout(Some(SERVICE_IO_TIMEOUT));
        write_json_line(&mut stream, &command)?;
        let mut reader = BufReader::new(stream);
        read_json_line(&mut reader)
    }
}

impl BackendTransport for RemoteEngine {
    fn send_command_raw(&self, command: BackendCommand) -> Result<BackendReply, io::Error> {
        Self::send_command_raw(self, command)
    }

    fn connect_error(&self, err: io::Error) -> EngineError {
        connect_error(self.socket_path.as_path(), err)
    }

    fn is_missing_backend_error(&self, err: &io::Error) -> bool {
        is_missing_backend_error(err)
    }

    fn is_access_denied_error(&self, err: &io::Error) -> bool {
        is_access_denied_error(err)
    }

    fn is_timeout_error(&self, err: &io::Error) -> bool {
        matches!(
            err.kind(),
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
        ) || matches!(err.raw_os_error(), Some(libc::EAGAIN))
    }
}
