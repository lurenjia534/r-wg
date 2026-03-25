use std::env;
use std::ffi::OsString;
use std::path::PathBuf;

use super::super::EngineError;
use super::install_model::{
    control_socket_path, into_string, InstallOptions, LinuxEntryCommand, ManageCommand,
    RemoveOptions, ServiceOptions, DEFAULT_INSTALLED_BINARY, DEFAULT_SOCKET_GROUP,
    DEFAULT_SOCKET_UNIT_PATH, DEFAULT_STARTUP_REPAIR_UNIT_PATH, DEFAULT_UNIT_PATH, SERVICE_ARG,
    SERVICE_SUBCOMMAND,
};
use super::manage::run_manage_command;
use super::remote_error;
use super::server::{install_signal_handlers, run_service};

pub fn maybe_run_service_mode() -> bool {
    crate::log::init();

    let entry = match parse_linux_entry_command(env::args_os()) {
        Ok(entry) => entry,
        Err(err) => exit_linux_entry_error("linux privileged backend command parse failed", err),
    };
    let Some(entry) = entry else {
        return false;
    };

    let result = match entry {
        LinuxEntryCommand::ServiceMode(options) => {
            let _mtu = gotatun::tun::MtuWatcher::new(1500);
            install_signal_handlers();
            run_service(options)
        }
        LinuxEntryCommand::Manage(command) => run_manage_command(command),
    };
    if let Err(err) = result {
        exit_linux_entry_error("linux privileged backend command failed", err);
    }

    true
}

pub(super) fn parse_linux_entry_command(
    args: impl IntoIterator<Item = OsString>,
) -> Result<Option<LinuxEntryCommand>, EngineError> {
    let mut args = args.into_iter();
    let _ = args.next();
    let Some(first) = args.next() else {
        return Ok(None);
    };

    if first == SERVICE_ARG {
        let options = parse_service_mode_args(args)?;
        return Ok(Some(LinuxEntryCommand::ServiceMode(options)));
    }
    if first == SERVICE_SUBCOMMAND {
        let command = parse_manage_command(args)?;
        return Ok(Some(LinuxEntryCommand::Manage(command)));
    }
    Ok(None)
}

fn parse_service_mode_args(
    args: impl IntoIterator<Item = OsString>,
) -> Result<ServiceOptions, EngineError> {
    let mut socket_path = control_socket_path();
    let mut socket_group = None;
    let mut allowed_uid = None;

    let mut pending = None::<String>;
    for arg in args {
        let arg = into_string(arg);
        match pending.take().as_deref() {
            Some("socket") => {
                socket_path = PathBuf::from(arg);
                continue;
            }
            Some("socket_group") => {
                socket_group = if arg.eq_ignore_ascii_case("none") {
                    None
                } else {
                    Some(arg)
                };
                continue;
            }
            Some("allowed_uid") => {
                allowed_uid =
                    Some(arg.parse().map_err(|_| {
                        remote_error(format!("invalid --allowed-uid value: {arg}"))
                    })?);
                continue;
            }
            Some(other) => {
                return Err(remote_error(format!(
                    "unknown pending service arg: {other}"
                )));
            }
            None => {}
        }

        match arg.as_str() {
            "--socket" => pending = Some("socket".to_string()),
            "--socket-group" => pending = Some("socket_group".to_string()),
            "--allowed-uid" => pending = Some("allowed_uid".to_string()),
            other => return Err(remote_error(format!("unknown Linux service arg: {other}"))),
        }
    }

    if let Some(flag) = pending {
        return Err(remote_error(format!(
            "missing value for --{}",
            flag.replace('_', "-")
        )));
    }

    Ok(ServiceOptions {
        socket_path,
        socket_group,
        allowed_uid,
    })
}

