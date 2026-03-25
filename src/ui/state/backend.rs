use std::time::SystemTime;

use gpui::SharedString;
use r_wg::backend::wg::{PrivilegedServiceAction, PrivilegedServiceStatus};
use serde::{Deserialize, Serialize};

// Backend health/status modeling and action gating.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConfigInspectorTab {
    #[serde(alias = "status")]
    Preview,
    #[serde(alias = "logs")]
    Activity,
    Diagnostics,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConfigsPrimaryPane {
    Library,
    Editor,
    Inspector,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TrafficPeriod {
    Today,
    ThisMonth,
    LastMonth,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum BackendHealth {
    Unknown,
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    Unsupported,
    Checking,
    Running,
    Installed,
    NotInstalled,
    AccessDenied,
    VersionMismatch {
        expected: u32,
        actual: u32,
    },
    Unreachable,
    Working {
        action: PrivilegedServiceAction,
    },
}

impl BackendHealth {
    pub(crate) fn summary(&self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            #[cfg(not(any(target_os = "linux", target_os = "windows")))]
            Self::Unsupported => "Unsupported",
            Self::Checking => "Checking",
            Self::Running => "Running",
            Self::Installed => "Installed",
            Self::NotInstalled => "Not installed",
            Self::AccessDenied => "Access denied",
            Self::VersionMismatch { .. } => "Version mismatch",
            Self::Unreachable => "Unreachable",
            Self::Working { action } => match action {
                PrivilegedServiceAction::Install => "Installing",
                PrivilegedServiceAction::Repair => "Repairing",
                PrivilegedServiceAction::Remove => "Removing",
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BackendDiagnostic {
    pub(crate) health: BackendHealth,
    pub(crate) detail: SharedString,
    pub(crate) checked_at: Option<SystemTime>,
}

impl BackendDiagnostic {
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    pub(crate) fn default_for_platform() -> Self {
        Self {
            health: BackendHealth::Unknown,
            detail: "Refresh to probe the privileged backend service.".into(),
            checked_at: None,
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    pub(crate) fn default_for_platform() -> Self {
        Self::unsupported()
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    pub(crate) fn unsupported() -> Self {
        Self {
            health: BackendHealth::Unsupported,
            detail: "Privileged backend management is not supported on this platform.".into(),
            checked_at: None,
        }
    }

    pub(crate) fn checking() -> Self {
        Self {
            health: BackendHealth::Checking,
            detail: "Probing privileged backend service...".into(),
            checked_at: None,
        }
    }

    pub(crate) fn working(action: PrivilegedServiceAction) -> Self {
        let detail = match action {
            PrivilegedServiceAction::Install => "Installing the privileged backend service...",
            PrivilegedServiceAction::Repair => "Repairing the privileged backend service...",
            PrivilegedServiceAction::Remove => "Removing the privileged backend service...",
        };

        Self {
            health: BackendHealth::Working { action },
            detail: detail.into(),
            checked_at: None,
        }
    }

    pub(crate) fn from_probe_status(status: PrivilegedServiceStatus) -> Self {
        match status {
            PrivilegedServiceStatus::Running => Self {
                health: BackendHealth::Running,
                detail:
                    "The privileged backend service is running and ready to handle tunnel control."
                        .into(),
                checked_at: None,
            },
            PrivilegedServiceStatus::Installed => Self {
                health: BackendHealth::Installed,
                detail:
                    "The privileged backend service is installed but not currently reporting a live control channel."
                        .into(),
                checked_at: None,
            },
            PrivilegedServiceStatus::NotInstalled => Self {
                health: BackendHealth::NotInstalled,
                detail:
                    "Install the privileged backend to enable tunnel control from the unprivileged UI."
                        .into(),
                checked_at: None,
            },
            PrivilegedServiceStatus::AccessDenied => Self {
                health: BackendHealth::AccessDenied,
                detail:
                    "The backend service exists, but this user cannot access its control channel."
                        .into(),
                checked_at: None,
            },
            PrivilegedServiceStatus::VersionMismatch { expected, actual } => Self {
                health: BackendHealth::VersionMismatch { expected, actual },
                detail: format!(
                    "GUI expects protocol v{expected}, but the running service reports v{actual}. Repair the backend installation."
                )
                .into(),
                checked_at: None,
            },
            PrivilegedServiceStatus::Unreachable(message) => Self {
                health: BackendHealth::Unreachable,
                detail: message.into(),
                checked_at: None,
            },
            #[cfg(not(any(target_os = "linux", target_os = "windows")))]
            PrivilegedServiceStatus::Unsupported => Self::unsupported(),
        }
        .checked_now()
    }

    pub(crate) fn summary(&self) -> &'static str {
        self.health.summary()
    }

    pub(crate) fn badge_label(&self) -> &'static str {
        match self.health {
            BackendHealth::Running => "Backend ready",
            BackendHealth::Checking => "Checking backend",
            BackendHealth::Working { action } => match action {
                PrivilegedServiceAction::Install => "Installing",
                PrivilegedServiceAction::Repair => "Repairing",
                PrivilegedServiceAction::Remove => "Removing",
            },
            _ => self.summary(),
        }
    }

    pub(crate) fn is_busy(&self) -> bool {
        matches!(
            self.health,
            BackendHealth::Checking | BackendHealth::Working { .. }
        )
    }

    pub(crate) fn allows_action(&self, action: PrivilegedServiceAction) -> bool {
        match action {
            PrivilegedServiceAction::Install => {
                matches!(self.health, BackendHealth::NotInstalled)
            }
            PrivilegedServiceAction::Repair => matches!(
                self.health,
                BackendHealth::Installed
                    | BackendHealth::Running
                    | BackendHealth::AccessDenied
                    | BackendHealth::VersionMismatch { .. }
                    | BackendHealth::Unreachable
            ),
            PrivilegedServiceAction::Remove => matches!(
                self.health,
                BackendHealth::Installed
                    | BackendHealth::Running
                    | BackendHealth::AccessDenied
                    | BackendHealth::VersionMismatch { .. }
            ),
        }
    }

    pub(crate) fn is_working_action(&self, action: PrivilegedServiceAction) -> bool {
        matches!(self.health, BackendHealth::Working { action: current } if current == action)
    }

    pub(crate) fn with_checked_at(mut self, checked_at: Option<SystemTime>) -> Self {
        self.checked_at = checked_at;
        self
    }

    fn checked_now(mut self) -> Self {
        self.checked_at = Some(SystemTime::now());
        self
    }
}
