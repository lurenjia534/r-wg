// Focused regression tests for backend action guidance.

use super::backend_diagnostics::{
    active_wireguard_backend_label, backend_recommended_action, backend_recovery_note,
    should_show_remove_action, should_show_repair_action,
};
use gpui_component::theme::ThemeMode;
use r_wg::{
    application::TunnelSessionService,
    backend::wg::{ActiveBackendStatus, Engine},
};

use crate::ui::{
    features::themes::AppearancePolicy,
    state::{BackendDiagnostic, BackendHealth, WgApp},
};

fn diagnostic(health: BackendHealth) -> BackendDiagnostic {
    BackendDiagnostic {
        health,
        detail: "".into(),
        checked_at: None,
    }
}

fn make_app() -> WgApp {
    WgApp::new(
        TunnelSessionService::new(Engine::new()),
        AppearancePolicy::Dark,
        ThemeMode::Dark,
        None,
        None,
        None,
        None,
        Default::default(),
    )
}

#[test]
fn running_backend_keeps_repair_and_remove_available() {
    let diagnostic = diagnostic(BackendHealth::Running);

    assert!(should_show_repair_action(&diagnostic));
    assert!(should_show_remove_action(&diagnostic));
}

#[test]
fn running_backend_explains_recovery_actions() {
    let diagnostic = diagnostic(BackendHealth::Running);
    let note = backend_recovery_note(&diagnostic).map(|value| value.to_string());

    assert_eq!(
        backend_recommended_action(&diagnostic),
        "Repair or Remove can stop the running helper before applying system changes."
    );
    assert_eq!(
        note.as_deref(),
        Some(
            "Repair or Remove can stop the running helper first when you need to recover or uninstall it."
        )
    );
}

#[test]
fn diagnostics_label_reports_active_wireguard_backend() {
    let mut app = make_app();
    assert_eq!(active_wireguard_backend_label(&app), "Not running");

    app.runtime.running = true;
    assert_eq!(active_wireguard_backend_label(&app), "Unknown");

    app.runtime.active_backend = Some(ActiveBackendStatus::LinuxKernel);
    assert_eq!(active_wireguard_backend_label(&app), "Linux Kernel");

    app.runtime.active_backend = Some(ActiveBackendStatus::UserspaceGotaTun);
    assert_eq!(active_wireguard_backend_label(&app), "GotaTun");
}
