use std::env;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;

use super::super::EngineError;
use super::client::RemoteEngine;
use super::ensure_root;
use super::fs_ops::{
    cleanup_runtime_socket_dir, ensure_group_exists, install_binary, install_desktop_integration,
    remove_desktop_integration, write_service_unit,
};
use super::install_model::{
    InstallOptions, ManageCommand, PrivilegedServiceAction, RemoveOptions, DEFAULT_SOCKET_PATH,
    SERVICE_SUBCOMMAND, SERVICE_UNIT_NAME, SOCKET_UNIT_NAME, STARTUP_REPAIR_UNIT_NAME,
};
use super::is_running_as_root;
use super::remote_error;
use super::render::{
    load_existing_install_auth_mode, render_service_unit, render_socket_unit,
    render_startup_repair_unit,
};
use super::systemd::{run_command, systemd_unit_is_active};

pub fn manage_privileged_service(action: PrivilegedServiceAction) -> Result<(), EngineError> {
    let current_exe = env::current_exe()
        .map_err(|err| remote_error(format!("failed to locate current exe: {err}")))?;

    let mut args = vec![
        OsString::from(SERVICE_SUBCOMMAND),
        OsString::from(action.as_cli()),
    ];
    if !matches!(action, PrivilegedServiceAction::Remove) {
        let current_uid = unsafe { libc::getuid() };
        args.push(OsString::from("--source"));
        args.push(current_exe.as_os_str().to_os_string());
        args.push(OsString::from("--socket-group"));
        args.push(OsString::from("none"));
        args.push(OsString::from("--allowed-uid"));
        args.push(OsString::from(current_uid.to_string()));
        let user = current_username(current_uid).ok_or_else(|| {
            remote_error(
                "failed to resolve current username for privileged backend socket ownership"
                    .to_string(),
            )
        })?;
        args.push(OsString::from("--socket-user"));
        args.push(OsString::from(user));
    }

    let output = if is_running_as_root() {
        Command::new(&current_exe).args(&args).output()
    } else {
        Command::new("pkexec")
            .arg(&current_exe)
            .args(&args)
            .output()
    }
    .map_err(|err| {
        if err.kind() == io::ErrorKind::NotFound && !is_running_as_root() {
            remote_error(
                "pkexec not found. Install polkit or run the service command as root.".to_string(),
            )
        } else {
            remote_error(format!(
                "failed to launch privileged backend manager: {err}"
            ))
        }
    })?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let message = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        format!("privileged backend manager exited with {}", output.status)
    };
    Err(remote_error(message))
}

pub(super) fn run_manage_command(command: ManageCommand) -> Result<(), EngineError> {
    ensure_root()?;
    match command {
        ManageCommand::Install(options) => install_or_repair(options, false),
        ManageCommand::Repair(options) => install_or_repair(options, true),
        ManageCommand::Remove(options) => remove_installation(options),
        ManageCommand::StartupRepair => crate::platform::linux::attempt_startup_repair()
            .map_err(|err| remote_error(format!("startup repair failed: {err}"))),
    }
}

