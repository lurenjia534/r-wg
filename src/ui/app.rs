//! GPUI 应用入口模块
//!
//! 本模块负责：
//! 1. GPUI 应用的初始化
//! 2. 主窗口的创建
//! 3. 主题系统的加载
//! 4. 系统托盘的初始化
//! 5. 窗口关闭时的隧道清理
//!
//! # 启动流程
//!
//! ```text
//! main()
//!   └── run(primary)
//!         ├── 创建 TunnelSessionService (Engine)
//!         ├── Application::new().run()
//!         │     └── GPUI 事件循环
//!         │           ├── 加载主题偏好
//!         │           ├── 创建窗口
//!         │           │     ├── 创建 WgApp 状态
//!         │           │     ├── 初始化托盘
//!         │           │     └── 注册关闭处理器
//!         │           └── Root::new() 挂载根视图
//!         └── 窗口关闭时清理隧道
//! ```
//!
//! # 平台差异
//!
//! - **Windows**: 关闭窗口直接退出（由 SCM 服务管理）
//! - **Linux**: 关闭窗口时需要停止隧道（使用 systemd service）

use gpui::*;
use gpui_component::Root;
use gpui_component_assets::Assets;

use r_wg::application::TunnelSessionService;
use r_wg::backend::wg::Engine;

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

/// 启动时的主题偏好设置
///
/// 包含亮色/暗色主题的键名和已解析的主题配置。
#[derive(Clone)]
struct StartupThemePrefs {
    /// 外观策略：跟随系统、始终亮色或始终暗色
    appearance_policy: AppearancePolicy,
    /// 解析后的主题模式
    resolved_mode: gpui_component::theme::ThemeMode,
    /// 亮色主题的键名
    light_key: Option<SharedString>,
    /// 暗色主题的键名
    dark_key: Option<SharedString>,
    /// 亮色主题的名称
    light_name: Option<SharedString>,
    /// 暗色主题的名称
    dark_name: Option<SharedString>,
}

