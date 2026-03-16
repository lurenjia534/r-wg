use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_SERVICE_DOES_NOT_EXIST};
use windows::Win32::System::Services::{
    ChangeServiceConfigW, CloseServiceHandle, ControlService, CreateServiceW, DeleteService,
    OpenSCManagerW, OpenServiceW, QueryServiceStatus, SC_HANDLE, SC_MANAGER_CONNECT,
    SC_MANAGER_CREATE_SERVICE, SERVICE_ALL_ACCESS, SERVICE_AUTO_START, SERVICE_CONTROL_STOP,
    SERVICE_ERROR_NORMAL, SERVICE_QUERY_STATUS, SERVICE_STATUS, SERVICE_STATUS_CURRENT_STATE,
    SERVICE_WIN32_OWN_PROCESS,
};

use super::ipc::IPC_PROTOCOL_VERSION;
use super::windows_service::{
    is_process_elevated, shell_execute_runas, PrivilegedServiceAction, PrivilegedServiceStatus,
    SERVICE_ARG, SERVICE_DISPLAY_NAME, SERVICE_NAME, SERVICE_SUBCOMMAND,
};
use super::{EngineError, Engine};

const MANAGER_POLL_INTERVAL: Duration = Duration::from_millis(300);
const MANAGER_WAIT_TIMEOUT: Duration = Duration::from_secs(12);

pub fn probe_privileged_service() -> PrivilegedServiceStatus {
    let manager = match ServiceManager::connect(SC_MANAGER_CONNECT) {
        Ok(manager) => manager,
        Err(EngineError::AccessDenied) => return PrivilegedServiceStatus::AccessDenied,
        Err(err) => {
            return PrivilegedServiceStatus::Unreachable(format!(
                "failed to open Windows service control manager: {err}"
            ));
        }
    };

    let service = match manager.open_service(SERVICE_QUERY_STATUS) {
        Ok(Some(service)) => service,
        Ok(None) => return PrivilegedServiceStatus::NotInstalled,
        Err(EngineError::AccessDenied) => return PrivilegedServiceStatus::AccessDenied,
        Err(err) => {
            return PrivilegedServiceStatus::Unreachable(format!(
                "failed to open Windows privileged backend service: {err}"
            ));
        }
    };

    match service.current_state() {
        Ok(state) if state == windows::Win32::System::Services::SERVICE_RUNNING => {
            match Engine::new().info() {
                Ok(protocol_version) if protocol_version == IPC_PROTOCOL_VERSION => {
                    PrivilegedServiceStatus::Running
                }
                Ok(protocol_version) => PrivilegedServiceStatus::VersionMismatch {
                    expected: IPC_PROTOCOL_VERSION,
                    actual: protocol_version,
                },
                Err(EngineError::AccessDenied) => PrivilegedServiceStatus::AccessDenied,
                Err(err) => PrivilegedServiceStatus::Unreachable(format!(
                    "failed to reach Windows privileged backend pipe: {err}"
                )),
            }
        }
        Ok(_) => PrivilegedServiceStatus::Installed,
        Err(EngineError::AccessDenied) => PrivilegedServiceStatus::AccessDenied,
        Err(err) => PrivilegedServiceStatus::Unreachable(format!(
            "failed to query Windows privileged backend service state: {err}"
        )),
    }
}

pub fn manage_privileged_service(action: PrivilegedServiceAction) -> Result<(), EngineError> {
    let current_exe = env::current_exe()
        .map_err(|err| EngineError::Remote(format!("failed to locate current exe: {err}")))?;

    let args = build_manage_args(action, &current_exe);
    if is_process_elevated() {
        return run_manage_command(&args[1..]);
    }

    let params = args
        .into_iter()
        .map(|arg| quote_windows_arg(&arg))
        .collect::<Vec<_>>()
        .join(" ");
    shell_execute_runas(current_exe.as_os_str(), &params)?;
    wait_for_manager_result(action)
}

pub(crate) fn run_manage_command(args: &[String]) -> Result<(), EngineError> {
    let command = parse_manage_command(args)?;
    match command {
        ManageCommand::Install(options) => install_or_repair(options, false),
        ManageCommand::Repair(options) => install_or_repair(options, true),
        ManageCommand::Remove(options) => remove_installation(options),
    }
}

