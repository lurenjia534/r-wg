use crate::application::BackendAdminService;
use crate::backend::wg::PrivilegedServiceStatus;

#[derive(Clone, Default)]
pub struct DiagnosticsService {
    backend_admin: BackendAdminService,
}

impl DiagnosticsService {
    pub fn new() -> Self {
        Self {
            backend_admin: BackendAdminService::new(),
        }
    }

    pub fn start_permission_message(&self) -> Option<String> {
        start_permission_message_for_status(self.backend_admin.probe_status())
    }
}

pub fn start_permission_message_for_status(status: PrivilegedServiceStatus) -> Option<String> {
    match status {
        PrivilegedServiceStatus::Running => None,
        PrivilegedServiceStatus::Installed => None,
        PrivilegedServiceStatus::NotInstalled => {
            Some("Privileged backend service is not installed. Install it from Settings.".to_string())
        }
        PrivilegedServiceStatus::AccessDenied => {
            Some("Access denied to the privileged backend service.".to_string())
        }
        PrivilegedServiceStatus::VersionMismatch { expected, actual } => Some(format!(
            "Privileged backend protocol mismatch. GUI expects v{expected}, service reports v{actual}. Repair the backend installation."
        )),
        PrivilegedServiceStatus::Unreachable(message) => Some(message),
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        PrivilegedServiceStatus::Unsupported => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_backend_allows_start() {
        assert_eq!(
            start_permission_message_for_status(PrivilegedServiceStatus::Running),
            None
        );
    }

    #[test]
    fn not_installed_backend_blocks_start() {
        assert_eq!(
            start_permission_message_for_status(PrivilegedServiceStatus::NotInstalled),
            Some(
                "Privileged backend service is not installed. Install it from Settings."
                    .to_string()
            )
        );
    }
}
