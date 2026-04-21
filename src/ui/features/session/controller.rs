//! 隧道会话控制器模块
//!
//! 本模块处理 UI 与隧道之间的交互逻辑，包括：
//! - 启动/停止隧道的决策
//! - 异步操作的结果处理
//! - 托盘操作的转发

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

use super::password_gate::{
    connect_password_window_required_message, request_connect_password_action,
    ConnectPasswordAction,
};

/// 处理启动/停止按钮点击
///
/// 这是主 UI 入口点，会根据当前状态决定执行什么操作。
pub(crate) fn handle_start_stop(app: &mut WgApp, _window: &mut Window, cx: &mut Context<WgApp>) {
    let decision = current_toggle_decision(app, cx);
    apply_toggle_decision(app, decision, Some(_window), cx);
}

/// 核心启动/停止逻辑
///
/// 根据当前状态（busy、running、选中的配置等）决定操作。
pub(crate) fn handle_start_stop_core(app: &mut WgApp, cx: &mut Context<WgApp>) {
    let decision = current_toggle_decision(app, cx);
    apply_toggle_decision(app, decision, None, cx);
}

fn current_toggle_decision(app: &mut WgApp, cx: &mut Context<WgApp>) -> ToggleTunnelDecision {
    let draft = app.configs_draft_snapshot(cx);
    decide_toggle(ToggleTunnelInput {
        busy: app.runtime.busy,
        running: app.runtime.running,
        selected_config_id: app.selection.selected_id,
        running_config_id: app.runtime.running_id,
        draft_has_saved_source: draft.source_id.is_some(),
        draft_is_dirty: draft.is_dirty(),
        restart_delay: app.runtime.restart_delay(),
    })
}

fn apply_toggle_decision(
    app: &mut WgApp,
    decision: ToggleTunnelDecision,
    mut window: Option<&mut Window>,
    cx: &mut Context<WgApp>,
) {
    match decision {
        // 无操作：当前状态不支持任何操作
        ToggleTunnelDecision::Noop => {}

        // 排队等待启动：忙碌中但有新启动请求
        ToggleTunnelDecision::QueuePendingStart { config_id } => {
            if app.ui_prefs.require_connect_password {
                let Some(window) = window.as_deref_mut() else {
                    let message = connect_password_window_required_message();
                    app.set_error(message);
                    tray::notify_system("r-wg", message, true);
                    cx.notify();
                    return;
                };
                request_connect_password_action(
                    app,
                    ConnectPasswordAction::QueuePendingStart { config_id },
                    window,
                    cx,
                );
                return;
            }
            if app.runtime.queue_pending_start(Some(PendingStart {
                config_id,
                password_authorized: false,
            })) {
                app.set_status("Stopping... (queued start)");
                cx.notify();
            }
        }

        // 停止运行中的隧道
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

        // 启动选中的配置
        ToggleTunnelDecision::StartSelected {
            config_id,
            restart_delay,
        } => {
            if app.ui_prefs.require_connect_password {
                let Some(window) = window.as_deref_mut() else {
                    let message = connect_password_window_required_message();
                    app.set_error(message);
                    tray::notify_system("r-wg", message, true);
                    cx.notify();
                    return;
                };
                request_connect_password_action(
                    app,
                    ConnectPasswordAction::StartSelected {
                        config_id,
                        restart_delay,
                    },
                    window,
                    cx,
                );
                return;
            }
            start_config_by_id(
                app,
                config_id,
                restart_delay,
                cx,
                "Select a tunnel first",
                false,
            );
        }

        // 启动被阻止（显示错误消息）
        ToggleTunnelDecision::Blocked(reason) => {
            app.set_error(reason.message());
            cx.notify();
        }
    }
}

/// 处理从托盘启动隧道
pub(crate) fn handle_start_from_tray(app: &mut WgApp, cx: &mut Context<WgApp>) {
    if app.runtime.running || app.runtime.busy {
        return;
    }
    handle_start_stop_core(app, cx);
}

/// 处理从托盘停止隧道
pub(crate) fn handle_stop_from_tray(app: &mut WgApp, cx: &mut Context<WgApp>) {
    if !app.runtime.running || app.runtime.busy {
        return;
    }
    handle_start_stop_core(app, cx);
}