fn parse_manage_command(
    args: impl IntoIterator<Item = OsString>,
) -> Result<ManageCommand, EngineError> {
    let mut args = args.into_iter();
    let action = args.next().ok_or_else(|| {
        remote_error("missing service action (install/repair/remove)".to_string())
    })?;

    let mut source_path = None;
    let mut binary_path = PathBuf::from(DEFAULT_INSTALLED_BINARY);
    let mut unit_path = PathBuf::from(DEFAULT_UNIT_PATH);
    let mut socket_unit_path = PathBuf::from(DEFAULT_SOCKET_UNIT_PATH);
    let mut startup_repair_unit_path = PathBuf::from(DEFAULT_STARTUP_REPAIR_UNIT_PATH);
    let mut socket_group = Some(DEFAULT_SOCKET_GROUP.to_string());
    let mut socket_user = None;
    let mut allowed_uid = None;
    let mut pending = None::<String>;

    for arg in args {
        let arg = into_string(arg);
        match pending.take().as_deref() {
            Some("source") => {
                source_path = Some(PathBuf::from(arg));
                continue;
            }
            Some("binary_path") => {
                binary_path = PathBuf::from(arg);
                continue;
            }
            Some("unit_path") => {
                unit_path = PathBuf::from(arg);
                continue;
            }
            Some("socket_unit_path") => {
                socket_unit_path = PathBuf::from(arg);
                continue;
            }
            Some("startup_repair_unit_path") => {
                startup_repair_unit_path = PathBuf::from(arg);
                continue;
            }
            Some("socket_group") => {
                socket_group = if arg.eq_ignore_ascii_case("none") {
                    None
                } else {
                    Some(arg)
                };
                continue;
            }
            Some("socket_user") => {
                socket_user = if arg.eq_ignore_ascii_case("none") {
                    None
                } else {
                    Some(arg)
                };
                continue;
            }
            Some("allowed_uid") => {
                allowed_uid =
                    Some(arg.parse().map_err(|_| {
                        remote_error(format!("invalid --allowed-uid value: {arg}"))
                    })?);
                continue;
            }
            Some(other) => {
                return Err(remote_error(format!(
                    "unknown pending service arg: {other}"
                )));
            }
            None => {}
        }

        match arg.as_str() {
            "--source" => pending = Some("source".to_string()),
            "--binary-path" => pending = Some("binary_path".to_string()),
            "--unit-path" => pending = Some("unit_path".to_string()),
            "--socket-unit-path" => pending = Some("socket_unit_path".to_string()),
            "--startup-repair-unit-path" => pending = Some("startup_repair_unit_path".to_string()),
            "--socket-group" => pending = Some("socket_group".to_string()),
            "--socket-user" => pending = Some("socket_user".to_string()),
            "--allowed-uid" => pending = Some("allowed_uid".to_string()),
            other => {
                return Err(remote_error(format!(
                    "unknown service management arg: {other}"
                )))
            }
        }
    }

    if let Some(flag) = pending {
        return Err(remote_error(format!(
            "missing value for --{}",
            flag.replace('_', "-")
        )));
    }

    match action.to_string_lossy().as_ref() {
        "install" => Ok(ManageCommand::Install(InstallOptions {
            source_path: source_path
                .ok_or_else(|| remote_error("service install requires --source".to_string()))?,
            binary_path,
            unit_path,
            socket_unit_path,
            startup_repair_unit_path,
            socket_group,
            socket_user,
            allowed_uid,
        })),
        "repair" => Ok(ManageCommand::Repair(InstallOptions {
            source_path: source_path
                .ok_or_else(|| remote_error("service repair requires --source".to_string()))?,
            binary_path,
            unit_path,
            socket_unit_path,
            startup_repair_unit_path,
            socket_group,
            socket_user,
            allowed_uid,
        })),
        "remove" => Ok(ManageCommand::Remove(RemoveOptions {
            binary_path,
            unit_path,
            socket_unit_path,
            startup_repair_unit_path,
        })),
        "startup-repair" => Ok(ManageCommand::StartupRepair),
        other => Err(remote_error(format!("unknown service action: {other}"))),
    }
}

fn exit_linux_entry_error(context: &str, err: EngineError) -> ! {
    tracing::error!("{context}: {err}");
    std::process::exit(1);
}
