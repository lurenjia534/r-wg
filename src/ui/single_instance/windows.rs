use std::ffi::c_void;
use std::io::{self, BufReader, Read, Write};
use std::mem::MaybeUninit;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, LocalFree, ERROR_ALREADY_EXISTS, ERROR_PIPE_CONNECTED, GENERIC_READ,
    GENERIC_WRITE, HANDLE, HLOCAL, INVALID_HANDLE_VALUE,
};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::SECURITY_ATTRIBUTES;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FlushFileBuffers, ReadFile, WriteFile, FILE_ATTRIBUTE_NORMAL,
    FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_MODE, OPEN_EXISTING, PIPE_ACCESS_DUPLEX,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, WaitNamedPipeW, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

use super::protocol::{read_json_line, write_json_line, UiInstanceReply, UiInstanceRequest};
use super::{ActivationState, PlatformStartup};

const MUTEX_NAME: &str = r"Local\r-wg-ui-single-instance";
const PIPE_NAME: &str = r"\\.\pipe\r-wg-ui-control";
const PIPE_BUFFER_SIZE: u32 = 4096;
const PIPE_WAIT_TIMEOUT_MS: u32 = 5_000;
const PIPE_CONNECT_RETRY_INTERVAL: Duration = Duration::from_millis(50);
const PIPE_SDDL: &str = "O:SYG:SYD:(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)";

pub(super) struct PrimaryGuard {
    mutex: HANDLE,
}

pub(super) fn startup(activation: Arc<ActivationState>) -> Result<PlatformStartup, String> {
    let name = encode_wide(MUTEX_NAME);
    let mutex = unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr())) }
        .map_err(|err| format!("failed to create UI single-instance mutex: {err}"))?;
    let already_exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;

    if already_exists {
        unsafe {
            let _ = CloseHandle(mutex);
        }
        send_activate()?;
        return Ok(PlatformStartup::Secondary);
    }

    if let Err(err) = spawn_listener(activation) {
        unsafe {
            let _ = CloseHandle(mutex);
        }
        return Err(format!(
            "failed to start UI single-instance listener: {err}"
        ));
    }

    Ok(PlatformStartup::Primary(PrimaryGuard { mutex }))
}

pub(super) fn show_bootstrap_error(message: &str) {
    let title = encode_wide("r-wg startup failed");
    let body = encode_wide(message);
    unsafe {
        let _ = MessageBoxW(
            None,
            PCWSTR(body.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONERROR,
        );
    }
}

impl Drop for PrimaryGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.mutex);
        }
    }
}

fn spawn_listener(activation: Arc<ActivationState>) -> io::Result<()> {
    let builder = thread::Builder::new().name("ui-single-instance".to_string());
    builder
        .spawn(move || loop {
            let instance = match ServerPipeInstance::create() {
                Ok(instance) => instance,
                Err(err) => {
                    tracing::warn!("failed to create UI control pipe: {err}");
                    thread::sleep(PIPE_CONNECT_RETRY_INTERVAL);
                    continue;
                }
            };

            match instance.connect() {
                Ok(mut stream) => {
                    if let Err(err) = handle_client(&mut stream, &activation) {
                        tracing::debug!("ui single-instance pipe handling failed: {err}");
                    }
                }
                Err(err) => {
                    tracing::debug!("ui single-instance pipe connect failed: {err}");
                    thread::sleep(PIPE_CONNECT_RETRY_INTERVAL);
                }
            }
        })
        .map(|_| ())
}

fn handle_client(stream: &mut PipeStream, activation: &ActivationState) -> io::Result<()> {
    let request = {
        let mut reader = BufReader::new(&mut *stream);
        read_json_line::<UiInstanceRequest>(&mut reader)?
    };
    match request {
        UiInstanceRequest::Activate => {
            activation.notify_activate();
            write_json_line(stream, &UiInstanceReply::Ok)
        }
    }
}

fn send_activate() -> Result<(), String> {
    let mut stream = PipeStream::connect()
        .map_err(|err| format!("failed to connect to primary UI instance: {err}"))?;
    write_json_line(&mut stream, &UiInstanceRequest::Activate)
        .map_err(|err| format!("failed to send UI activation request: {err}"))?;
    let mut reader = BufReader::new(&mut stream);
    match read_json_line::<UiInstanceReply>(&mut reader)
        .map_err(|err| format!("failed to read UI activation reply: {err}"))?
    {
        UiInstanceReply::Ok => Ok(()),
        UiInstanceReply::Error { message } => Err(message),
    }
}

struct PipeStream {
    handle: HANDLE,
}

unsafe impl Send for PipeStream {}

