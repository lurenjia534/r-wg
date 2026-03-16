use gpui::{AnyWindowHandle, App, Global, UpdateGlobal};
use r_wg::backend::wg::Engine;
#[cfg(not(target_os = "windows"))]
use r_wg::backend::wg::EngineError;
use std::sync::{mpsc, Arc, Mutex};

use crate::ui::state::WgApp;

use super::{platform, types::TrayCommand};

/// 托盘可用状态（写入 GPUI 全局）。
///
/// `enabled = true` 代表托盘初始化成功，此时窗口关闭按钮应走“最小化到托盘”。
#[derive(Default)]
struct TrayState {
    enabled: bool,
}

impl Global for TrayState {}

/// 初始化托盘 controller。
///
/// 负责：
/// - 创建命令通道；
/// - 请求平台层启动托盘线程；
/// - 在成功时挂起命令消费循环；
/// - 在失败时把应用降级为“无托盘模式”。
pub(super) fn init(
    window_handle: AnyWindowHandle,
    view: gpui::WeakEntity<WgApp>,
    engine: Engine,
    cx: &mut App,
) {
    let (tx, rx) = mpsc::channel();
    if platform::spawn_tray_thread(tx) {
        TrayState::set_global(cx, TrayState { enabled: true });
        start_command_loop(rx, window_handle, view, engine, cx);
        return;
    }

    TrayState::set_global(cx, TrayState { enabled: false });
}

/// 判断关闭窗口时是否应拦截为“最小化到托盘”。
pub(super) fn should_minimize_on_close(cx: &App) -> bool {
    cx.try_global::<TrayState>()
        .map(|state| state.enabled)
        .unwrap_or(false)
}

/// 托盘命令消费循环。
///
/// 数据流：
/// 1. 平台线程通过 `mpsc::Sender<TrayCommand>` 发送命令；
/// 2. 此循环在 UI 异步上下文中接收命令；
/// 3. 按命令分发到 UI 状态更新或后端引擎调用。
fn start_command_loop(
    rx: mpsc::Receiver<TrayCommand>,
    window_handle: AnyWindowHandle,
    view: gpui::WeakEntity<WgApp>,
    engine: Engine,
    cx: &mut App,
) {
    let rx = Arc::new(Mutex::new(rx));
    let view_handle = view.clone();
    let engine_handle = engine.clone();

    cx.spawn(async move |cx| loop {
        let rx = rx.clone();
        let cmd = cx
            .background_executor()
            .spawn(async move { rx.lock().ok()?.recv().ok() })
            .await;
        let Some(cmd) = cmd else { break };

        match cmd {
            TrayCommand::ShowWindow => {
                focus_main_window(window_handle, cx);
            }
            TrayCommand::StartTunnel => {
                let _ = view_handle.update(cx, |this, cx| {
                    this.handle_start_from_tray(cx);
                });
            }
            TrayCommand::StopTunnel => {
                let _ = view_handle.update(cx, |this, cx| {
                    this.handle_stop_from_tray(cx);
                });
            }
            TrayCommand::QuitApp => {
                request_quit(view_handle.clone(), engine_handle.clone(), cx).await;
            }
        }
    })
    .detach();
}

/// 将主窗口带回前台。
///
/// 说明：
/// - `platform::show_window` 负责平台级显示动作；
/// - `activate_window` 负责焦点激活，提升“点托盘立即可见”的一致性。
fn focus_main_window(window_handle: AnyWindowHandle, cx: &mut gpui::AsyncApp) {
    let _ = window_handle.update(cx, |_, window, _| {
        platform::show_window(window);
        window.activate_window();
    });
}

/// 处理“从托盘退出应用”。
///
/// 退出策略：
/// - 先尝试停止隧道；
/// - 对“已经停止/通道已关闭”视为可退出；
/// - 停止失败时保留应用并在 UI 显示错误。
async fn request_quit(view: gpui::WeakEntity<WgApp>, engine: Engine, cx: &mut gpui::AsyncApp) {
    #[cfg(target_os = "windows")]
    {
        let _ = view;
        let _ = engine;
        platform::shutdown_tray();
        let _ = cx.update(|app| app.quit());
        return;
    }

    #[cfg(not(target_os = "windows"))]
    {
        let mut was_running = false;
        let _ = view.update(cx, |this, cx| {
            was_running = this.runtime.running;
            if this.runtime.running {
                this.runtime.busy = true;
                this.set_status("Stopping...");
                cx.notify();
            }
        });

        let result = cx
            .background_executor()
            .spawn(async move { engine.stop() })
            .await;
        let should_quit = matches!(
            &result,
            Ok(()) | Err(EngineError::NotRunning) | Err(EngineError::ChannelClosed)
        );

        let _ = view.update(cx, |this, cx| {
            if should_quit {
                if was_running {
                    this.runtime.finish_stop_success();
                    this.stats.clear_runtime_metrics();
                    this.set_status("Stopped");
                }
            } else if let Err(err) = result {
                if was_running {
                    this.runtime.busy = false;
                }
                this.set_error(format!("Stop failed: {err}"));
            }
            cx.notify();
        });

        if should_quit {
            platform::shutdown_tray();
            let _ = cx.update(|app| app.quit());
        }
    }
}
