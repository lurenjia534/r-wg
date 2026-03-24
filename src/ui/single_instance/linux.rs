use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use super::protocol::{read_json_line, write_json_line, UiInstanceReply, UiInstanceRequest};
use super::{ActivationState, PlatformStartup};

const SOCKET_DIR_NAME: &str = "r-wg";
const SOCKET_FILE_NAME: &str = "ui.sock";
const LOCK_FILE_NAME: &str = "ui.lock";
const ACCEPT_RETRY_INTERVAL: Duration = Duration::from_millis(200);
const ACTIVATE_RETRY_INTERVAL: Duration = Duration::from_millis(100);
const ACTIVATE_RECOVERY_ATTEMPTS: usize = 10;

pub(super) struct PrimaryGuard {
    runtime_dir: PathBuf,
    socket_path: PathBuf,
    lock_path: PathBuf,
    _lock_file: File,
}

struct ControlPaths {
    runtime_dir: PathBuf,
    socket_path: PathBuf,
    lock_path: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActivateErrorKind {
    Retryable,
    Rebind,
    Fatal,
}

#[derive(Debug)]
struct ActivateError {
    kind: ActivateErrorKind,
    message: String,
}

pub(super) fn startup(activation: Arc<ActivationState>) -> Result<PlatformStartup, String> {
    let paths = control_paths()?;
    ensure_runtime_dir(&paths.runtime_dir)?;

    if let Some(lock_file) = try_acquire_primary_lock(&paths.lock_path)? {
        return activate_existing_or_bind_primary(&paths, lock_file, activation);
    }

    recover_existing_primary(&paths, activation)
}

impl Drop for PrimaryGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.socket_path);
        let _ = fs::remove_file(&self.lock_path);
        let _ = fs::remove_dir(&self.runtime_dir);
    }
}

fn control_paths() -> Result<ControlPaths, String> {
    let runtime_dir = if let Some(runtime_dir) = dirs::runtime_dir() {
        runtime_dir.join(SOCKET_DIR_NAME)
    } else {
        let uid = unsafe { libc::getuid() };
        std::env::temp_dir().join(format!("r-wg-{uid}"))
    };
    Ok(ControlPaths {
        socket_path: runtime_dir.join(SOCKET_FILE_NAME),
        lock_path: runtime_dir.join(LOCK_FILE_NAME),
        runtime_dir,
    })
}

