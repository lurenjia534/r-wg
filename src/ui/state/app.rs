//! WgApp 状态管理模块
//!
//! 本模块定义了 WgApp 结构体，它是整个 UI 应用状态的中心管理者。
//! WgApp 协调多个子模块的状态，并提供统一的访问接口。
//!
//! # 状态子模块
//!
//! - `services`: 应用服务聚合（隧道会话、后端管理、配置库）
//! - `configs`: 配置列表和当前编辑状态
//! - `selection`: 当前选中的配置
//! - `runtime`: 隧道运行状态（运行中/空闲/忙碌）
//! - `stats`: 流量统计
//! - `persistence`: 持久化状态
//! - `ui_prefs`: UI 用户偏好（主题、面板宽度等）
//! - `ui_session`: UI 会话状态（当前页面、侧边栏状态等）
//! - `ui`: UI 内部状态（搜索、临时数据等）

use gpui::SharedString;
use gpui_component::theme::ThemeMode;
use r_wg::application::{BackendAdminService, ConfigLibraryService, TunnelSessionService};

use crate::ui::features::themes::AppearancePolicy;
use crate::ui::i18n::LanguagePreference;

use super::{
    ConfigsState, PersistenceState, RuntimeState, SelectionState, StatsState, UiPrefsState,
    UiSessionState, UiState,
};

/// UI root 持有的应用服务集合。
#[derive(Clone)]
pub(crate) struct AppServices {
    /// 隧道会话服务，用于与后端引擎交互
    pub(crate) tunnel_session: TunnelSessionService,
    /// 特权后端管理服务，用于安装/移除后端服务
    pub(crate) backend_admin: BackendAdminService,
    /// 配置库服务，用于管理配置文件的 CRUD 操作
    pub(crate) config_library: ConfigLibraryService,
}

impl AppServices {
    fn new(tunnel_session: TunnelSessionService) -> Self {
        Self {
            tunnel_session,
            backend_admin: BackendAdminService::new(),
            config_library: ConfigLibraryService::new(),
        }
    }
}

/// WgApp 结构体
///
/// 这是 GPUI 应用的主状态容器，包含所有子模块的状态和服务。
/// 它提供了大量 helper 方法用于查询和修改各种 UI 状态。
pub(crate) struct WgApp {
    /// 应用服务聚合，避免 root state 顶层字段继续膨胀。
    pub(crate) services: AppServices,
    /// 配置列表状态
    pub(crate) configs: ConfigsState,
    /// 当前选中状态
    pub(crate) selection: SelectionState,
    /// 运行时状态（隧道是否运行、忙碌状态等）
    pub(crate) runtime: RuntimeState,
    /// 流量统计状态
    pub(crate) stats: StatsState,
    /// 持久化状态
    pub(crate) persistence: PersistenceState,
    /// 用户偏好设置
    pub(crate) ui_prefs: UiPrefsState,
    /// UI 会话状态（页面导航等）
    pub(crate) ui_session: UiSessionState,
    /// UI 内部状态
    pub(crate) ui: UiState,
}

impl WgApp {
    /// 创建新的 WgApp 实例
    ///
    /// # 参数
    /// * `tunnel_session` - 隧道会话服务
    /// * `appearance_policy` - 外观策略（跟随系统/亮色/暗色）
    /// * `resolved_theme_mode` - 解析后的主题模式
    /// * `theme_light_key/dark_key` - 亮/暗主题的键名
    /// * `theme_light_name/dark_name` - 亮/暗主题的名称
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        tunnel_session: TunnelSessionService,
        appearance_policy: AppearancePolicy,
        resolved_theme_mode: ThemeMode,
        theme_light_key: Option<SharedString>,
        theme_dark_key: Option<SharedString>,
        theme_light_name: Option<SharedString>,
        theme_dark_name: Option<SharedString>,
        language_preference: LanguagePreference,
    ) -> Self {
        let ui_prefs = UiPrefsState::new(
            appearance_policy,
            resolved_theme_mode,
            theme_light_key,
            theme_dark_key,
            theme_light_name,
            theme_dark_name,
            language_preference,
        );
        Self {
            services: AppServices::new(tunnel_session),
            configs: ConfigsState::new(),
            selection: SelectionState::new(),
            runtime: RuntimeState::new(),
            stats: StatsState::new(),
            persistence: PersistenceState::new(),
            ui_session: UiSessionState::from_prefs(&ui_prefs),
            ui_prefs,
            ui: UiState::new(),
        }
    }
}