/// 启动指定配置
///
/// 这是启动操作的核心实现，处理：
/// 1. 权限检查
/// 2. 读取配置文本（如果未缓存）
/// 3. 调用 tunnel_session.start()
/// 4. 处理结果并更新 UI
fn start_with_config(
    app: &mut WgApp,
    selected: TunnelConfig,
    initial_text: Option<SharedString>,
    delay: Option<Duration>,
    cx: &mut Context<WgApp>,
) {
    // 权限检查
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
    let quantum_mode = app.ui_prefs.quantum_mode;
    let daita_mode = app.ui_prefs.daita_mode;

    cx.spawn(async move |view, cx| {
        // 如果指定了延迟，先等待
        if let Some(delay) = delay {
            cx.background_executor().timer(delay).await;
        }

        // 获取配置文本
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

        // 缓存配置文本
        let text_for_cache = text.clone();
        let path_for_cache = selected.storage_path.clone();
        view.update(cx, |this, _| {
            this.cache_config_text(path_for_cache, text_for_cache);
        })
        .ok();

        // 发起启动请求
        let request = StartTunnelRequest::new(
            selected.name.clone(),
            text.to_string(),
            dns_selection,
            quantum_mode,
            daita_mode,
        );
        let start_task = cx.background_spawn(async move {
            let outcome = tunnel_session.start(request);
            (
                outcome.result,
                outcome.apply_report,
                outcome.runtime_snapshot,
            )
        });
        let (result, apply_report, runtime_snapshot) = start_task.await;

        // 处理结果
        view.update(cx, |this, cx| {
            this.runtime.finish_start_attempt();
            match result {
                Ok(()) => {
                    this.runtime.mark_started(&selected);
                    if let Some(snapshot) = runtime_snapshot.as_ref() {
                        this.runtime
                            .set_last_apply_report(snapshot.apply_report.clone());
                        this.runtime.set_quantum_status(
                            snapshot.quantum_protected,
                            snapshot.last_quantum_failure,
                        );
                        this.runtime
                            .set_daita_status(snapshot.daita_active, snapshot.last_daita_failure);
                    } else {
                        this.runtime.set_last_apply_report(apply_report);
                        this.runtime.set_quantum_status(false, None);
                        this.runtime.set_daita_status(false, None);
                    }
                    this.refresh_configs_workspace_row_flags(cx);
                    this.stats.reset_for_start();
                    let status = format!("Running {}", selected.name);
                    let notification = format!("Tunnel connected: {}", selected.name);
                    this.set_status(status);
                    tray::notify_system("r-wg", &notification, false);
                    this.start_stats_polling(cx);
                }
                Err(err) => {
                    if let Some(snapshot) = runtime_snapshot.as_ref() {
                        this.runtime
                            .set_last_apply_report(snapshot.apply_report.clone());
                        this.runtime
                            .set_quantum_status(false, snapshot.last_quantum_failure);
                        this.runtime
                            .set_daita_status(false, snapshot.last_daita_failure);
                    } else {
                        this.runtime.set_last_apply_report(apply_report);
                        this.runtime.set_quantum_status(false, None);
                        this.runtime.set_daita_status(false, None);
                    }
                    let negotiation_failure = runtime_snapshot
                        .as_ref()
                        .and_then(|snapshot| {
                            snapshot.last_quantum_failure.or(snapshot.last_daita_failure)
                        });
                    let message = format_start_failure(&err, negotiation_failure);
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

/// 根据配置 ID 启动
///
/// 查找配置并调用 start_with_config。
pub(crate) fn start_config_by_id(
    app: &mut WgApp,
    config_id: u64,
    delay: Option<Duration>,
    cx: &mut Context<WgApp>,
    missing_message: &str,
    password_authorized: bool,
) {
    if app.ui_prefs.require_connect_password && !password_authorized {
        app.set_error(connect_password_window_required_message());
        cx.notify();
        return;
    }
    let Some(selected) = app.configs.find_by_id(config_id) else {
        app.set_error(missing_message.to_string());
        cx.notify();
        return;
    };
    let cached_text = app.cached_config_text(&selected.storage_path);
    let initial_text = selected.text.clone().or(cached_text);
    start_with_config(app, selected, initial_text, delay, cx);
}

/// 重启待处理的启动
///
/// 停止成功后检查是否有排队的启动请求。
fn restart_pending_start(app: &mut WgApp, cx: &mut Context<WgApp>) {
    let pending_start = app.runtime.pending_start.take();
    let pending_config_id = pending_start.map(|pending| pending.config_id);
    match decide_after_stop_success(pending_config_id) {
        StopSuccessDecision::RestartPending { config_id } => {
            let delay = app.runtime.restart_delay();
            let password_authorized = pending_start
                .map(|pending| pending.password_authorized)
                .unwrap_or(false);
            start_config_by_id(
                app,
                config_id,
                delay,
                cx,
                "Pending start config not found",
                password_authorized,
            );
        }
        StopSuccessDecision::Idle => {}
    }
}

/// 完成停止成功
///
/// 更新 UI 状态为"已停止"。
pub(crate) fn complete_stop_success(app: &mut WgApp, cx: &mut Context<WgApp>) {
    app.runtime.finish_stop_success();
    app.refresh_configs_workspace_row_flags(cx);
    app.stats.clear_runtime_metrics();
    app.set_status("Stopped");
}

/// 完成停止失败
///
/// 显示错误消息。
pub(crate) fn complete_stop_failure(app: &mut WgApp, message: impl Into<SharedString>) {
    app.runtime.finish_stop_failure();
    app.set_error(message);
}

fn format_start_failure(
    err: &impl std::fmt::Display,
    negotiation_failure: Option<r_wg::backend::wg::EphemeralFailureKind>,
) -> String {
    match negotiation_failure {
        Some(kind) => format!("Start failed ({kind}): {err}"),
        None => format!("Start failed: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use r_wg::backend::wg::EphemeralFailureKind;

    use super::format_start_failure;

    #[test]
    fn format_start_failure_includes_quantum_failure_kind() {
        let message = format_start_failure(&"timed out", Some(EphemeralFailureKind::Timeout));

        assert_eq!(
            message,
            "Start failed (ephemeral peer negotiation timeout): timed out"
        );
    }
}