fn install_or_repair(mut options: InstallOptions, repairing: bool) -> Result<(), EngineError> {
    if !options.source_path.is_file() {
        return Err(remote_error(format!(
            "source binary not found: {}",
            options.source_path.display()
        )));
    }

    if repairing {
        if let Some(existing) =
            load_existing_install_auth_mode(&options.unit_path, &options.socket_unit_path)?
        {
            options.socket_group = existing.socket_group;
            options.socket_user = existing.socket_user;
            options.allowed_uid = existing.allowed_uid;
        }
    }

    if let Some(group) = options.socket_group.as_deref() {
        ensure_group_exists(group)?;
    }

    install_binary(&options.source_path, &options.binary_path)?;
    install_desktop_integration(&options.binary_path)?;
    write_service_unit(
        &options.unit_path,
        render_service_unit(
            &options.binary_path,
            options.socket_group.as_deref(),
            options.allowed_uid,
        ),
    )?;
    write_service_unit(
        &options.socket_unit_path,
        render_socket_unit(
            options.socket_user.as_deref(),
            options.socket_group.as_deref(),
        ),
    )?;
    write_service_unit(
        &options.startup_repair_unit_path,
        render_startup_repair_unit(&options.binary_path),
    )?;

    run_command("systemctl", ["daemon-reload"])?;
    run_command("systemctl", ["enable", SOCKET_UNIT_NAME])?;
    run_command("systemctl", ["enable", STARTUP_REPAIR_UNIT_NAME])?;
    if repairing {
        graceful_stop_active_backend()?;
        cleanup_runtime_socket_dir(Path::new(DEFAULT_SOCKET_PATH))?;
        run_command("systemctl", ["restart", SOCKET_UNIT_NAME])?;
    } else {
        cleanup_runtime_socket_dir(Path::new(DEFAULT_SOCKET_PATH))?;
        run_command("systemctl", ["start", SOCKET_UNIT_NAME])?;
    }

    Ok(())
}

fn remove_installation(options: RemoveOptions) -> Result<(), EngineError> {
    graceful_stop_active_backend()?;
    let _ = run_command("systemctl", ["disable", STARTUP_REPAIR_UNIT_NAME]);
    let _ = run_command("systemctl", ["disable", "--now", SOCKET_UNIT_NAME]);
    let _ = run_command("systemctl", ["stop", SERVICE_UNIT_NAME]);
    let _ = cleanup_runtime_socket_dir(Path::new(DEFAULT_SOCKET_PATH));
    let _ = remove_desktop_integration();

    if options.unit_path.exists() {
        fs::remove_file(&options.unit_path).map_err(|err| {
            remote_error(format!(
                "failed to remove service unit {}: {err}",
                options.unit_path.display()
            ))
        })?;
    }
    if options.socket_unit_path.exists() {
        fs::remove_file(&options.socket_unit_path).map_err(|err| {
            remote_error(format!(
                "failed to remove socket unit {}: {err}",
                options.socket_unit_path.display()
            ))
        })?;
    }
    if options.startup_repair_unit_path.exists() {
        fs::remove_file(&options.startup_repair_unit_path).map_err(|err| {
            remote_error(format!(
                "failed to remove startup repair unit {}: {err}",
                options.startup_repair_unit_path.display()
            ))
        })?;
    }

    let _ = run_command("systemctl", ["daemon-reload"]);

    if options.binary_path.exists() {
        fs::remove_file(&options.binary_path).map_err(|err| {
            remote_error(format!(
                "failed to remove installed binary {}: {err}",
                options.binary_path.display()
            ))
        })?;
    }

    if let Some(parent) = options.binary_path.parent() {
        match fs::remove_dir(parent) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) if err.kind() == io::ErrorKind::DirectoryNotEmpty => {}
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

fn graceful_stop_active_backend() -> Result<(), EngineError> {
    if !systemd_unit_is_active(SERVICE_UNIT_NAME) {
        return Ok(());
    }
    match RemoteEngine::new().stop() {
        Ok(()) | Err(EngineError::NotRunning) => Ok(()),
        Err(err) => Err(err),
    }
}

fn current_username(uid: u32) -> Option<String> {
    let mut pwd = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut result = std::ptr::null_mut();
    let mut buf = vec![0u8; 1024];

    loop {
        let rc = unsafe {
            libc::getpwuid_r(
                uid,
                pwd.as_mut_ptr(),
                buf.as_mut_ptr().cast(),
                buf.len(),
                &mut result,
            )
        };
        if rc == 0 {
            if result.is_null() {
                return None;
            }
            let pwd = unsafe { pwd.assume_init() };
            let name = unsafe { std::ffi::CStr::from_ptr(pwd.pw_name) };
            return Some(name.to_string_lossy().into_owned());
        }
        if rc == libc::ERANGE {
            buf.resize(buf.len() * 2, 0);
            continue;
        }
        return None;
    }
}
