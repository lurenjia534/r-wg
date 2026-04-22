use gpui::{AppContext, Context};

use crate::ui::state::{DaitaResourcesDiagnostic, WgApp};

pub(crate) fn refresh_daita_resources_status(app: &mut WgApp, cx: &mut Context<WgApp>) {
    let last_checked = app.ui.daita_resources.checked_at;
    let previous = app.ui.daita_resources.clone();
    let tunnel_session = app.tunnel_session.clone();
    app.ui.set_daita_resources_diagnostic(
        DaitaResourcesDiagnostic::checking().with_checked_at(last_checked),
    );
    cx.notify();

    cx.spawn(async move |view, cx| {
        let result = cx
            .background_spawn(async move { tunnel_session.relay_inventory_status() })
            .await;
        let _ = view.update(cx, |this, cx| {
            match result {
                Ok(snapshot) => {
                    this.ui.set_daita_resources_diagnostic(
                        DaitaResourcesDiagnostic::from_snapshot(snapshot),
                    );
                }
                Err(err) => {
                    let message = format!("DAITA resources check failed: {err}");
                    this.ui
                        .set_daita_resources_diagnostic(DaitaResourcesDiagnostic::error(
                            message.clone(),
                            Some(&previous),
                        ));
                }
            }
            cx.notify();
        });
    })
    .detach();
}

pub(crate) fn refresh_daita_resources_cache(app: &mut WgApp, cx: &mut Context<WgApp>) {
    let tunnel_session = app.tunnel_session.clone();
    let previous = app.ui.daita_resources.clone();
    app.set_status("Refreshing DAITA resources...");
    app.ui
        .set_daita_resources_diagnostic(DaitaResourcesDiagnostic::refreshing(Some(&previous)));
    cx.notify();

    cx.spawn(async move |view, cx| {
        let result = cx
            .background_spawn(async move { tunnel_session.refresh_relay_inventory() })
            .await;
        let _ = view.update(cx, |this, cx| {
            match result {
                Ok(snapshot) => {
                    this.ui.set_daita_resources_diagnostic(
                        DaitaResourcesDiagnostic::from_snapshot(snapshot),
                    );
                    this.set_status("DAITA resources refreshed");
                }
                Err(err) => {
                    let message = format!("DAITA resources refresh failed: {err}");
                    this.ui
                        .set_daita_resources_diagnostic(DaitaResourcesDiagnostic::error(
                            message.clone(),
                            Some(&previous),
                        ));
                    this.set_error(message);
                }
            }
            cx.notify();
        });
    })
    .detach();
}