enum ManageCommand {
    Install(InstallOptions),
    Repair(InstallOptions),
    Remove(RemoveOptions),
}

struct InstallOptions {
    source_path: PathBuf,
    binary_path: PathBuf,
}

struct RemoveOptions {
    binary_path: PathBuf,
}

fn parse_manage_command(args: &[String]) -> Result<ManageCommand, EngineError> {
    if args.is_empty() {
        return Err(remote_error(
            "missing service action (install/repair/remove)".to_string(),
        ));
    }

    let action = args[0].as_str();
    let mut source_path = None;
    let binary_path = installed_binary_path();
    let mut idx = 1usize;
    while idx < args.len() {
        match args[idx].as_str() {
            "--source" => {
                idx += 1;
                let Some(value) = args.get(idx) else {
                    return Err(remote_error("missing value for --source".to_string()));
                };
                source_path = Some(PathBuf::from(value));
            }
            other => {
                return Err(remote_error(format!(
                    "unexpected service manager argument: {other}"
                )));
            }
        }
        idx += 1;
    }

    match action {
        "install" => Ok(ManageCommand::Install(InstallOptions {
            source_path: source_path
                .ok_or_else(|| remote_error("service install requires --source".to_string()))?,
            binary_path,
        })),
        "repair" => Ok(ManageCommand::Repair(InstallOptions {
            source_path: source_path
                .ok_or_else(|| remote_error("service repair requires --source".to_string()))?,
            binary_path,
        })),
        "remove" => Ok(ManageCommand::Remove(RemoveOptions { binary_path })),
        other => Err(remote_error(format!("unknown service action: {other}"))),
    }
}

fn install_or_repair(options: InstallOptions, repairing: bool) -> Result<(), EngineError> {
    if !options.source_path.is_file() {
        return Err(remote_error(format!(
            "source binary not found: {}",
            options.source_path.display()
        )));
    }

    if repairing {
        let _ = remove_service_registration();
    }

    install_binary(&options.source_path, &options.binary_path)?;
    let manager = ServiceManager::connect(SC_MANAGER_CONNECT | SC_MANAGER_CREATE_SERVICE)?;
    let service = match manager.open_service(SERVICE_ALL_ACCESS) {
        Ok(Some(service)) => {
            reconfigure_service(&service, &options.binary_path)?;
            service
        }
        Ok(None) => create_service(&manager, &options.binary_path)?,
        Err(err) => return Err(remote_error(format!("failed to open service for update: {err}"))),
    };

    start_service(&service)?;
    wait_for_service_state(
        &service,
        windows::Win32::System::Services::SERVICE_RUNNING,
        MANAGER_WAIT_TIMEOUT,
    )?;
    wait_for_manager_result(PrivilegedServiceAction::Install)?;
    Ok(())
}

fn remove_installation(options: RemoveOptions) -> Result<(), EngineError> {
    let _ = remove_service_registration();

    match fs::remove_file(&options.binary_path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(remote_error(format!(
                "failed to remove installed binary {}: {err}",
                options.binary_path.display()
            )));
        }
    }

    if let Some(parent) = options.binary_path.parent() {
        match fs::remove_dir(parent) {
            Ok(()) => {}
            Err(err)
                if err.kind() == std::io::ErrorKind::NotFound
                    || err.kind() == std::io::ErrorKind::DirectoryNotEmpty => {}
            Err(err) => {
                return Err(remote_error(format!(
                    "failed to clean install dir {}: {err}",
                    parent.display()
                )));
            }
        }
    }

    Ok(())
}

fn remove_service_registration() -> Result<(), EngineError> {
    let manager = ServiceManager::connect(SC_MANAGER_CONNECT)?;
    let Some(service) = manager.open_service(SERVICE_ALL_ACCESS)? else {
        return Ok(());
    };

    let state = service.current_state()?;
    if state != windows::Win32::System::Services::SERVICE_STOPPED {
        let mut status = SERVICE_STATUS::default();
        unsafe {
            let _ = ControlService(service.raw(), SERVICE_CONTROL_STOP, &mut status);
        }
        let _ = wait_for_service_state(
            &service,
            windows::Win32::System::Services::SERVICE_STOPPED,
            Duration::from_secs(15),
        );
    }

    unsafe { DeleteService(service.raw()) }
        .map_err(|err| remote_error(format!("failed to delete Windows service: {err}")))?;
    Ok(())
}

