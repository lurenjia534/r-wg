use std::env;
use std::ffi::{OsStr, OsString};
use std::io::{self, BufReader};
use std::sync::Arc;

use windows::core::PCWSTR;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

use super::ipc::{
    map_backend_error, protocol_mismatch, read_json_line, unexpected_reply, write_json_line,
    BackendCommand, BackendReply, IPC_PROTOCOL_VERSION,
};
use super::windows_pipe::PipeStream;
use super::windows_service_host;
use super::windows_service_manager;
use super::{EngineError, EngineStats, EngineStatus, StartRequest};

pub(crate) const SERVICE_NAME: &str = "r-wg-service";
pub(crate) const SERVICE_DISPLAY_NAME: &str = "r-wg Privileged Backend";
pub(crate) const SERVICE_ARG: &str = "--run-service";
pub(crate) const SERVICE_SUBCOMMAND: &str = "service";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrivilegedServiceStatus {
    Running,
    Installed,
    NotInstalled,
    AccessDenied,
    VersionMismatch { expected: u32, actual: u32 },
    Unreachable(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivilegedServiceAction {
    Install,
    Repair,
    Remove,
}

impl PrivilegedServiceAction {
    pub(crate) fn as_cli(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Repair => "repair",
            Self::Remove => "remove",
        }
    }
}

#[derive(Clone)]
pub struct Engine {
    inner: Arc<RemoteEngine>,
}

#[derive(Clone, Default)]
struct RemoteEngine;

enum WindowsEntryCommand {
    ServiceMode,
    Manage(Vec<String>),
}

pub fn maybe_run_service_mode() -> bool {
    let entry = match parse_windows_entry_command(env::args_os()) {
        Ok(entry) => entry,
        Err(err) => exit_windows_entry_error("windows privileged backend command parse failed", err),
    };
    let Some(entry) = entry else {
        return false;
    };

    crate::log::init();

    let result = match entry {
        WindowsEntryCommand::ServiceMode => {
            let _mtu = gotatun::tun::MtuWatcher::new(1500);
            windows_service_host::run_service_dispatcher()
        }
        WindowsEntryCommand::Manage(command) => windows_service_manager::run_manage_command(&command),
    };
    if let Err(err) = result {
        exit_windows_entry_error("windows privileged backend command failed", err);
    }

    true
}

pub fn probe_privileged_service() -> PrivilegedServiceStatus {
    windows_service_manager::probe_privileged_service()
}

pub fn manage_privileged_service(action: PrivilegedServiceAction) -> Result<(), EngineError> {
    windows_service_manager::manage_privileged_service(action)
}

impl Engine {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RemoteEngine),
        }
    }

    pub fn info(&self) -> Result<u32, EngineError> {
        self.inner.info()
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
}

impl RemoteEngine {
    fn info(&self) -> Result<u32, EngineError> {
        match self.send_command_raw(BackendCommand::Info) {
            Ok(BackendReply::Info { protocol_version }) => Ok(protocol_version),
            Ok(BackendReply::Error { kind, message }) => Err(map_backend_error(kind, message)),
            Ok(other) => Err(unexpected_reply(other)),
            Err(err) if is_missing_backend_error(&err) => Err(EngineError::ChannelClosed),
            Err(err) if is_access_denied_error(&err) => Err(EngineError::AccessDenied),
            Err(err) => Err(connect_error(err)),
        }
    }

    fn start(&self, request: StartRequest) -> Result<(), EngineError> {
        self.check_protocol()?;
        let reply = self
            .send_command_raw(BackendCommand::Start { request })
            .map_err(connect_error)?;
        self.expect_unit(reply)
    }

    fn stop(&self) -> Result<(), EngineError> {
        match self.send_command_raw(BackendCommand::Stop) {
            Ok(reply) => self.expect_unit(reply),
            Err(err) if is_missing_backend_error(&err) => Err(EngineError::NotRunning),
            Err(err) if is_access_denied_error(&err) => Err(EngineError::AccessDenied),
            Err(err) => Err(connect_error(err)),
        }
    }