fn ensure_runtime_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path)
        .map_err(|err| format!("failed to create UI runtime dir {}: {err}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|err| format!("failed to chmod UI runtime dir {}: {err}", path.display()))
}

fn try_acquire_primary_lock(lock_path: &Path) -> Result<Option<File>, String> {
    let lock_file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(lock_path)
        .map_err(|err| {
            format!(
                "failed to open UI single-instance lock {}: {err}",
                lock_path.display()
            )
        })?;

    let result = unsafe { libc::flock(lock_file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result == 0 {
        return Ok(Some(lock_file));
    }

    let err = io::Error::last_os_error();
    if matches!(err.raw_os_error(), Some(libc::EWOULDBLOCK)) {
        Ok(None)
    } else {
        Err(format!(
            "failed to acquire UI single-instance lock {}: {err}",
            lock_path.display()
        ))
    }
}

fn activate_existing_or_bind_primary(
    paths: &ControlPaths,
    lock_file: File,
    activation: Arc<ActivationState>,
) -> Result<PlatformStartup, String> {
    let mut last_error = match send_activate(&paths.socket_path) {
        Ok(()) => return Ok(PlatformStartup::Secondary),
        Err(err) => err,
    };

    for _ in 0..ACTIVATE_RECOVERY_ATTEMPTS {
        match last_error.kind {
            ActivateErrorKind::Rebind => {
                return bind_primary(paths, lock_file, activation.clone());
            }
            ActivateErrorKind::Retryable => {
                thread::sleep(ACTIVATE_RETRY_INTERVAL);
                match send_activate(&paths.socket_path) {
                    Ok(()) => return Ok(PlatformStartup::Secondary),
                    Err(err) => last_error = err,
                }
            }
            ActivateErrorKind::Fatal => return Err(last_error.message),
        }
    }

    if last_error.kind == ActivateErrorKind::Rebind {
        return bind_primary(paths, lock_file, activation);
    }

    Err(format!(
        "existing UI instance detected, but activation failed via {}: {}",
        paths.socket_path.display(),
        last_error.message
    ))
}

fn recover_existing_primary(
    paths: &ControlPaths,
    activation: Arc<ActivationState>,
) -> Result<PlatformStartup, String> {
    let mut last_error = match send_activate(&paths.socket_path) {
        Ok(()) => return Ok(PlatformStartup::Secondary),
        Err(err) => err,
    };

    for _ in 0..ACTIVATE_RECOVERY_ATTEMPTS {
        match last_error.kind {
            ActivateErrorKind::Retryable | ActivateErrorKind::Rebind => {
                thread::sleep(ACTIVATE_RETRY_INTERVAL);
                if let Some(lock_file) = try_acquire_primary_lock(&paths.lock_path)? {
                    return activate_existing_or_bind_primary(paths, lock_file, activation.clone());
                }
                match send_activate(&paths.socket_path) {
                    Ok(()) => return Ok(PlatformStartup::Secondary),
                    Err(err) => last_error = err,
                }
            }
            ActivateErrorKind::Fatal => return Err(last_error.message),
        }
    }

    Err(format!(
        "existing UI instance detected, but activation failed via {}: {}",
        paths.socket_path.display(),
        last_error.message
    ))
}

fn bind_primary(
    paths: &ControlPaths,
    lock_file: File,
    activation: Arc<ActivationState>,
) -> Result<PlatformStartup, String> {
    prepare_socket_path_for_bind(&paths.socket_path)?;
    let listener = UnixListener::bind(&paths.socket_path).map_err(|err| {
        format!(
            "failed to bind UI single-instance socket {}: {err}",
            paths.socket_path.display()
        )
    })?;
    fs::set_permissions(&paths.socket_path, fs::Permissions::from_mode(0o600)).map_err(|err| {
        format!(
            "failed to chmod UI single-instance socket {}: {err}",
            paths.socket_path.display()
        )
    })?;
    spawn_listener(listener, activation, paths.socket_path.clone()).map_err(|err| {
        format!(
            "failed to spawn UI single-instance listener for {}: {err}",
            paths.socket_path.display()
        )
    })?;
    Ok(PlatformStartup::Primary(PrimaryGuard {
        runtime_dir: paths.runtime_dir.clone(),
        socket_path: paths.socket_path.clone(),
        lock_path: paths.lock_path.clone(),
        _lock_file: lock_file,
    }))
}

fn prepare_socket_path_for_bind(socket_path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(socket_path) {
        Ok(metadata) => {
            if metadata.file_type().is_socket() {
                fs::remove_file(socket_path).map_err(|err| {
                    format!(
                        "failed to remove stale UI socket {}: {err}",
                        socket_path.display()
                    )
                })?;
                Ok(())
            } else {
                Err(format!(
                    "refusing to replace non-socket path at {}",
                    socket_path.display()
                ))
            }
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(format!(
            "failed to inspect UI socket path {} before bind: {err}",
            socket_path.display()
        )),
    }
}

fn spawn_listener(
    listener: UnixListener,
    activation: Arc<ActivationState>,
    socket_path: PathBuf,
) -> io::Result<()> {
    let socket_path_for_thread = socket_path.clone();
    let builder = thread::Builder::new().name("ui-single-instance".to_string());
    builder
        .spawn(move || loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    if let Err(err) = handle_client(stream, &activation) {
                        tracing::debug!(
                            "ui single-instance client handling failed for {}: {err}",
                            socket_path_for_thread.display()
                        );
                    }
                }
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(err) => {
                    tracing::warn!(
                        "ui single-instance accept failed for {}: {err}",
                        socket_path_for_thread.display()
                    );
                    thread::sleep(ACCEPT_RETRY_INTERVAL);
                }
            }
        })
        .map(|_| ())
        .map_err(|err| {
            io::Error::new(
                err.kind(),
                format!(
                    "failed to spawn ui single-instance listener for {}: {err}",
                    socket_path.display()
                ),
            )
        })
}

fn handle_client(mut stream: UnixStream, activation: &ActivationState) -> io::Result<()> {
    let request = {
        let mut reader = BufReader::new(&mut stream);
        read_json_line::<UiInstanceRequest>(&mut reader)?
    };
    match request {
        UiInstanceRequest::Activate => {
            activation.notify_activate();
            write_json_line(&mut stream, &UiInstanceReply::Ok)
        }
    }
}

fn send_activate(socket_path: &Path) -> Result<(), ActivateError> {
    let mut stream = UnixStream::connect(socket_path)
        .map_err(|err| activation_io_error("connect to primary UI instance", err))?;
    write_json_line(&mut stream, &UiInstanceRequest::Activate)
        .map_err(|err| activation_io_error("send activate request", err))?;

    let mut reader = BufReader::new(&mut stream);
    match read_json_line::<UiInstanceReply>(&mut reader)
        .map_err(|err| activation_io_error("read activation reply", err))?
    {
        UiInstanceReply::Ok => Ok(()),
        UiInstanceReply::Error { message } => Err(ActivateError {
            kind: ActivateErrorKind::Fatal,
            message,
        }),
    }
}

fn activation_io_error(action: &str, err: io::Error) -> ActivateError {
    let kind = if can_rebind_after_activation_error(&err) {
        ActivateErrorKind::Rebind
    } else if is_retryable_activation_error(&err) {
        ActivateErrorKind::Retryable
    } else {
        ActivateErrorKind::Fatal
    };
    ActivateError {
        kind,
        message: format!("failed to {action}: {err}"),
    }
}

fn can_rebind_after_activation_error(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
    ) || matches!(err.raw_os_error(), Some(libc::ECONNREFUSED | libc::ENOENT))
}

fn is_retryable_activation_error(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::ConnectionReset
            | io::ErrorKind::BrokenPipe
            | io::ErrorKind::UnexpectedEof
            | io::ErrorKind::TimedOut
    ) || matches!(err.raw_os_error(), Some(libc::ECONNRESET | libc::EPIPE))
}
