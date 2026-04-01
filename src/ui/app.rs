// UI 入口：负责应用初始化、窗口创建以及关闭时的隧道清理。
use gpui::*;
use gpui_component::Root;
use gpui_component_assets::Assets;
// Engine 负责隧道生命周期；EngineError 用于区分停止时的错误类型。
use r_wg::application::TunnelSessionService;
use r_wg::backend::wg::Engine;
// 关闭流程需要跨异步任务共享状态，因此使用原子布尔标记。
#[cfg(not(target_os = "windows"))]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(not(target_os = "windows"))]
use std::sync::Arc;

use super::features::{
    session::lifecycle,
    themes::{self, AppearancePolicy},
};
use super::persistence;
use super::single_instance::PrimaryInstance;
use super::state::WgApp;
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

pub fn run(primary: PrimaryInstance) {
    // 在 UI 启动前先创建后端引擎，供整个应用生命周期复用。
    let tunnel_session = TunnelSessionService::new(Engine::new());

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

            let tunnel_session = tunnel_session.clone();
            let primary = primary;
            cx.open_window(
                WindowOptions {
                    app_id: Some("r-wg".to_string()),
                    ..WindowOptions::default()
                },
                move |window, cx| {
                    // 创建应用状态并挂到窗口根视图。
                    let startup_theme = startup_theme.clone();
                    let view_tunnel_session = tunnel_session.clone();
                    let view = cx.new(move |_cx| {
                        WgApp::new(
                            view_tunnel_session.clone(),
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
                    lifecycle::sync_apply_report(view_handle.clone(), tunnel_session.clone(), cx);
                    // 启动期向引擎反查一次状态，兼容 helper 已在运行而 UI 后打开的场景。
                    #[cfg(target_os = "windows")]
                    lifecycle::sync_engine_status(view_handle.clone(), tunnel_session.clone(), cx);
                    // 初始化系统托盘并启动命令监听。
                    tray::init(
                        primary.clone(),
                        window.window_handle(),
                        view_handle.clone(),
                        tunnel_session.clone(),
                        cx,
                    );
                    // 防止重复触发关闭逻辑（多次点击关闭按钮）。
                    #[cfg(not(target_os = "windows"))]
                    let closing = Arc::new(AtomicBool::new(false));
                    #[cfg(not(target_os = "windows"))]
                    let close_tunnel_session = tunnel_session.clone();
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

                            // 保存窗口句柄，供异步停止完成后关闭窗口。
                            let handle = window.window_handle();
                            // 克隆需要跨异步任务使用的引用/标记。
                            let view_handle = view_handle.clone();
                            let close_flag = Arc::clone(&close_flag);
                            let tunnel_session = close_tunnel_session.clone();
                            // 在 UI 线程启动异步任务，后台线程执行 engine.stop()。
                            cx.spawn(async move |cx| {
                                let should_close = lifecycle::request_shutdown_stop(
                                    view_handle.clone(),
                                    tunnel_session,
                                    cx,
                                )
                                .await;
                                if should_close {
                                    // 停止成功（或已停止/通道关闭）：更新 UI 并关闭窗口。
                                    // 实际移除窗口，触发关闭。
                                    let _ =
                                        handle.update(cx, |_, window, _| window.remove_window());
                                } else {
                                    // 停止失败：保留窗口，允许再次尝试关闭。
                                    close_flag.store(false, Ordering::SeqCst);
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
