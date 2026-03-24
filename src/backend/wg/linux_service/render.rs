use std::fs;
use std::io;
use std::path::Path;

use super::super::EngineError;
use super::install_model::{InstallAuthMode, DEFAULT_SOCKET_PATH};
use super::remote_error;

pub(super) fn load_existing_install_auth_mode(
    unit_path: &Path,
    socket_unit_path: &Path,
) -> Result<Option<InstallAuthMode>, EngineError> {
    let service_text = match fs::read_to_string(unit_path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(remote_error(format!(
                "failed to read existing service unit {}: {err}",
                unit_path.display()
            )))
        }
    };
    let socket_text = match fs::read_to_string(socket_unit_path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(remote_error(format!(
                "failed to read existing socket unit {}: {err}",
                socket_unit_path.display()
            )))
        }
    };

    let mut mode = InstallAuthMode::default();

    if let Some(exec_start) = unit_value(&service_text, "ExecStart") {
        let mut parts = exec_start.split_whitespace();
        while let Some(part) = parts.next() {
            match part {
                "--socket-group" => {
                    if let Some(value) = parts.next() {
                        mode.socket_group = Some(value.to_string());
                    }
                }
                "--allowed-uid" => {
                    if let Some(value) = parts.next() {
                        mode.allowed_uid = value.parse().ok();
                    }
                }
                _ => {}
            }
        }
    }

    if let Some(socket_user) = unit_value(&socket_text, "SocketUser") {
        if socket_user != "root" {
            mode.socket_user = Some(socket_user.to_string());
        }
    }
    if let Some(socket_group) = unit_value(&socket_text, "SocketGroup") {
        mode.socket_group = Some(socket_group.to_string());
    }

    Ok(Some(mode))
}

fn unit_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines()
        .map(str::trim)
        .find(|line| line.starts_with(key) && line[key.len()..].starts_with('='))
        .map(|line| line[key.len() + 1..].trim())
}

pub(super) fn render_service_unit(
    binary_path: &Path,
    socket_group: Option<&str>,
    allowed_uid: Option<u32>,
) -> String {
    let mut exec_start = format!("{}", binary_path.display());
    exec_start.push_str(" --linux-service");
    if let Some(group) = socket_group {
        exec_start.push_str(" --socket-group ");
        exec_start.push_str(group);
    }
    if let Some(uid) = allowed_uid {
        exec_start.push_str(" --allowed-uid ");
        exec_start.push_str(&uid.to_string());
    }
    format!(
        "[Unit]\nDescription=r-wg privileged backend\nAfter=network-online.target\nWants=network-online.target\nRequires=r-wg.socket\n\n[Service]\nType=simple\nExecStart={exec_start}\nRestart=on-failure\nRestartSec=1\nStateDirectory=r-wg\nNoNewPrivileges=yes\n\n[Install]\nWantedBy=multi-user.target\n"
    )
}

pub(super) fn render_socket_unit(socket_user: Option<&str>, socket_group: Option<&str>) -> String {
    let mut unit = format!(
        "[Unit]\nDescription=r-wg privileged backend socket\n\n[Socket]\nListenStream={DEFAULT_SOCKET_PATH}\n"
    );
    match (socket_user, socket_group) {
        (Some(user), _) => {
            unit.push_str("DirectoryMode=0711\nSocketMode=0600\n");
            unit.push_str("SocketUser=");
            unit.push_str(user);
            unit.push('\n');
        }
        (None, Some(group)) => {
            unit.push_str("DirectoryMode=0711\nSocketMode=0660\nSocketUser=root\nSocketGroup=");
            unit.push_str(group);
            unit.push('\n');
        }
        (None, None) => {
            unit.push_str("DirectoryMode=0700\nSocketMode=0600\nSocketUser=root\n");
        }
    }
    unit.push_str("RemoveOnStop=true\n\n[Install]\nWantedBy=sockets.target\n");
    unit
}

pub(super) fn render_startup_repair_unit(binary_path: &Path) -> String {
    format!(
        "[Unit]\nDescription=r-wg boot-time startup repair\nAfter=local-fs.target\nConditionPathExists=/var/lib/r-wg/recovery.json\n\n[Service]\nType=oneshot\nExecStart={} service startup-repair\nStateDirectory=r-wg\n\n[Install]\nWantedBy=multi-user.target\n",
        binary_path.display()
    )
}

pub(super) fn render_desktop_entry(binary_path: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=r-wg\nComment=WireGuard desktop client\nExec={}\nIcon=r-wg\nTerminal=false\nCategories=Network;Utility;\nKeywords=WireGuard;VPN;Tunnel;\nStartupNotify=true\nStartupWMClass=r-wg\n",
        binary_path.display()
    )
}
