use std::io::{self, BufReader};
use std::path::PathBuf;
use std::sync::Arc;

use std::os::unix::net::UnixStream;

use super::super::ipc::{
    map_backend_error, protocol_mismatch, read_json_line, unexpected_reply, write_json_line,
    BackendCommand, BackendReply, IPC_PROTOCOL_VERSION,
};
use super::super::{EngineError, EngineStats, EngineStatus, StartRequest};
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
                    PrivilegedServiceStatus::Running
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
    ) -> Result<Option<crate::backend::wg::route_plan::RouteApplyReport>, EngineError> {
        self.inner.apply_report()
    }
}

impl RemoteEngine {
    pub(super) fn new() -> Self {
        Self {
            socket_path: Arc::new(control_socket_path()),
        }
    }

    fn info(&self) -> Result<u32, EngineError> {
        match self.send_command_raw(BackendCommand::Info) {
            Ok(BackendReply::Info { protocol_version }) => Ok(protocol_version),
            Ok(BackendReply::Error { kind, message }) => Err(map_backend_error(kind, message)),
            Ok(other) => Err(unexpected_reply(other)),
            Err(err) if is_missing_backend_error(&err) => Err(EngineError::ChannelClosed),
            Err(err) if is_access_denied_error(&err) => Err(EngineError::AccessDenied),
            Err(err) => Err(connect_error(self.socket_path.as_path(), err)),
        }
    }

    pub(super) fn start(&self, request: StartRequest) -> Result<(), EngineError> {
        let reply = self.send_command(BackendCommand::Start { request })?;
        self.expect_unit(reply)
    }

    pub(super) fn stop(&self) -> Result<(), EngineError> {
        match self.send_command_raw(BackendCommand::Stop) {
            Ok(reply) => self.expect_unit(reply),
            Err(err) if is_missing_backend_error(&err) => Err(EngineError::NotRunning),
            Err(err) if is_access_denied_error(&err) => Err(EngineError::AccessDenied),
            Err(err) => Err(connect_error(self.socket_path.as_path(), err)),
        }
    }

    pub(super) fn status(&self) -> Result<EngineStatus, EngineError> {
        match self.send_command_raw(BackendCommand::Status) {
            Ok(BackendReply::Status { status }) => Ok(status),
            Ok(BackendReply::Error { kind, message }) => Err(map_backend_error(kind, message)),
            Ok(other) => Err(unexpected_reply(other)),
            Err(err) if is_missing_backend_error(&err) => Ok(EngineStatus::Stopped),
            Err(err) if is_access_denied_error(&err) => Err(EngineError::AccessDenied),
            Err(err) => Err(connect_error(self.socket_path.as_path(), err)),
        }
    }

    pub(super) fn stats(&self) -> Result<EngineStats, EngineError> {
        match self.send_command_raw(BackendCommand::Stats) {
            Ok(BackendReply::Stats { stats }) => Ok(stats),
            Ok(BackendReply::Error { kind, message }) => Err(map_backend_error(kind, message)),
            Ok(other) => Err(unexpected_reply(other)),
            Err(err) if is_missing_backend_error(&err) => Err(EngineError::NotRunning),
            Err(err) if is_access_denied_error(&err) => Err(EngineError::AccessDenied),
            Err(err) => Err(connect_error(self.socket_path.as_path(), err)),
        }
    }

    pub(super) fn apply_report(
        &self,
    ) -> Result<Option<crate::backend::wg::route_plan::RouteApplyReport>, EngineError> {
        self.check_protocol()?;
        match self.send_command_raw(BackendCommand::ApplyReport) {
            Ok(BackendReply::ApplyReport { report }) => Ok(report),
            Ok(BackendReply::Error { kind, message }) => Err(map_backend_error(kind, message)),
            Ok(other) => Err(unexpected_reply(other)),
            Err(err) if is_missing_backend_error(&err) => Err(EngineError::NotRunning),
            Err(err) if is_access_denied_error(&err) => Err(EngineError::AccessDenied),
            Err(err) => Err(connect_error(self.socket_path.as_path(), err)),
        }
    }

    fn check_protocol(&self) -> Result<(), EngineError> {
        match self.info() {
            Ok(protocol_version) => {
                if protocol_version == IPC_PROTOCOL_VERSION {
                    Ok(())
                } else {
                    Err(protocol_mismatch(IPC_PROTOCOL_VERSION, protocol_version))
                }
            }
            Err(err) => Err(err),
        }
    }

    fn send_command(&self, command: BackendCommand) -> Result<BackendReply, EngineError> {
        self.send_command_raw(command)
            .map_err(|err| connect_error(self.socket_path.as_path(), err))
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

    fn expect_unit(&self, reply: BackendReply) -> Result<(), EngineError> {
        match reply {
            BackendReply::Ok => Ok(()),
            BackendReply::Error { kind, message } => Err(map_backend_error(kind, message)),
            other => Err(unexpected_reply(other)),
        }
    }
}
