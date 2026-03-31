use std::time::Duration;

use gpui::{AppContext, Context, SharedString, Window};
use r_wg::application::{
    decide_after_stop_success, decide_toggle, StartTunnelRequest, StopSuccessDecision,
    ToggleTunnelDecision, ToggleTunnelInput,
};
#[cfg(target_os = "windows")]
use r_wg::backend::wg::EngineError;
use r_wg::core::dns::DnsSelection;

use crate::ui::permissions::start_permission_message;
use crate::ui::state::{PendingStart, TunnelConfig, WgApp};
use crate::ui::tray;

pub(crate) fn handle_start_stop(app: &mut WgApp, _window: &mut Window, cx: &mut Context<WgApp>) {
    handle_start_stop_core(app, cx);
}

pub(crate) fn handle_start_stop_core(app: &mut WgApp, cx: &mut Context<WgApp>) {
    let draft = app.configs_draft_snapshot(cx);
    let decision = decide_toggle(ToggleTunnelInput {
        busy: app.runtime.busy,
        running: app.runtime.running,
        selected_config_id: app.selection.selected_id,
        running_config_id: app.runtime.running_id,
        draft_has_saved_source: draft.source_id.is_some(),
        draft_is_dirty: draft.is_dirty(),
        restart_delay: app.runtime.restart_delay(),
    });

    match decision {
        ToggleTunnelDecision::Noop => {}
        ToggleTunnelDecision::QueuePendingStart { config_id } => {
            if app
                .runtime
                .queue_pending_start(Some(PendingStart { config_id }))
            {
                app.set_status("Stopping... (queued start)");
                cx.notify();
            }
        }
        ToggleTunnelDecision::StopRunning => {
            app.runtime.begin_stop();
            app.stats.stats_generation = app.stats.stats_generation.wrapping_add(1);
            app.set_status("Stopping...");
            cx.notify();

            let tunnel_session = app.tunnel_session.clone();
            cx.spawn(async move |view, cx| {
                let stop_task = cx.background_spawn(async move { tunnel_session.stop() });
                let result = stop_task.await;
                view.update(cx, |this, cx| {
                    match result {
                        #[cfg(target_os = "windows")]
                        Ok(()) | Err(EngineError::NotRunning) | Err(EngineError::ChannelClosed) => {
                            complete_stop_success(this, cx);
                            tray::notify_system("r-wg", "Tunnel disconnected", false);
                            restart_pending_start(this, cx);
                        }
                        #[cfg(not(target_os = "windows"))]
                        Ok(()) => {
                            complete_stop_success(this, cx);
                            tray::notify_system("r-wg", "Tunnel disconnected", false);
                            restart_pending_start(this, cx);
                        }
                        Err(err) => {
                            let message = format!("Stop failed: {err}");
                            complete_stop_failure(this, message.clone());
                            tray::notify_system("r-wg", &message, true);
                        }
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        ToggleTunnelDecision::StartSelected {
            config_id,
            restart_delay,
        } => {
            start_config_by_id(app, config_id, restart_delay, cx, "Select a tunnel first");
        }
        ToggleTunnelDecision::Blocked(reason) => {
            app.set_error(reason.message());
            cx.notify();
        }
    };
}

pub(crate) fn handle_start_from_tray(app: &mut WgApp, cx: &mut Context<WgApp>) {
    if app.runtime.running || app.runtime.busy {
        return;
    }
    handle_start_stop_core(app, cx);
}

pub(crate) fn handle_stop_from_tray(app: &mut WgApp, cx: &mut Context<WgApp>) {
    if !app.runtime.running || app.runtime.busy {
        return;
    }
    handle_start_stop_core(app, cx);
}

fn start_with_config(
    app: &mut WgApp,
    selected: TunnelConfig,
    initial_text: Option<SharedString>,
    delay: Option<Duration>,
    cx: &mut Context<WgApp>,
) {
    if let Some(message) = start_permission_message() {
        app.set_error(message);
        cx.notify();
        return;
    }

    app.runtime.begin_start();
    app.set_status(format!("Starting {}...", selected.name));
    cx.notify();

    let tunnel_session = app.tunnel_session.clone();
    let config_library = app.config_library.clone();
    let dns_selection = DnsSelection::new(app.ui_prefs.dns_mode, app.ui_prefs.dns_preset);
    cx.spawn(async move |view, cx| {
        if let Some(delay) = delay {
            cx.background_executor().timer(delay).await;
        }

        let text_result = match initial_text {
            Some(text) => Ok(text),
            None => {
                let path = selected.storage_path.clone();
                let read_task =
                    cx.background_spawn(async move { config_library.read_config_text(&path) });
                match read_task.await {
                    Ok(text) => Ok(SharedString::from(text)),
                    Err(err) => Err(err),
                }
            }
        };

        let text = match text_result {
            Ok(text) => text,
            Err(message) => {
                view.update(cx, |this, cx| {
                    this.runtime.finish_start_attempt();
                    this.set_error(message);
                    cx.notify();
                })
                .ok();
                return;
            }
        };

        let text_for_cache = text.clone();
        let path_for_cache = selected.storage_path.clone();
        view.update(cx, |this, _| {
            this.cache_config_text(path_for_cache, text_for_cache);
        })
        .ok();

        let request = StartTunnelRequest::new(selected.name.clone(), text.to_string(), dns_selection);
        let start_task = cx.background_spawn(async move {
            let outcome = tunnel_session.start(request);
            (outcome.result, outcome.apply_report)
        });
        let (result, apply_report) = start_task.await;
        view.update(cx, |this, cx| {
            this.runtime.finish_start_attempt();
            match result {
                Ok(()) => {
                    this.runtime.mark_started(&selected);
                    this.runtime.set_last_apply_report(apply_report);
                    this.refresh_configs_workspace_row_flags(cx);
                    this.stats.reset_for_start();
                    this.set_status(format!("Running {}", selected.name));
                    tray::notify_system(
                        "r-wg",
                        &format!("Tunnel connected: {}", selected.name),
                        false,
                    );
                    this.start_stats_polling(cx);
                }
                Err(err) => {
                    this.runtime.set_last_apply_report(apply_report);
                    let message = format!("Start failed: {err}");
                    this.set_error(message.clone());
                    tray::notify_system("r-wg", &message, true);
                }
            }
            cx.notify();
        })
        .ok();
    })
    .detach();
}

fn start_config_by_id(
    app: &mut WgApp,
    config_id: u64,
    delay: Option<Duration>,
    cx: &mut Context<WgApp>,
    missing_message: &str,
) {
    let Some(selected) = app.configs.find_by_id(config_id) else {
        app.set_error(missing_message.to_string());
        cx.notify();
        return;
    };
    let cached_text = app.cached_config_text(&selected.storage_path);
    let initial_text = selected.text.clone().or(cached_text);
    start_with_config(app, selected, initial_text, delay, cx);
}

fn restart_pending_start(app: &mut WgApp, cx: &mut Context<WgApp>) {
    let pending_config_id = app.runtime.pending_start.take().map(|pending| pending.config_id);
    match decide_after_stop_success(pending_config_id) {
        StopSuccessDecision::RestartPending { config_id } => {
            let delay = app.runtime.restart_delay();
            start_config_by_id(app, config_id, delay, cx, "Pending start config not found");
        }
        StopSuccessDecision::Idle => {}
    }
}

pub(crate) fn complete_stop_success(app: &mut WgApp, cx: &mut Context<WgApp>) {
    app.runtime.finish_stop_success();
    app.refresh_configs_workspace_row_flags(cx);
    app.stats.clear_runtime_metrics();
    app.set_status("Stopped");
}

pub(crate) fn complete_stop_failure(app: &mut WgApp, message: impl Into<SharedString>) {
    app.runtime.finish_stop_failure();
    app.set_error(message);
}
