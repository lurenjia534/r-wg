use std::ffi::c_void;
use std::io::{self, Read, Write};
use std::mem::MaybeUninit;
use std::thread;
use std::time::{Duration, Instant};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, LocalFree, ERROR_PIPE_CONNECTED, GENERIC_READ, GENERIC_WRITE,
    HLOCAL, HANDLE, INVALID_HANDLE_VALUE,
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

pub const PIPE_NAME: &str = r"\\.\pipe\r-wg-control";
const PIPE_BUFFER_SIZE: u32 = 64 * 1024;
const PIPE_WAIT_TIMEOUT_MS: u32 = 5_000;
const PIPE_CONNECT_RETRY_INTERVAL: Duration = Duration::from_millis(50);
const PIPE_SDDL: &str = "O:SYG:SYD:(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)";

pub struct PipeStream {
    handle: HANDLE,
}

// PipeStream 独占持有一个已连接的 named-pipe HANDLE。
// Windows HANDLE 可以在线程间转移所有权，这里不实现 Sync，只允许 move 到 worker 线程。
unsafe impl Send for PipeStream {}

impl PipeStream {
    pub fn connect() -> io::Result<Self> {
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

    pub fn from_handle(handle: HANDLE) -> Self {
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

pub struct ServerPipeInstance {
    handle: HANDLE,
}

impl ServerPipeInstance {
    pub fn create() -> io::Result<Self> {
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

    pub fn connect(self) -> io::Result<PipeStream> {
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

pub fn poke_server() {
    let _ = PipeStream::connect();
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
