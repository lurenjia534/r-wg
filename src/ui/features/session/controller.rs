use std::time::Duration;

use gpui::{AppContext, Context, SharedString, Window};
#[cfg(target_os = "windows")]
use r_wg::backend::wg::EngineError;
use r_wg::backend::wg::StartRequest;
use r_wg::core::dns::DnsSelection;

use crate::ui::permissions::start_permission_message;
use crate::ui::state::{TunnelConfig, WgApp};
use crate::ui::tray;

pub(crate) fn handle_start_stop(
    app: &mut WgApp,
    _window: &mut Window,
    cx: &mut Context<WgApp>,
) {
    handle_start_stop_core(app, cx);
}

pub(crate) fn handle_start_stop_core(app: &mut WgApp, cx: &mut Context<WgApp>) {
    let draft = app.configs_draft_snapshot(cx);
    if app.runtime.busy {
        if app.runtime.running
            && app.runtime.queue_pending_start(app.selection.build_pending_start(
                &app.configs,
                &app.runtime,
            ))
        {
            app.set_status("Stopping... (queued start)");
            cx.notify();
        }
        return;
    }

    if app.runtime.running {
        app.runtime.begin_stop();
        app.stats.stats_generation = app.stats.stats_generation.wrapping_add(1);
        app.set_status("Stopping...");
        cx.notify();

        let engine = app.engine.clone();
        cx.spawn(async move |view, cx| {
            let stop_task = cx.background_spawn(async move { engine.stop() });
            let result = stop_task.await;
            view.update(cx, |this, cx| {
                match result {
                    #[cfg(target_os = "windows")]
                    Ok(()) | Err(EngineError::NotRunning) | Err(EngineError::ChannelClosed) => {
                        complete_stop_success(this, cx);
                        tray::notify_system("r-wg", "Tunnel disconnected", false);

                        if let Some(pending) = this.runtime.pending_start.take() {
                            if let Some(selected) = this.configs.find_by_id(pending.config_id) {
                                let cached_text = this.cached_config_text(&selected.storage_path);
                                let initial_text = selected.text.clone().or(cached_text);
                                let delay = this.runtime.restart_delay();
                                start_with_config(this, selected, initial_text, delay, cx);
                            } else {
                                this.set_error("Pending start config not found".to_string());
                            }
                        }
                    }
                    #[cfg(not(target_os = "windows"))]
                    Ok(()) => {
                        complete_stop_success(this, cx);
                        tray::notify_system("r-wg", "Tunnel disconnected", false);

                        if let Some(pending) = this.runtime.pending_start.take() {
                            if let Some(selected) = this.configs.find_by_id(pending.config_id) {
                                let cached_text = this.cached_config_text(&selected.storage_path);
                                let initial_text = selected.text.clone().or(cached_text);
                                let delay = this.runtime.restart_delay();
                                start_with_config(this, selected, initial_text, delay, cx);
                            } else {
                                this.set_error("Pending start config not found".to_string());
                            }
                        }
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
        return;
    }

    if draft.source_id.is_none() {
        app.set_error("Save this draft before starting");
        cx.notify();
        return;
    }
    if draft.is_dirty() {
        app.set_error("Save changes before starting");
        cx.notify();
        return;
    }

    let Some(selected) = app.selected_config().cloned() else {
        app.set_error("Select a tunnel first");
        cx.notify();
        return;
    };
    let cached_text = app.cached_config_text(&selected.storage_path);
    let initial_text = selected.text.clone().or(cached_text);
    let delay = app.runtime.restart_delay();
    start_with_config(app, selected, initial_text, delay, cx);
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

    let engine = app.engine.clone();
    let dns_selection = DnsSelection::new(app.ui_prefs.dns_mode, app.ui_prefs.dns_preset);
    cx.spawn(async move |view, cx| {
        if let Some(delay) = delay {
            cx.background_executor().timer(delay).await;
        }

        let text_result = match initial_text {
            Some(text) => Ok(text),
            None => {
                let path = selected.storage_path.clone();
                let read_task = cx.background_spawn(async move { std::fs::read_to_string(&path) });
                match read_task.await {
                    Ok(text) => Ok(SharedString::from(text)),
                    Err(err) => Err(format!("Read failed: {err}")),
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

        let request = StartRequest::new(selected.name.clone(), text.to_string(), dns_selection);
        let start_task = cx.background_spawn(async move {
            let result = engine.start(request);
            let apply_report = engine.apply_report().ok().flatten();
            (result, apply_report)
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
