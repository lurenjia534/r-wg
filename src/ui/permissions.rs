use gpui::SharedString;

#[cfg(target_os = "linux")]
use r_wg::backend::wg::{probe_privileged_service, PrivilegedServiceStatus};

#[cfg(target_os = "linux")]
pub(crate) fn start_permission_message() -> Option<SharedString> {
    match probe_privileged_service() {
        PrivilegedServiceStatus::Running => None,
        PrivilegedServiceStatus::Installed => None,
        PrivilegedServiceStatus::NotInstalled => Some(
            "Linux privileged backend is not installed. Install it from Settings or via `pkexec r-wg service install --source <path>`."
                .into(),
        ),
        PrivilegedServiceStatus::AccessDenied => Some(
            "Access denied to the Linux privileged backend. Check /run/r-wg/control.sock permissions and your backend access group."
                .into(),
        ),
        PrivilegedServiceStatus::VersionMismatch { expected, actual } => Some(
            format!(
                "Linux privileged backend protocol mismatch. GUI expects v{expected}, service reports v{actual}. Reinstall or restart r-wg.service."
            )
            .into(),
        ),
        PrivilegedServiceStatus::Unreachable(message) => Some(message.into()),
    }
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn start_permission_message() -> Option<SharedString> {
    None
}
