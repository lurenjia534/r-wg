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
    read_json_line, write_json_line, BackendCommand, BackendReply, IPC_PROTOCOL_VERSION,
};
use super::ipc_client::{self, BackendTransport};
use super::windows_pipe::PipeStream;
use super::windows_service_host;
use super::windows_service_manager;
use super::{
    EngineError, EngineRuntimeSnapshot, EngineStats, EngineStatus, RelayInventoryStatusSnapshot,
    StartRequest,
};

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
        Err(err) => {
            exit_windows_entry_error("windows privileged backend command parse failed", err)
        }
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
        WindowsEntryCommand::Manage(command) => {
            windows_service_manager::run_manage_command(&command)
        }
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
    fn info(&self) -> Result<u32, EngineError> {
        ipc_client::info(self)
    }

    fn start(&self, request: StartRequest) -> Result<(), EngineError> {
        ipc_client::start(self, request)
    }

    fn stop(&self) -> Result<(), EngineError> {
        ipc_client::stop(self, EngineError::NotRunning)
    }

    fn status(&self) -> Result<EngineStatus, EngineError> {
        ipc_client::status(self)
    }

    fn stats(&self) -> Result<EngineStats, EngineError> {
        ipc_client::stats(self, EngineError::NotRunning)
    }

    fn apply_report(
        &self,
    ) -> Result<Option<crate::core::route_plan::RouteApplyReport>, EngineError> {
        ipc_client::apply_report(self, EngineError::ChannelClosed)
    }

    fn runtime_snapshot(&self) -> Result<EngineRuntimeSnapshot, EngineError> {
        ipc_client::runtime_snapshot(self, EngineError::ChannelClosed)
    }

    fn relay_inventory_status(&self) -> Result<RelayInventoryStatusSnapshot, EngineError> {
        ipc_client::relay_inventory_status(self, EngineError::ChannelClosed)
    }

    fn refresh_relay_inventory(&self) -> Result<RelayInventoryStatusSnapshot, EngineError> {
        ipc_client::refresh_relay_inventory(self, EngineError::ChannelClosed)
    }

    fn send_command_raw(&self, command: BackendCommand) -> Result<BackendReply, io::Error> {
        let mut stream = PipeStream::connect()?;
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
        connect_error(err)
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
        )
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
    EngineError::Remote(format!(
        "failed to connect to Windows privileged backend pipe: {err}"
    ))
}

fn is_missing_backend_error(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
    ) || matches!(err.raw_os_error(), Some(2 | 231 | 233))
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