    fn status(&self) -> Result<EngineStatus, EngineError> {
        match self.send_command_raw(BackendCommand::Status) {
            Ok(BackendReply::Status { status }) => Ok(status),
            Ok(BackendReply::Error { kind, message }) => Err(map_backend_error(kind, message)),
            Ok(other) => Err(unexpected_reply(other)),
            Err(err) if is_missing_backend_error(&err) => Ok(EngineStatus::Stopped),
            Err(err) if is_access_denied_error(&err) => Err(EngineError::AccessDenied),
            Err(err) => Err(connect_error(err)),
        }
    }

    fn stats(&self) -> Result<EngineStats, EngineError> {
        match self.send_command_raw(BackendCommand::Stats) {
            Ok(BackendReply::Stats { stats }) => Ok(stats),
            Ok(BackendReply::Error { kind, message }) => Err(map_backend_error(kind, message)),
            Ok(other) => Err(unexpected_reply(other)),
            Err(err) if is_missing_backend_error(&err) => Err(EngineError::NotRunning),
            Err(err) if is_access_denied_error(&err) => Err(EngineError::AccessDenied),
            Err(err) => Err(connect_error(err)),
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

    fn send_command_raw(&self, command: BackendCommand) -> Result<BackendReply, io::Error> {
        let mut stream = PipeStream::connect()?;
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

fn parse_windows_entry_command<I, S>(args: I) -> Result<Option<WindowsEntryCommand>, EngineError>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let mut args = args.into_iter().map(Into::into);
    let _ = args.next();
    let Some(first) = args.next() else {
        return Ok(None);
    };

    if first == SERVICE_ARG {
        return Ok(Some(WindowsEntryCommand::ServiceMode));
    }

    if first != SERVICE_SUBCOMMAND {
        return Ok(None);
    }

    Ok(Some(WindowsEntryCommand::Manage(
        args.map(|arg| arg.to_string_lossy().to_string()).collect(),
    )))
}

fn connect_error(err: io::Error) -> EngineError {
    EngineError::Remote(format!("failed to connect to Windows privileged backend pipe: {err}"))
}

fn is_missing_backend_error(err: &io::Error) -> bool {
    matches!(err.kind(), io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused)
        || matches!(err.raw_os_error(), Some(2 | 231 | 233))
}

fn is_access_denied_error(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::PermissionDenied || matches!(err.raw_os_error(), Some(5))
}

fn exit_windows_entry_error(context: &str, err: EngineError) -> ! {
    tracing::error!("{context}: {err}");
    std::process::exit(1);
}

pub(super) fn shell_execute_runas(exe: &OsStr, params: &str) -> Result<(), EngineError> {
    let verb_w = encode_wide("runas");
    let exe_w = encode_wide(&exe.to_string_lossy());
    let params_w = encode_wide(params);
    let empty_w = encode_wide("");

    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(verb_w.as_ptr()),
            PCWSTR(exe_w.as_ptr()),
            PCWSTR(params_w.as_ptr()),
            PCWSTR(empty_w.as_ptr()),
            SW_HIDE,
        )
    };
    if result.0 as isize <= 32 {
        return Err(EngineError::Remote(format!(
            "failed to launch elevated Windows service manager via UAC (code={})",
            result.0 as isize
        )));
    }
    Ok(())
}

pub(super) fn is_process_elevated() -> bool {
    unsafe {
        let mut token = Default::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }

        let mut elevation = TOKEN_ELEVATION::default();
        let mut out_len = 0u32;
        let result = GetTokenInformation(
            token,
            TokenElevation,
            Some((&mut elevation as *mut TOKEN_ELEVATION).cast()),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut out_len,
        );
        let _ = CloseHandle(token);
        result.is_ok() && elevation.TokenIsElevated != 0
    }
}

fn encode_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
