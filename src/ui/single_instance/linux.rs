use std::fs;
use std::io::{self, BufReader};
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
const ACCEPT_RETRY_INTERVAL: Duration = Duration::from_millis(200);

pub(super) struct PrimaryGuard {
    runtime_dir: PathBuf,
    socket_path: PathBuf,
}

pub(super) fn startup(activation: Arc<ActivationState>) -> Result<PlatformStartup, String> {
    let socket_path = control_socket_path()?;
    let runtime_dir = socket_path
        .parent()
        .ok_or_else(|| {
            format!(
                "invalid UI single-instance socket path: {}",
                socket_path.display()
            )
        })?
        .to_path_buf();

    ensure_runtime_dir(&runtime_dir)?;
    match try_bind_primary(&socket_path, &runtime_dir, activation.clone()) {
        Ok(guard) => Ok(PlatformStartup::Primary(guard)),
        Err(err) if err.kind() == io::ErrorKind::AddrInUse => match send_activate(&socket_path) {
            Ok(()) => Ok(PlatformStartup::Secondary),
            Err(connect_err) if can_recover_stale_socket(&connect_err) => {
                remove_stale_socket(&socket_path)?;
                let guard = try_bind_primary(&socket_path, &runtime_dir, activation).map_err(
                    |err| {
                        format!(
                            "failed to re-bind UI single-instance socket {} after stale cleanup: {err}",
                            socket_path.display()
                        )
                    },
                )?;
                Ok(PlatformStartup::Primary(guard))
            }
            Err(connect_err) => Err(format!(
                "existing UI instance detected, but activation failed via {}: {connect_err}",
                socket_path.display()
            )),
        },
        Err(err) => Err(format!(
            "failed to bind UI single-instance socket {}: {err}",
            socket_path.display()
        )),
    }
}

impl Drop for PrimaryGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.socket_path);
        let _ = fs::remove_dir(&self.runtime_dir);
    }
}

fn control_socket_path() -> Result<PathBuf, String> {
    if let Some(runtime_dir) = dirs::runtime_dir() {
        return Ok(runtime_dir.join(SOCKET_DIR_NAME).join(SOCKET_FILE_NAME));
    }

    let uid = unsafe { libc::getuid() };
    Ok(std::env::temp_dir()
        .join(format!("r-wg-{uid}"))
        .join(SOCKET_FILE_NAME))
}

fn ensure_runtime_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path)
        .map_err(|err| format!("failed to create UI runtime dir {}: {err}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|err| format!("failed to chmod UI runtime dir {}: {err}", path.display()))
}

fn try_bind_primary(
    socket_path: &Path,
    runtime_dir: &Path,
    activation: Arc<ActivationState>,
) -> io::Result<PrimaryGuard> {
    let listener = UnixListener::bind(socket_path)?;
    fs::set_permissions(socket_path, fs::Permissions::from_mode(0o600))?;
    spawn_listener(listener, activation, socket_path.to_path_buf())?;
    Ok(PrimaryGuard {
        runtime_dir: runtime_dir.to_path_buf(),
        socket_path: socket_path.to_path_buf(),
    })
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

fn send_activate(socket_path: &Path) -> io::Result<()> {
    let mut stream = UnixStream::connect(socket_path)?;
    write_json_line(&mut stream, &UiInstanceRequest::Activate).map_err(|err| {
        io::Error::new(err.kind(), format!("send activate request failed: {err}"))
    })?;

    let mut reader = BufReader::new(&mut stream);
    match read_json_line::<UiInstanceReply>(&mut reader)? {
        UiInstanceReply::Ok => Ok(()),
        UiInstanceReply::Error { message } => Err(io::Error::new(io::ErrorKind::Other, message)),
    }
}

fn can_recover_stale_socket(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
    ) || matches!(err.raw_os_error(), Some(libc::ECONNREFUSED | libc::ENOENT))
}

fn remove_stale_socket(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|err| {
        format!(
            "failed to inspect stale UI socket {}: {err}",
            path.display()
        )
    })?;
    if metadata.file_type().is_socket() {
        fs::remove_file(path)
            .map_err(|err| format!("failed to remove stale UI socket {}: {err}", path.display()))?;
        return Ok(());
    }
    Err(format!(
        "refusing to replace non-socket path at {}",
        path.display()
    ))
}