fn install_binary(source: &Path, binary: &Path) -> Result<(), EngineError> {
    if let Some(parent) = binary.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| remote_error(format!("failed to create install dir: {err}")))?;
    }
    fs::copy(source, binary)
        .map_err(|err| remote_error(format!("failed to copy installed binary: {err}")))?;
    Ok(())
}

fn create_service(manager: &ServiceManager, binary_path: &Path) -> Result<ServiceHandle, EngineError> {
    let service_name = encode_wide(SERVICE_NAME);
    let display_name = encode_wide(SERVICE_DISPLAY_NAME);
    let binary = encode_wide(&service_binary_path(binary_path));
    let account = encode_wide("LocalSystem");
    let handle = unsafe {
        CreateServiceW(
            manager.raw(),
            PCWSTR(service_name.as_ptr()),
            PCWSTR(display_name.as_ptr()),
            SERVICE_ALL_ACCESS,
            SERVICE_WIN32_OWN_PROCESS,
            SERVICE_AUTO_START,
            SERVICE_ERROR_NORMAL,
            PCWSTR(binary.as_ptr()),
            None,
            None,
            None,
            PCWSTR(account.as_ptr()),
            None,
        )
    }
    .map_err(|err| remote_error(format!("failed to create Windows service: {err}")))?;
    Ok(ServiceHandle(handle))
}

fn reconfigure_service(service: &ServiceHandle, binary_path: &Path) -> Result<(), EngineError> {
    let display_name = encode_wide(SERVICE_DISPLAY_NAME);
    let binary = encode_wide(&service_binary_path(binary_path));
    let account = encode_wide("LocalSystem");
    unsafe {
        ChangeServiceConfigW(
            service.raw(),
            SERVICE_WIN32_OWN_PROCESS,
            SERVICE_AUTO_START,
            SERVICE_ERROR_NORMAL,
            PCWSTR(binary.as_ptr()),
            None,
            None,
            None,
            PCWSTR(account.as_ptr()),
            None,
            PCWSTR(display_name.as_ptr()),
        )
    }
    .map_err(|err| remote_error(format!("failed to update Windows service config: {err}")))?;
    Ok(())
}

fn start_service(service: &ServiceHandle) -> Result<(), EngineError> {
    let state = service.current_state()?;
    if state == windows::Win32::System::Services::SERVICE_RUNNING {
        return Ok(());
    }
    unsafe { windows::Win32::System::Services::StartServiceW(service.raw(), None) }
        .map_err(|err| remote_error(format!("failed to start Windows service: {err}")))?;
    Ok(())
}

fn wait_for_service_state(
    service: &ServiceHandle,
    desired_state: SERVICE_STATUS_CURRENT_STATE,
    timeout: Duration,
) -> Result<(), EngineError> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if service.current_state()? == desired_state {
            return Ok(());
        }
        thread::sleep(MANAGER_POLL_INTERVAL);
    }
    Err(remote_error(format!(
        "timed out waiting for Windows service state {}",
        desired_state.0
    )))
}

fn wait_for_manager_result(action: PrivilegedServiceAction) -> Result<(), EngineError> {
    let start = Instant::now();
    while start.elapsed() < MANAGER_WAIT_TIMEOUT {
        match (action, probe_privileged_service()) {
            (PrivilegedServiceAction::Remove, PrivilegedServiceStatus::NotInstalled) => return Ok(()),
            (PrivilegedServiceAction::Install, PrivilegedServiceStatus::Running)
            | (PrivilegedServiceAction::Install, PrivilegedServiceStatus::Installed)
            | (PrivilegedServiceAction::Repair, PrivilegedServiceStatus::Running)
            | (PrivilegedServiceAction::Repair, PrivilegedServiceStatus::Installed) => return Ok(()),
            (_, PrivilegedServiceStatus::VersionMismatch { expected, actual }) => {
                return Err(EngineError::VersionMismatch { expected, actual });
            }
            (_, PrivilegedServiceStatus::AccessDenied) => return Err(EngineError::AccessDenied),
            _ => thread::sleep(MANAGER_POLL_INTERVAL),
        }
    }

    Err(remote_error(format!(
        "timed out waiting for Windows service {}",
        action.as_cli()
    )))
}

