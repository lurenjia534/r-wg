use gpui::{AppContext, Context};

use r_wg::backend::wg::PrivilegedServiceAction;

use crate::ui::state::{BackendDiagnostic, WgApp};

#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(crate) fn refresh_privileged_backend_status(app: &mut WgApp, cx: &mut Context<WgApp>) {
    let last_checked = app.ui.backend.checked_at;
    let backend_admin = app.backend_admin.clone();
    app.ui
        .set_backend_diagnostic(BackendDiagnostic::checking().with_checked_at(last_checked));
    cx.notify();

    cx.spawn(async move |view, cx| {
        let status = cx
            .background_spawn(async move { backend_admin.probe_status() })
            .await;
        let _ = view.update(cx, |this, cx| {
            this.ui
                .set_backend_diagnostic(BackendDiagnostic::from_probe_status(status));
            cx.notify();
        });
    })
    .detach();
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(crate) fn run_privileged_backend_action(
    app: &mut WgApp,
    action: PrivilegedServiceAction,
    cx: &mut Context<WgApp>,
) {
    let backend_admin = app.backend_admin.clone();
    let verb = backend_admin.action_verb(action);
    let last_checked = app.ui.backend.checked_at;
    app.set_status(format!("{verb} privileged backend..."));
    app.ui
        .set_backend_diagnostic(BackendDiagnostic::working(action).with_checked_at(last_checked));
    cx.notify();

    cx.spawn(async move |view, cx| {
        let result = cx
            .background_spawn(async move { backend_admin.run_action(action) })
            .await;
        let _ = view.update(cx, |this, cx| {
            match result {
                Ok(()) => {
                    let done = this.backend_admin.action_success_message(action);
                    this.set_status(done);
                }
                Err(err) => {
                    let message = format!("Backend action failed: {err}");
                    this.ui.set_backend_last_error(message.clone());
                    this.set_error(message);
                }
            }
            refresh_privileged_backend_status(this, cx);
        });
    })
    .detach();
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub(crate) fn refresh_privileged_backend_status(app: &mut WgApp, cx: &mut Context<WgApp>) {
    app.ui
        .set_backend_diagnostic(BackendDiagnostic::unsupported());
    cx.notify();
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub(crate) fn run_privileged_backend_action(
    app: &mut WgApp,
    _action: PrivilegedServiceAction,
    cx: &mut Context<WgApp>,
) {
    app.ui
        .set_backend_diagnostic(BackendDiagnostic::unsupported());
    cx.notify();
}
