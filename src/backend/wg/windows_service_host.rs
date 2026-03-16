use std::ffi::c_void;
use std::io::{self, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::NO_ERROR;
use windows::Win32::System::Services::{
    RegisterServiceCtrlHandlerExW, SetServiceStatus, StartServiceCtrlDispatcherW,
    SERVICE_ACCEPT_PRESHUTDOWN, SERVICE_ACCEPT_SHUTDOWN, SERVICE_ACCEPT_STOP,
    SERVICE_CONTROL_PRESHUTDOWN, SERVICE_CONTROL_SHUTDOWN, SERVICE_CONTROL_STOP,
    SERVICE_RUNNING, SERVICE_START_PENDING, SERVICE_STATUS, SERVICE_STATUS_CURRENT_STATE,
    SERVICE_STATUS_HANDLE, SERVICE_STOP_PENDING, SERVICE_STOPPED, SERVICE_TABLE_ENTRYW,
    SERVICE_WIN32_OWN_PROCESS,
};

use super::engine::Engine as LocalEngine;
use super::ipc::{error_reply, read_json_line, unit_reply, write_json_line, BackendCommand, BackendReply};
use super::windows_service::SERVICE_NAME;
use super::{EngineError, EngineStatus};
use crate::backend::wg::ipc::IPC_PROTOCOL_VERSION;

const START_WAIT_HINT_MS: u32 = 30_000;
const STOP_WAIT_HINT_MS: u32 = 120_000;

static STOP_REQUESTED: AtomicBool = AtomicBool::new(false);
static SERVICE_STATUS_HANDLE_RAW: OnceLock<usize> = OnceLock::new();

pub fn run_service_dispatcher() -> Result<(), EngineError> {
    let mut service_name = encode_wide(SERVICE_NAME);
    let mut table = [
        SERVICE_TABLE_ENTRYW {
            lpServiceName: PWSTR(service_name.as_mut_ptr()),
            lpServiceProc: Some(service_main),
        },
        SERVICE_TABLE_ENTRYW::default(),
    ];

    unsafe {
        StartServiceCtrlDispatcherW(table.as_mut_ptr())
            .map_err(|err| EngineError::Remote(format!("failed to start service dispatcher: {err}")))
    }
}

extern "system" fn service_main(_argc: u32, _argv: *mut PWSTR) {
    if let Err(err) = service_main_inner() {
        tracing::error!("windows service failed: {err}");
        if let Some(raw) = SERVICE_STATUS_HANDLE_RAW.get().copied() {
            let _ = set_service_status(
                service_status_handle(raw),
                SERVICE_STOPPED,
                0,
                1,
                0,
            );
        }
    }
}

fn service_main_inner() -> Result<(), EngineError> {
    STOP_REQUESTED.store(false, Ordering::SeqCst);

    let service_name = encode_wide(SERVICE_NAME);
    let status_handle = unsafe {
        RegisterServiceCtrlHandlerExW(
            PCWSTR(service_name.as_ptr()),
            Some(service_control_handler),
            None,
        )
        .map_err(|err| EngineError::Remote(format!("failed to register service handler: {err}")))?
    };

    let _ = SERVICE_STATUS_HANDLE_RAW.set(status_handle.0 as usize);

    set_service_status(status_handle, SERVICE_START_PENDING, 0, 0, START_WAIT_HINT_MS)?;

    crate::platform::windows::attempt_startup_repair()
        .map_err(|err| EngineError::Remote(format!("startup repair failed: {err}")))?;

    let engine = LocalEngine::new();

    set_service_status(
        status_handle,
        SERVICE_RUNNING,
        SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN | SERVICE_ACCEPT_PRESHUTDOWN,
        0,
        0,
    )?;

    let run_result = run_pipe_loop(&engine);

    set_service_status(status_handle, SERVICE_STOP_PENDING, 0, 0, STOP_WAIT_HINT_MS)?;

    if matches!(engine.status(), Ok(EngineStatus::Running)) {
        if let Err(err) = engine.stop() {
            tracing::warn!("failed to stop engine during service shutdown: {err}");
        }
    }

    let stop_code = if run_result.is_ok() { 0 } else { 1 };
    let stopped_result = set_service_status(status_handle, SERVICE_STOPPED, 0, stop_code, 0);

    run_result.and(stopped_result)
}

fn run_pipe_loop(engine: &LocalEngine) -> Result<(), EngineError> {
    loop {
        if STOP_REQUESTED.load(Ordering::SeqCst) {
            return Ok(());
        }

        let instance = super::windows_pipe::ServerPipeInstance::create()
            .map_err(|err| EngineError::Remote(format!("failed to create named pipe: {err}")))?;
        let stream = match instance.connect() {
            Ok(stream) => stream,
            Err(err) => {
                if STOP_REQUESTED.load(Ordering::SeqCst) {
                    return Ok(());
                }
                return Err(EngineError::Remote(format!(
                    "failed to accept named pipe client: {err}"
                )));
            }
        };

        if STOP_REQUESTED.load(Ordering::SeqCst) {
            return Ok(());
        }

        if let Err(err) = handle_pipe_client(stream, engine) {
            tracing::debug!("named pipe client handling failed: {err}");
        }
    }
}

fn handle_pipe_client(
    mut stream: super::windows_pipe::PipeStream,
    engine: &LocalEngine,
) -> io::Result<()> {
    let command: BackendCommand = {
        let mut reader = BufReader::new(&mut stream);
        read_json_line(&mut reader)?
    };
    let reply = match command {
        BackendCommand::Info => BackendReply::Info {
            protocol_version: IPC_PROTOCOL_VERSION,
        },
        BackendCommand::Ping => BackendReply::Ok,
        BackendCommand::Start { request } => unit_reply(engine.start(request)),
        BackendCommand::Stop => unit_reply(engine.stop()),
        BackendCommand::Status => match engine.status() {
            Ok(status) => BackendReply::Status { status },
            Err(err) => error_reply(err),
        },
        BackendCommand::Stats => match engine.stats() {
            Ok(stats) => BackendReply::Stats { stats },
            Err(err) => error_reply(err),
        },
    };
    write_json_line(&mut stream, &reply)
}

extern "system" fn service_control_handler(
    control: u32,
    _event_type: u32,
    _event_data: *mut c_void,
    _context: *mut c_void,
) -> u32 {
    match control {
        SERVICE_CONTROL_STOP | SERVICE_CONTROL_SHUTDOWN | SERVICE_CONTROL_PRESHUTDOWN => {
            STOP_REQUESTED.store(true, Ordering::SeqCst);
            if let Some(raw) = SERVICE_STATUS_HANDLE_RAW.get().copied() {
                let _ = set_service_status(
                    service_status_handle(raw),
                    SERVICE_STOP_PENDING,
                    0,
                    0,
                    STOP_WAIT_HINT_MS,
                );
            }
            super::windows_pipe::poke_server();
        }
        _ => {}
    }

    NO_ERROR.0
}

fn set_service_status(
    handle: SERVICE_STATUS_HANDLE,
    current_state: SERVICE_STATUS_CURRENT_STATE,
    controls_accepted: u32,
    win32_exit_code: u32,
    wait_hint: u32,
) -> Result<(), EngineError> {
    let checkpoint = if current_state == SERVICE_START_PENDING || current_state == SERVICE_STOP_PENDING
    {
        1
    } else {
        0
    };
    let mut status = SERVICE_STATUS {
        dwServiceType: SERVICE_WIN32_OWN_PROCESS,
        dwCurrentState: current_state,
        dwControlsAccepted: controls_accepted,
        dwWin32ExitCode: win32_exit_code,
        dwServiceSpecificExitCode: 0,
        dwCheckPoint: checkpoint,
        dwWaitHint: wait_hint,
    };
    unsafe {
        SetServiceStatus(handle, &mut status)
            .map_err(|err| EngineError::Remote(format!("failed to report service status: {err}")))
    }
}

fn service_status_handle(raw: usize) -> SERVICE_STATUS_HANDLE {
    SERVICE_STATUS_HANDLE(raw as *mut c_void)
}

fn encode_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