/// 应用主入口函数
///
/// 创建 GPUI 应用、初始化主题、创建窗口并运行事件循环。
///
/// # 参数
/// * `primary` - 单实例锁的主实例句柄
pub fn run(primary: PrimaryInstance) {
    // 在 UI 启动前先创建后端引擎，供整个应用生命周期复用。
    // Engine 运行在独立线程中，不阻塞 UI。
    let tunnel_session = TunnelSessionService::new(Engine::new());

    Application::new()
        .with_assets(Assets)
        .run(move |cx: &mut App| {
            // 初始化 GPUI 组件库
            gpui_component::init(cx);
            
            // 打开窗口前加载主题选择，确保首帧尽量贴近用户偏好。
            let mut startup_theme = load_startup_theme_prefs(cx);
            let storage = persistence::ensure_storage_dirs().ok();
            if let Some(storage) = storage.as_ref() {
                let _ = themes::ensure_theme_registry(storage, cx);
            }
            
            // 解析亮色主题
            let light_theme = themes::resolve_theme_preference(
                gpui_component::theme::ThemeMode::Light,
                startup_theme.light_key.as_deref().map(|key| &**key),
                startup_theme.light_name.as_deref().map(|name| &**name),
                storage.as_ref(),
                cx,
            );
            
            // 解析暗色主题
            let dark_theme = themes::resolve_theme_preference(
                gpui_component::theme::ThemeMode::Dark,
                startup_theme.dark_key.as_deref().map(|key| &**key),
                startup_theme.dark_name.as_deref().map(|name| &**name),
                storage.as_ref(),
                cx,
            );
            
            // 更新主题偏好
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
            
            // 创建主窗口
            cx.open_window(
                WindowOptions {
                    app_id: Some("r-wg".to_string()),
                    ..WindowOptions::default()
                },
                move |window, cx| {
                    // 创建应用状态并挂到窗口根视图
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
                    
                    // 刷新特权后端状态
                    view.update(cx, |this, cx| {
                        this.refresh_privileged_backend_status(cx);
                    });
                    
                    // 弱引用：窗口关闭后不会阻止资源释放
                    let view_handle = view.downgrade();
                    
                    // 同步路由应用报告
                    lifecycle::sync_apply_report(view_handle.clone(), tunnel_session.clone(), cx);
                    
                    // Windows 专用：启动期向引擎反查状态
                    // 兼容 helper 已在运行而 UI 后打开的场景
                    #[cfg(target_os = "windows")]
                    lifecycle::sync_engine_status(view_handle.clone(), tunnel_session.clone(), cx);
                    
                    // 初始化系统托盘并启动命令监听
                    tray::init(
                        primary.clone(),
                        window.window_handle(),
                        view_handle.clone(),
                        tunnel_session.clone(),
                        cx,
                    );
                    
                    // Linux 专用：防止重复触发关闭逻辑
                    #[cfg(not(target_os = "windows"))]
                    let closing = Arc::new(AtomicBool::new(false));
                    #[cfg(not(target_os = "windows"))]
                    let close_tunnel_session = tunnel_session.clone();
                    #[cfg(not(target_os = "windows"))]
                    let close_flag = Arc::clone(&closing);

                    // 拦截窗口关闭事件
                    window.on_window_should_close(cx, move |window, cx| {
                        // 如果托盘启用，最小化到托盘而不是关闭
                        if tray::should_minimize_on_close(cx) {
                            tray::hide_window(window);
                            return false;
                        }

                        #[cfg(target_os = "windows")]
                        {
                            // Windows 上直接关闭，让 SCM 服务处理清理
                            return true;
                        }

                        #[cfg(not(target_os = "windows"))]
                        {
                            // 已经进入关闭流程则直接拒绝本次关闭请求
                            if close_flag.swap(true, Ordering::SeqCst) {
                                return false;
                            }

                            // 保存窗口句柄，供异步停止完成后关闭窗口
                            let handle = window.window_handle();
                            let view_handle = view_handle.clone();
                            let close_flag = Arc::clone(&close_flag);
                            let tunnel_session = close_tunnel_session.clone();
                            
                            // 在 UI 线程启动异步任务，后台线程执行 engine.stop()
                            cx.spawn(async move |cx| {
                                let should_close = lifecycle::request_shutdown_stop(
                                    view_handle.clone(),
                                    tunnel_session,
                                    cx,
                                )
                                .await;
                                if should_close {
                                    // 停止成功，更新 UI 并关闭窗口
                                    let _ =
                                        handle.update(cx, |_, window, _| window.remove_window());
                                } else {
                                    // 停止失败：保留窗口，允许再次尝试关闭
                                    close_flag.store(false, Ordering::SeqCst);
                                }
                            })
                            .detach();

                            // 返回 false 阻止立即关闭，等待异步 stop 完成后手动关闭
                            false
                        }
                    });

                    cx.new(|cx| Root::new(view, window, cx))
                },
            )
            .unwrap();
        });
}

/// 加载启动时的主题偏好设置
///
/// 从持久化存储中读取用户之前选择的主题设置。
/// 如果读取失败或文件不存在，默认使用“跟随系统”策略，
/// 并以深色 `resolved_mode` 作为启动阶段的兜底值。
///
/// # 返回
/// * `StartupThemePrefs` - 包含外观策略和亮/暗主题的键名和名称
fn load_startup_theme_prefs(_cx: &App) -> StartupThemePrefs {
    // 读取持久化主题；读取失败则回退到“跟随系统 + 深色启动兜底”
    let storage = match persistence::ensure_storage_dirs() {
        Ok(storage) => storage,
        Err(_) => {
            // 存储目录创建失败，返回默认主题偏好
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
    
    // 尝试加载之前保存的状态
    let state = persistence::load_state(&storage).ok().flatten();
    
    // 确定外观策略：优先使用 theme_policy，否则尝试 theme_mode，最后默认 System
    let appearance_policy = state
        .as_ref()
        .and_then(|state| state.theme_policy)
        .or_else(|| {
            state
                .as_ref()
                .and_then(|state| state.theme_mode.map(Into::into))
        })
        .unwrap_or(AppearancePolicy::System);
        
    // 构建完整的主题偏好
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
