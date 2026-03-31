use std::fs;
use std::io::{self, BufReader};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Instant;

use super::super::engine::Engine as LocalEngine;
use super::super::ipc::{read_json_line, write_json_line, BackendCommand, BackendReply};
use super::super::ipc_server::dispatch_command;
use super::super::EngineStatus;
use super::auth::{is_peer_allowed, peer_credentials};
use super::fs_ops::{configure_socket_permissions, lookup_group_gid, remove_stale_socket};
use super::install_model::{
    ServiceOptions, SERVICE_IDLE_TIMEOUT, SERVICE_IO_TIMEOUT, SERVICE_POLL_INTERVAL,
};
use super::remote_error;
use super::systemd::inherited_listener;

static SERVICE_TERMINATE_REQUESTED: AtomicBool = AtomicBool::new(false);

pub(super) fn run_service(options: ServiceOptions) -> Result<(), super::super::EngineError> {
    SERVICE_TERMINATE_REQUESTED.store(false, Ordering::Relaxed);
    let socket_gid = match options.socket_group.as_deref() {
        Some(group) => Some(lookup_group_gid(group)?),
        None => None,
    };
    let listener = if let Some(listener) = inherited_listener()? {
        listener
    } else {
        if let Some(parent) = options.socket_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| remote_error(format!("failed to create runtime dir: {err}")))?;
        }
        remove_stale_socket(options.socket_path.as_path())?;
        let listener = UnixListener::bind(options.socket_path.as_path()).map_err(|err| {
            remote_error(format!(
                "failed to bind Linux privileged backend socket {}: {err}",
                options.socket_path.display()
            ))
        })?;
        configure_socket_permissions(options.socket_path.as_path(), socket_gid)?;
        listener
    };
    listener.set_nonblocking(true).map_err(|err| {
        remote_error(format!(
            "failed to configure Linux privileged backend socket: {err}"
        ))
    })?;

    crate::platform::linux::attempt_startup_repair()
        .map_err(|err| remote_error(format!("startup repair failed: {err}")))?;

    let engine = LocalEngine::new();
    let mut last_activity = Instant::now();

    loop {
        if SERVICE_TERMINATE_REQUESTED.load(Ordering::Relaxed) {
            return graceful_stop_for_shutdown(&engine);
        }
        match listener.accept() {
            Ok((stream, _)) => {
                last_activity = Instant::now();
                let engine = engine.clone();
                if let Err(err) = thread::Builder::new()
                    .name("wg-linux-service-client".to_string())
                    .spawn(move || {
                        if let Err(err) =
                            handle_service_client(stream, &engine, options.allowed_uid, socket_gid)
                        {
                            tracing::debug!("linux service client handling failed: {err}");
                        }
                    })
                {
                    tracing::warn!("failed to spawn linux service client worker: {err}");
                }
            }
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                let running = matches!(engine.status(), Ok(EngineStatus::Running));
                if !running && last_activity.elapsed() >= SERVICE_IDLE_TIMEOUT {
                    break Ok(());
                }
                thread::sleep(SERVICE_POLL_INTERVAL);
            }
            Err(err) => {
                tracing::warn!("linux service accept failed: {err}");
                thread::sleep(SERVICE_POLL_INTERVAL);
            }
        }
    }
}

pub(super) fn install_signal_handlers() {
    unsafe {
        libc::signal(
            libc::SIGTERM,
            signal_terminate_handler as *const () as libc::sighandler_t,
        );
        libc::signal(
            libc::SIGINT,
            signal_terminate_handler as *const () as libc::sighandler_t,
        );
    }
}

extern "C" fn signal_terminate_handler(_: libc::c_int) {
    SERVICE_TERMINATE_REQUESTED.store(true, Ordering::Relaxed);
}

fn graceful_stop_for_shutdown(engine: &LocalEngine) -> Result<(), super::super::EngineError> {
    match engine.stop() {
        Ok(())
        | Err(super::super::EngineError::NotRunning)
        | Err(super::super::EngineError::ChannelClosed) => Ok(()),
        Err(err) => Err(err),
    }
}

fn handle_service_client(
    mut stream: UnixStream,
    engine: &LocalEngine,
    allowed_uid: Option<u32>,
    allowed_gid: Option<u32>,
) -> io::Result<()> {
    let _ = stream.set_read_timeout(Some(SERVICE_IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(SERVICE_IO_TIMEOUT));
    let reply = match peer_credentials(&stream) {
        Ok(creds) if is_peer_allowed(creds, allowed_uid, allowed_gid) => {
            handle_command(&mut stream, engine)?
        }
        Ok(_) => BackendReply::Error {
            kind: super::super::ipc::BackendErrorKind::AccessDenied,
            message: "peer is not allowed to access Linux privileged backend".to_string(),
        },
        Err(err) => BackendReply::Error {
            kind: super::super::ipc::BackendErrorKind::AccessDenied,
            message: format!("failed to inspect peer credentials: {err}"),
        },
    };

    write_json_line(&mut stream, &reply)
}

fn handle_command(stream: &mut UnixStream, engine: &LocalEngine) -> io::Result<BackendReply> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let command: BackendCommand = read_json_line(&mut reader)?;
    Ok(dispatch_command(engine, command))
}
