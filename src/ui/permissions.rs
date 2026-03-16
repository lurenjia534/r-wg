use gpui::SharedString;

#[cfg(any(target_os = "linux", target_os = "windows"))]
use r_wg::backend::wg::{probe_privileged_service, PrivilegedServiceStatus};

#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(crate) fn start_permission_message() -> Option<SharedString> {
    match probe_privileged_service() {
        PrivilegedServiceStatus::Running => None,
        PrivilegedServiceStatus::Installed => None,
        PrivilegedServiceStatus::NotInstalled => Some(
            "Privileged backend service is not installed. Install it from Settings."
                .into(),
        ),
        PrivilegedServiceStatus::AccessDenied => Some(
            "Access denied to the privileged backend service."
                .into(),
        ),
        PrivilegedServiceStatus::VersionMismatch { expected, actual } => Some(
            format!(
                "Privileged backend protocol mismatch. GUI expects v{expected}, service reports v{actual}. Repair the backend installation."
            )
            .into(),
        ),
        PrivilegedServiceStatus::Unreachable(message) => Some(message.into()),
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub(crate) fn start_permission_message() -> Option<SharedString> {
    None
}
