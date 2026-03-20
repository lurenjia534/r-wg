// UI 入口：负责应用初始化、窗口创建以及关闭时的隧道清理。
use gpui::*;
use gpui_component::Root;
use gpui_component_assets::Assets;
// Engine 负责隧道生命周期；EngineError 用于区分停止时的错误类型。
use r_wg::backend::wg::Engine;
#[cfg(not(target_os = "windows"))]
use r_wg::backend::wg::EngineError;
// 关闭流程需要跨异步任务共享状态，因此使用原子布尔标记。
#[cfg(not(target_os = "windows"))]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(not(target_os = "windows"))]
use std::sync::Arc;

use super::persistence;
use super::state::WgApp;
use super::themes::{self, AppearancePolicy};
use super::tray;

#[derive(Clone)]
struct StartupThemePrefs {
    appearance_policy: AppearancePolicy,
    resolved_mode: gpui_component::theme::ThemeMode,
    light_key: Option<SharedString>,
    dark_key: Option<SharedString>,
    light_name: Option<SharedString>,
    dark_name: Option<SharedString>,
}

pub fn run() {
    // 在 UI 启动前先创建后端引擎，供整个应用生命周期复用。
    let engine = Engine::new();

    Application::new()
        .with_assets(Assets)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);
            // 打开窗口前加载主题选择，确保首帧尽量贴近用户偏好。
            let mut startup_theme = load_startup_theme_prefs(cx);
            let storage = persistence::ensure_storage_dirs().ok();
            if let Some(storage) = storage.as_ref() {
                let _ = themes::ensure_theme_registry(storage, cx);
            }
            let light_theme = themes::resolve_theme_preference(
                gpui_component::theme::ThemeMode::Light,
                startup_theme.light_key.as_deref().map(|key| &**key),
                startup_theme.light_name.as_deref().map(|name| &**name),
                storage.as_ref(),
                cx,
            );
            let dark_theme = themes::resolve_theme_preference(
                gpui_component::theme::ThemeMode::Dark,
                startup_theme.dark_key.as_deref().map(|key| &**key),
                startup_theme.dark_name.as_deref().map(|name| &**name),
                storage.as_ref(),
                cx,
            );
            startup_theme.light_key = Some(light_theme.entry.key.clone());
            startup_theme.dark_key = Some(dark_theme.entry.key.clone());
            startup_theme.light_name = Some(light_theme.entry.name.clone());
            startup_theme.dark_name = Some(dark_theme.entry.name.clone());
            startup_theme.resolved_mode = themes::apply_resolved_theme_preferences(
                startup_theme.appearance_policy,
                light_theme.entry.config.clone(),
                dark_theme.entry.config.clone(),
                None,
                cx,
            );

            let engine = engine.clone();
            cx.open_window(
                WindowOptions {
                    app_id: Some("r-wg".to_string()),
                    ..WindowOptions::default()
                },
                move |window, cx| {
                    // 创建应用状态并挂到窗口根视图。
                    let startup_theme = startup_theme.clone();
                    let view_engine = engine.clone();
                    let view = cx.new(move |_cx| {
                        WgApp::new(
                            view_engine.clone(),
                            startup_theme.appearance_policy,
                            startup_theme.resolved_mode,
                            startup_theme.light_key.clone(),
                            startup_theme.dark_key.clone(),
                            startup_theme.light_name.clone(),
                            startup_theme.dark_name.clone(),
                        )
                    });
                    view.update(cx, |this, cx| {
                        this.refresh_privileged_backend_status(cx);
                    });
                    // 弱引用：窗口关闭后不会阻止资源释放。
                    let view_handle = view.downgrade();
                    // 启动期向引擎反查一次状态，兼容 helper 已在运行而 UI 后打开的场景。
                    #[cfg(target_os = "windows")]
                    sync_engine_status(view_handle.clone(), engine.clone(), cx);
                    // 初始化系统托盘并启动命令监听。
                    tray::init(
                        window.window_handle(),
                        view_handle.clone(),
                        engine.clone(),
                        cx,
                    );
                    // 防止重复触发关闭逻辑（多次点击关闭按钮）。
                    #[cfg(not(target_os = "windows"))]
                    let closing = Arc::new(AtomicBool::new(false));
                    #[cfg(not(target_os = "windows"))]
                    let close_engine = engine.clone();
                    #[cfg(not(target_os = "windows"))]
                    let close_flag = Arc::clone(&closing);

                    // 拦截窗口关闭：托盘启用时仅隐藏窗口，否则停隧道并关闭窗口。
                    window.on_window_should_close(cx, move |window, cx| {
                        if tray::should_minimize_on_close(cx) {
                            tray::hide_window(window);
                            return false;
                        }

                        #[cfg(target_os = "windows")]
                        {
                            return true;
                        }

                        #[cfg(not(target_os = "windows"))]
                        {
                            // 已经进入关闭流程则直接拒绝本次关闭请求。
                            if close_flag.swap(true, Ordering::SeqCst) {
                                return false;
                            }

                            // 记录是否正在运行，并同步 UI 提示为“正在停止”。
                            let mut was_running = false;
                            if let Some(view) = view_handle.upgrade() {
                                view.update(cx, |this, cx| {
                                    was_running = this.runtime.running;
                                    if this.runtime.running {
                                        this.runtime.busy = true;
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
                                let result =
                                    cx.background_spawn(async move { engine.stop() }).await;
                                match result {
                                    Ok(())
                                    | Err(EngineError::NotRunning)
                                    | Err(EngineError::ChannelClosed) => {
                                        // 停止成功（或已停止/通道关闭）：更新 UI 并关闭窗口。
                                        if was_running {
                                            if let Some(view) = view_handle.upgrade() {
                                                let _ = view.update(cx, |this, cx| {
                                                    this.runtime.finish_stop_success();
                                                    this.stats.clear_runtime_metrics();
                                                    this.set_status("Stopped");
                                                    cx.notify();
                                                });
                                            }
                                        }
                                        // 实际移除窗口，触发关闭。
                                        let _ = handle
                                            .update(cx, |_, window, _| window.remove_window());
                                    }
                                    Err(err) => {
                                        // 停止失败：保留窗口，提示错误，允许再次尝试关闭。
                                        if let Some(view) = view_handle.upgrade() {
                                            let _ = view.update(cx, |this, cx| {
                                                if was_running {
                                                    this.runtime.busy = false;
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
                        }
                    });

                    cx.new(|cx| Root::new(view, window, cx))
                },
            )
            .unwrap();
        });
}

/// 启动窗口后同步一次后端状态。
///
/// 这里主要处理 Windows helper 已经在运行、但 UI 是后打开的情况。
/// 此时我们只能确认“当前确实有隧道在跑”，未必能拿到精确配置项，
/// 所以先恢复为通用运行态，再启动统计轮询。
#[cfg(target_os = "windows")]
fn sync_engine_status(view: gpui::WeakEntity<WgApp>, engine: Engine, cx: &mut App) {
    cx.spawn(async move |cx| {
        let result = cx.background_spawn(async move { engine.status() }).await;
        let _ = view.update(cx, |this, cx| {
            if !matches!(result, Ok(r_wg::backend::wg::EngineStatus::Running)) {
                return;
            }
            this.runtime.running = true;
            this.runtime.busy = false;
            // helper 恢复场景下不一定拿得到原始配置名，先放通用占位避免 UI 空白。
            if this.runtime.running_name.is_none() {
                this.runtime.running_name = Some("Tunnel".to_string());
            }
            // 这里只恢复运行态与统计轮询，不推断具体配置来源。
            this.set_status("Tunnel running");
            this.stats.reset_for_start();
            this.start_stats_polling(cx);
            cx.notify();
        });
    })
    .detach();
}
fn load_startup_theme_prefs(_cx: &App) -> StartupThemePrefs {
    // 读取持久化主题；读取失败则回退深色模式与默认 palette。
    let storage = match persistence::ensure_storage_dirs() {
        Ok(storage) => storage,
        Err(_) => {
            return StartupThemePrefs {
                appearance_policy: AppearancePolicy::System,
                resolved_mode: gpui_component::theme::ThemeMode::Dark,
                light_key: None,
                dark_key: None,
                light_name: None,
                dark_name: None,
            };
        }
    };
    let state = persistence::load_state(&storage).ok().flatten();
    let appearance_policy = state
        .as_ref()
        .and_then(|state| state.theme_policy)
        .or_else(|| {
            state
                .as_ref()
                .and_then(|state| state.theme_mode.map(Into::into))
        })
        .unwrap_or(AppearancePolicy::System);
    StartupThemePrefs {
        appearance_policy,
        resolved_mode: match appearance_policy {
            AppearancePolicy::Light => gpui_component::theme::ThemeMode::Light,
            AppearancePolicy::Dark => gpui_component::theme::ThemeMode::Dark,
            AppearancePolicy::System => gpui_component::theme::ThemeMode::Dark,
        },
        light_key: state
            .as_ref()
            .and_then(|state| state.theme_light_key.clone())
            .map(Into::into),
        dark_key: state
            .as_ref()
            .and_then(|state| state.theme_dark_key.clone())
            .map(Into::into),
        light_name: state
            .as_ref()
            .and_then(|state| state.theme_light_name.clone())
            .map(Into::into),
        dark_name: state
            .as_ref()
            .and_then(|state| state.theme_dark_name.clone())
            .map(Into::into),
    }
}
