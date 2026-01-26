// UI 入口：负责应用初始化、窗口创建以及关闭时的隧道清理。
use gpui::*;
use gpui_component::theme::{Theme, ThemeMode};
use gpui_component::Root;
use gpui_component_assets::Assets;
// Engine 负责隧道生命周期；EngineError 用于区分停止时的错误类型。
use r_wg::backend::wg::{Engine, EngineError};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    // 关闭流程需要跨异步任务共享状态，因此使用原子布尔标记。
    Arc,
};

use super::persistence;
use super::state::WgApp;

pub fn run() {
    // 在 UI 启动前先创建后端引擎，供整个应用生命周期复用。
    let engine = Engine::new();

    Application::new()
        .with_assets(Assets)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);
            // 打开窗口前加载主题选择，确保首帧使用正确主题。
            let theme_mode = load_persisted_theme_mode();
            Theme::change(theme_mode, None, cx);

            let engine = engine.clone();
            cx.open_window(WindowOptions::default(), move |window, cx| {
                // 创建应用状态并挂到窗口根视图。
                let view = cx.new(|_cx| WgApp::new(engine.clone(), theme_mode));
                // 弱引用：窗口关闭后不会阻止资源释放。
                let view_handle = view.downgrade();
                // 防止重复触发关闭逻辑（多次点击关闭按钮）。
                let closing = Arc::new(AtomicBool::new(false));
                let close_engine = engine.clone();
                let close_flag = Arc::clone(&closing);

                // 拦截窗口关闭：先停隧道并回滚 DNS，再真正关闭窗口。
                window.on_window_should_close(cx, move |window, cx| {
                    // 已经进入关闭流程则直接拒绝本次关闭请求。
                    if close_flag.swap(true, Ordering::SeqCst) {
                        return false;
                    }

                    // 记录是否正在运行，并同步 UI 提示为“正在停止”。
                    let mut was_running = false;
                    if let Some(view) = view_handle.upgrade() {
                        view.update(cx, |this, cx| {
                            was_running = this.running;
                            if this.running {
                                this.busy = true;
                                this.set_status("Stopping...");
                                cx.notify();
                            }
                        });
                    }

                    // 保存窗口句柄，供异步停止完成后关闭窗口。
                    let handle = window.window_handle();
                    // 克隆需要跨异步任务使用的引用/标记。
                    let view_handle = view_handle.clone();
                    let close_flag = Arc::clone(&close_flag);
                    let engine = close_engine.clone();
                    // 在 UI 线程启动异步任务，后台线程执行 engine.stop()。
                    cx.spawn(async move |cx| {
                        let result = cx.background_spawn(async move { engine.stop() }).await;
                        match result {
                            Ok(())
                            | Err(EngineError::NotRunning)
                            | Err(EngineError::ChannelClosed) => {
                                // 停止成功（或已停止/通道关闭）：更新 UI 并关闭窗口。
                                if was_running {
                                    if let Some(view) = view_handle.upgrade() {
                                        let _ = view.update(cx, |this, cx| {
                                            this.busy = false;
                                            this.running = false;
                                            this.running_name = None;
                                            this.running_id = None;
                                            this.started_at = None;
                                            this.clear_stats();
                                            this.set_status("Stopped");
                                            cx.notify();
                                        });
                                    }
                                }
                                // 实际移除窗口，触发关闭。
                                let _ = handle.update(cx, |_, window, _| window.remove_window());
                            }
                            Err(err) => {
                                // 停止失败：保留窗口，提示错误，允许再次尝试关闭。
                                if let Some(view) = view_handle.upgrade() {
                                    let _ = view.update(cx, |this, cx| {
                                        if was_running {
                                            this.busy = false;
                                        }
                                        this.set_error(format!("Stop failed: {err}"));
                                        cx.notify();
                                    });
                                }
                                close_flag.store(false, Ordering::SeqCst);
                            }
                        }
                    })
                    .detach();

                    // 返回 false 阻止立即关闭，等待异步 stop 完成后手动关闭窗口。
                    false
                });

                cx.new(|cx| Root::new(view, window, cx))
            })
            .unwrap();
        });
}

fn load_persisted_theme_mode() -> ThemeMode {
    // 读取持久化主题；读取失败则回退深色模式。
    let storage = match persistence::ensure_storage_dirs() {
        Ok(storage) => storage,
        Err(_) => return ThemeMode::Dark,
    };
    persistence::load_state(&storage)
        .ok()
        .flatten()
        .and_then(|state| state.theme_mode)
        .unwrap_or(ThemeMode::Dark)
}