impl PipeStream {
    fn connect() -> io::Result<Self> {
        let name = encode_wide(PIPE_NAME);
        let deadline = Instant::now() + Duration::from_millis(PIPE_WAIT_TIMEOUT_MS as u64);
        loop {
            unsafe {
                if WaitNamedPipeW(PCWSTR(name.as_ptr()), PIPE_WAIT_TIMEOUT_MS).as_bool() {
                    let handle = CreateFileW(
                        PCWSTR(name.as_ptr()),
                        GENERIC_READ.0 | GENERIC_WRITE.0,
                        FILE_SHARE_MODE(0),
                        None,
                        OPEN_EXISTING,
                        FILE_ATTRIBUTE_NORMAL,
                        None,
                    )
                    .map_err(io_error_from_win32)?;
                    return Ok(Self { handle });
                }
            }

            let err = last_os_error();
            if should_retry_connect(&err) && Instant::now() < deadline {
                thread::sleep(PIPE_CONNECT_RETRY_INTERVAL);
                continue;
            }
            return Err(err);
        }
    }

    fn from_handle(handle: HANDLE) -> Self {
        Self { handle }
    }
}

impl Read for PipeStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        let mut read = 0u32;
        unsafe {
            ReadFile(self.handle, Some(buf), Some(&mut read), None).map_err(io_error_from_win32)?;
        }
        Ok(read as usize)
    }
}

impl Write for PipeStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        let mut written = 0u32;
        unsafe {
            WriteFile(self.handle, Some(buf), Some(&mut written), None)
                .map_err(io_error_from_win32)?;
        }
        Ok(written as usize)
    }

    fn flush(&mut self) -> io::Result<()> {
        unsafe { FlushFileBuffers(self.handle).map_err(io_error_from_win32) }
    }
}

impl Drop for PipeStream {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

struct ServerPipeInstance {
    handle: HANDLE,
}

impl ServerPipeInstance {
    fn create() -> io::Result<Self> {
        let security = PipeSecurity::new()?;
        let name = encode_wide(PIPE_NAME);
        let handle = unsafe {
            CreateNamedPipeW(
                PCWSTR(name.as_ptr()),
                FILE_FLAGS_AND_ATTRIBUTES(PIPE_ACCESS_DUPLEX.0),
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                PIPE_BUFFER_SIZE,
                PIPE_BUFFER_SIZE,
                0,
                Some(security.attributes()),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(last_os_error());
        }
        Ok(Self { handle })
    }

    fn connect(self) -> io::Result<PipeStream> {
        let handle = self.handle;
        std::mem::forget(self);
        let connected = unsafe { ConnectNamedPipe(handle, None) };
        match connected {
            Ok(()) => Ok(PipeStream::from_handle(handle)),
            Err(err) => {
                let win_err = err.code().0 as u32;
                if win_err == ERROR_PIPE_CONNECTED.0 {
                    Ok(PipeStream::from_handle(handle))
                } else {
                    unsafe {
                        let _ = CloseHandle(handle);
                    }
                    Err(io_error_from_win32(err))
                }
            }
        }
    }
}

impl Drop for ServerPipeInstance {
    fn drop(&mut self) {
        unsafe {
            let _ = DisconnectNamedPipe(self.handle);
            let _ = CloseHandle(self.handle);
        }
    }
}

struct PipeSecurity {
    attributes: SECURITY_ATTRIBUTES,
    descriptor: *mut c_void,
}

impl PipeSecurity {
    fn new() -> io::Result<Self> {
        let mut descriptor = MaybeUninit::<*mut c_void>::zeroed();
        let sddl = encode_wide(PIPE_SDDL);
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(sddl.as_ptr()),
                SDDL_REVISION_1 as u32,
                descriptor.as_mut_ptr().cast(),
                None,
            )
            .map_err(io_error_from_win32)?;
            let descriptor = descriptor.assume_init();
            Ok(Self {
                attributes: SECURITY_ATTRIBUTES {
                    nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                    lpSecurityDescriptor: descriptor,
                    bInheritHandle: false.into(),
                },
                descriptor,
            })
        }
    }

    fn attributes(&self) -> &SECURITY_ATTRIBUTES {
        &self.attributes
    }
}

impl Drop for PipeSecurity {
    fn drop(&mut self) {
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.descriptor)));
        }
    }
}

fn encode_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn io_error_from_win32(err: windows::core::Error) -> io::Error {
    let code = win32_error_code(&err);
    io::Error::from_raw_os_error(code as i32)
}

fn last_os_error() -> io::Error {
    let code = unsafe { GetLastError() };
    io::Error::from_raw_os_error(code.0 as i32)
}

fn should_retry_connect(err: &io::Error) -> bool {
    matches!(err.raw_os_error(), Some(2 | 231))
}

fn win32_error_code(err: &windows::core::Error) -> u32 {
    let code = err.code().0 as u32;
    if (code & 0xFFFF_0000) == 0x8007_0000 {
        code & 0xFFFF
    } else {
        code
    }
}
