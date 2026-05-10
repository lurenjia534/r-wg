use std::env::consts::{ARCH, OS};
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Local};
use gpui::SharedString;
use r_wg::backend::wg::PrivilegedServiceAction;

use crate::ui::state::{BackendDiagnostic, BackendHealth, WgApp};

pub(super) fn backend_checked_label(diagnostic: &BackendDiagnostic) -> SharedString {
    match diagnostic.checked_at {
        Some(checked_at) => {
            let prefix = if diagnostic.is_busy() {
                "Last checked "
            } else {
                "Checked "
            };
            format!("{prefix}{}", format_checked_age(checked_at)).into()
        }
        None if diagnostic.is_busy() => "Checking now".into(),
        None => "Not checked yet".into(),
    }
}

pub(super) fn build_backend_diagnostics_text(app: &WgApp) -> String {
    let diagnostic = &app.ui.backend;
    let checked = diagnostic
        .checked_at
        .map(format_checked_timestamp)
        .unwrap_or_else(|| "not checked yet".to_string());
    let wireguard_backend = active_wireguard_backend_label(app);
    let mut lines = vec![
        format!("App: r-wg v{}", env!("CARGO_PKG_VERSION")),
        format!("Platform: {OS} / {ARCH}"),
        format!("Active WireGuard backend: {wireguard_backend}"),
        format!("Health: {}", diagnostic.summary()),
        format!("Checked: {checked}"),
        format!("Integration: {}", helper_platform_detail()),
        format!("Control endpoint: {}", helper_control_endpoint()),
        format!(
            "Recommended next step: {}",
            backend_recommended_action(diagnostic)
        ),
        format!("Detail: {}", diagnostic.detail),
    ];
    if let Some(last_error) = &app.ui.backend_last_error {
        lines.push(format!("Backend last error: {last_error}"));
    }

    match diagnostic.health {
        BackendHealth::VersionMismatch { expected, actual } => {
            lines.push(format!(
                "Protocol mismatch: expected v{expected}, actual v{actual}"
            ));
        }
        BackendHealth::Unreachable => {
            lines.push(format!("Unreachable message: {}", diagnostic.detail));
        }
        _ => {}
    }

    lines.join("\n")
}

pub(super) fn active_wireguard_backend_label(app: &WgApp) -> String {
    if !app.runtime.running {
        return "Not running".to_string();
    }
    app.runtime
        .active_backend
        .map(|backend| backend.label().to_string())
        .unwrap_or_else(|| "Unknown".to_string())
}

pub(super) fn format_checked_timestamp(checked_at: SystemTime) -> String {
    let absolute = DateTime::<Local>::from(checked_at)
        .format("%Y-%m-%d %H:%M:%S local")
        .to_string();
    format!("{absolute} ({})", format_checked_age(checked_at))
}

fn format_checked_age(checked_at: SystemTime) -> String {
    let elapsed = SystemTime::now()
        .duration_since(checked_at)
        .unwrap_or(Duration::from_secs(0));
    let seconds = elapsed.as_secs();

    match seconds {
        0..=9 => "just now".to_string(),
        10..=59 => format!("{seconds}s ago"),
        60..=3_599 => format!("{} min ago", seconds / 60),
        3_600..=86_399 => format!("{} hr ago", seconds / 3_600),
        _ => format!("{} d ago", seconds / 86_400),
    }
}

pub(super) fn backend_recommended_action(diagnostic: &BackendDiagnostic) -> &'static str {
    match diagnostic.health {
        BackendHealth::Running => {
            "Repair or Remove can stop the running helper before applying system changes."
        }
        BackendHealth::NotInstalled => "Install the helper integration.",
        BackendHealth::Installed => "Refresh first, then Repair if the helper stays unavailable.",
        BackendHealth::AccessDenied | BackendHealth::VersionMismatch { .. } => {
            "Repair the helper integration."
        }
        BackendHealth::Unreachable => {
            "Refresh first, then Repair or Remove if the helper stays unreachable."
        }
        BackendHealth::Checking => "Wait for the current probe to finish.",
        BackendHealth::Working { .. } => "Wait for the current action to finish.",
        BackendHealth::Unknown => "Refresh to probe the helper state.",
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        BackendHealth::Unsupported => "No helper actions are available on this platform.",
    }
}

pub(super) fn helper_platform_detail() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "Linux privileged service"
    }
    #[cfg(target_os = "windows")]
    {
        "Windows privileged service"
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        "No privileged helper on this platform"
    }
}

pub(super) fn helper_control_endpoint() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "/run/r-wg/control.sock"
    }
    #[cfg(target_os = "windows")]
    {
        r"\\.\pipe\r-wg-control"
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        "Not available"
    }
}

pub(super) fn should_show_repair_action(diagnostic: &BackendDiagnostic) -> bool {
    diagnostic.allows_action(PrivilegedServiceAction::Repair)
}

pub(super) fn should_show_remove_action(diagnostic: &BackendDiagnostic) -> bool {
    diagnostic.allows_action(PrivilegedServiceAction::Remove)
}

pub(super) fn backend_recovery_note(diagnostic: &BackendDiagnostic) -> Option<SharedString> {
    let note = match diagnostic.health {
        BackendHealth::Running => {
            "Repair or Remove can stop the running helper first when you need to recover or uninstall it."
        }
        BackendHealth::NotInstalled => {
            "Install is the recommended next step before using desktop tunnel start, route, or DNS actions."
        }
        BackendHealth::Installed => {
            "The helper is installed but not currently live. Refresh first, then repair if the control channel stays unavailable."
        }
        BackendHealth::AccessDenied => {
            "Repair is the recommended next step when the helper exists but this account cannot reach it."
        }
        BackendHealth::VersionMismatch { .. } => {
            "Repair is the recommended next step when the installed helper protocol does not match this GUI build."
        }
        BackendHealth::Unreachable => {
            "Refresh re-checks the current state. Repair tries to recover the helper; Remove uninstalls stale integration when recovery is not worth it."
        }
        _ => return None,
    };

    Some(note.into())
}