fn build_manage_args(action: PrivilegedServiceAction, current_exe: &Path) -> Vec<String> {
    let mut args = vec![
        SERVICE_SUBCOMMAND.to_string(),
        action.as_cli().to_string(),
    ];
    if !matches!(action, PrivilegedServiceAction::Remove) {
        args.push("--source".to_string());
        args.push(current_exe.display().to_string());
    }
    args
}

fn installed_binary_path() -> PathBuf {
    let base = env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    base.join("r-wg").join("r-wg.exe")
}

fn service_binary_path(binary_path: &Path) -> String {
    format!("{} {}", quote_windows_arg(&binary_path.display().to_string()), SERVICE_ARG)
}

fn remote_error(message: String) -> EngineError {
    EngineError::Remote(message)
}

struct ServiceManager(SC_HANDLE);

impl ServiceManager {
    fn connect(access: u32) -> Result<Self, EngineError> {
        let handle = unsafe { OpenSCManagerW(None, None, access) }.map_err(map_service_error)?;
        Ok(Self(handle))
    }

    fn raw(&self) -> SC_HANDLE {
        self.0
    }

    fn open_service(&self, access: u32) -> Result<Option<ServiceHandle>, EngineError> {
        let name = encode_wide(SERVICE_NAME);
        match unsafe { OpenServiceW(self.raw(), PCWSTR(name.as_ptr()), access) } {
            Ok(handle) => Ok(Some(ServiceHandle(handle))),
            Err(err) if win32_error_code(&err) == ERROR_SERVICE_DOES_NOT_EXIST.0 => Ok(None),
            Err(err) => Err(map_service_error(err)),
        }
    }
}

impl Drop for ServiceManager {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseServiceHandle(self.0);
        }
    }
}

struct ServiceHandle(SC_HANDLE);

impl ServiceHandle {
    fn raw(&self) -> SC_HANDLE {
        self.0
    }

    fn current_state(&self) -> Result<SERVICE_STATUS_CURRENT_STATE, EngineError> {
        let mut status = SERVICE_STATUS::default();
        unsafe { QueryServiceStatus(self.raw(), &mut status) }.map_err(map_service_error)?;
        Ok(status.dwCurrentState)
    }
}

impl Drop for ServiceHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseServiceHandle(self.0);
        }
    }
}

fn map_service_error(err: windows::core::Error) -> EngineError {
    let code = win32_error_code(&err);
    if code == ERROR_ACCESS_DENIED.0 {
        EngineError::AccessDenied
    } else {
        remote_error(err.to_string())
    }
}

fn win32_error_code(err: &windows::core::Error) -> u32 {
    let code = err.code().0 as u32;
    if (code & 0xFFFF_0000) == 0x8007_0000 {
        code & 0xFFFF
    } else {
        code
    }
}

fn encode_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn quote_windows_arg(value: &str) -> String {
    if !value.chars().any(|ch| ch.is_whitespace() || ch == '"') {
        return value.to_string();
    }

    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    let mut backslashes = 0usize;
    for ch in value.chars() {
        match ch {
            '\\' => backslashes += 1,
            '"' => {
                quoted.extend(std::iter::repeat('\\').take(backslashes * 2 + 1));
                quoted.push('"');
                backslashes = 0;
            }
            _ => {
                quoted.extend(std::iter::repeat('\\').take(backslashes));
                backslashes = 0;
                quoted.push(ch);
            }
        }
    }
    quoted.extend(std::iter::repeat('\\').take(backslashes * 2));
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use super::win32_error_code;

    #[test]
    fn strips_hresult_wrapper_from_win32_codes() {
        let err = windows::core::Error::from_hresult(windows::core::HRESULT(0x8007_0424u32 as i32));
        assert_eq!(win32_error_code(&err), 0x0424);
    }
}
