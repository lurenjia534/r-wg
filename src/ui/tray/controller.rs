use gpui::{AnyWindowHandle, App, Global, UpdateGlobal};
use r_wg::backend::wg::Engine;
use std::sync::{mpsc, Arc, Mutex};

use crate::ui::features::session::lifecycle;
use crate::ui::single_instance::PrimaryInstance;
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
    primary: PrimaryInstance,
    window_handle: AnyWindowHandle,
    view: gpui::WeakEntity<WgApp>,
    engine: Engine,
    cx: &mut App,
) {
    let (tx, rx) = mpsc::channel();
    let tray_enabled = platform::spawn_tray_thread(tx.clone());
    TrayState::set_global(
        cx,
        TrayState {
            enabled: tray_enabled,
        },
    );
    attach_activation_bridge(&primary, tx);

    start_command_loop(primary, rx, window_handle, view, engine, cx);
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
    primary: PrimaryInstance,
    rx: mpsc::Receiver<TrayCommand>,
    window_handle: AnyWindowHandle,
    view: gpui::WeakEntity<WgApp>,
    engine: Engine,
    cx: &mut App,
) {
    let rx = Arc::new(Mutex::new(rx));
    let view_handle = view.clone();
    let engine_handle = engine.clone();

    cx.spawn(async move |cx| {
        let _single_instance_guard = primary;
        loop {
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

fn attach_activation_bridge(primary: &PrimaryInstance, tx: mpsc::Sender<TrayCommand>) {
    primary.attach(move || {
        let _ = tx.send(TrayCommand::ShowWindow);
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
        let should_quit = lifecycle::request_shutdown_stop(view, engine, cx).await;
        if should_quit {
            platform::shutdown_tray();
            let _ = cx.update(|app| app.quit());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::{attach_activation_bridge, TrayCommand};
    use crate::ui::single_instance::PrimaryInstance;

    #[test]
    fn activation_bridge_sends_show_window_even_without_tray_thread() {
        let primary = PrimaryInstance::new_for_tests();
        let (tx, rx) = mpsc::channel();

        // This covers the "tray init failed" path: the bridge must still exist even when the
        // tray thread never produced commands of its own.
        attach_activation_bridge(&primary, tx);
        primary.trigger_activate_for_tests();

        let command = rx
            .recv_timeout(Duration::from_millis(100))
            .expect("activation bridge should emit ShowWindow");
        assert!(matches!(command, TrayCommand::ShowWindow));
    }
}
