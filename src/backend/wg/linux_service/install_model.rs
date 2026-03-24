use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub(super) const SERVICE_ARG: &str = "--linux-service";
pub(super) const SERVICE_SUBCOMMAND: &str = "service";
pub(super) const DEFAULT_SOCKET_PATH: &str = "/run/r-wg/control.sock";
pub(super) const DEFAULT_SOCKET_GROUP: &str = "r-wg";
pub(super) const DEFAULT_INSTALLED_BINARY: &str = "/usr/local/libexec/r-wg/r-wg";
pub(super) const DEFAULT_UNIT_PATH: &str = "/etc/systemd/system/r-wg.service";
pub(super) const DEFAULT_SOCKET_UNIT_PATH: &str = "/etc/systemd/system/r-wg.socket";
pub(super) const DEFAULT_STARTUP_REPAIR_UNIT_PATH: &str = "/etc/systemd/system/r-wg-repair.service";
pub(super) const DEFAULT_DESKTOP_ENTRY_PATH: &str = "/usr/share/applications/r-wg.desktop";
pub(super) const DEFAULT_ICON_SVG_PATH: &str = "/usr/share/icons/hicolor/scalable/apps/r-wg.svg";
pub(super) const DEFAULT_ICON_PNG_PATH: &str = "/usr/share/icons/hicolor/256x256/apps/r-wg.png";
pub(super) const SERVICE_UNIT_NAME: &str = "r-wg.service";
pub(super) const SOCKET_UNIT_NAME: &str = "r-wg.socket";
pub(super) const STARTUP_REPAIR_UNIT_NAME: &str = "r-wg-repair.service";
pub(super) const SERVICE_POLL_INTERVAL: Duration = Duration::from_millis(200);
pub(super) const SERVICE_IO_TIMEOUT: Duration = Duration::from_secs(30);
pub(super) const SERVICE_IDLE_TIMEOUT: Duration = Duration::from_secs(15);

pub(super) const DESKTOP_ICON_SVG: &[u8] =
    include_bytes!("../../../../resources/icons/r-wg.svg");
pub(super) const DESKTOP_ICON_PNG: &[u8] =
    include_bytes!("../../../../resources/icons/hicolor/256x256/apps/r-wg.png");

/// Linux privileged backend probe result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrivilegedServiceStatus {
    Running,
    Installed,
    NotInstalled,
    AccessDenied,
    VersionMismatch { expected: u32, actual: u32 },
    Unreachable(String),
}

/// Management actions exposed to the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivilegedServiceAction {
    Install,
    Repair,
    Remove,
}

impl PrivilegedServiceAction {
    pub(super) fn as_cli(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Repair => "repair",
            Self::Remove => "remove",
        }
    }
}

pub(super) enum LinuxEntryCommand {
    ServiceMode(ServiceOptions),
    Manage(ManageCommand),
}

pub(super) struct ServiceOptions {
    pub(super) socket_path: PathBuf,
    pub(super) socket_group: Option<String>,
    pub(super) allowed_uid: Option<u32>,
}

pub(super) enum ManageCommand {
    Install(InstallOptions),
    Repair(InstallOptions),
    Remove(RemoveOptions),
    StartupRepair,
}

pub(super) struct InstallOptions {
    pub(super) source_path: PathBuf,
    pub(super) binary_path: PathBuf,
    pub(super) unit_path: PathBuf,
    pub(super) socket_unit_path: PathBuf,
    pub(super) startup_repair_unit_path: PathBuf,
    pub(super) socket_group: Option<String>,
    pub(super) socket_user: Option<String>,
    pub(super) allowed_uid: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct InstallAuthMode {
    pub(super) socket_group: Option<String>,
    pub(super) socket_user: Option<String>,
    pub(super) allowed_uid: Option<u32>,
}

pub(super) struct RemoveOptions {
    pub(super) binary_path: PathBuf,
    pub(super) unit_path: PathBuf,
    pub(super) socket_unit_path: PathBuf,
    pub(super) startup_repair_unit_path: PathBuf,
}

pub(super) fn control_socket_path() -> PathBuf {
    env::var_os("RWG_CONTROL_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_SOCKET_PATH))
}

pub(super) fn installation_exists() -> bool {
    Path::new(DEFAULT_UNIT_PATH).exists()
        || Path::new(DEFAULT_SOCKET_UNIT_PATH).exists()
        || Path::new(DEFAULT_INSTALLED_BINARY).exists()
}

pub(super) fn into_string(arg: OsString) -> String {
    arg.to_string_lossy().to_string()
}
