//! WgApp 状态管理模块
//!
//! 本模块定义了 WgApp 结构体，它是整个 UI 应用状态的中心管理者。
//! WgApp 协调多个子模块的状态，并提供统一的访问接口。
//!
//! # 状态子模块
//!
//! - `tunnel_session`: 隧道会话服务（与后端引擎交互）
//! - `backend_admin`: 特权后端管理服务
//! - `config_library`: 配置库服务（文件的导入/导出/删除）
//! - `configs`: 配置列表和当前编辑状态
//! - `selection`: 当前选中的配置
//! - `runtime`: 隧道运行状态（运行中/空闲/忙碌）
//! - `stats`: 流量统计
//! - `persistence`: 持久化状态
//! - `ui_prefs`: UI 用户偏好（主题、面板宽度等）
//! - `ui_session`: UI 会话状态（当前页面、侧边栏状态等）
//! - `ui`: UI 内部状态（搜索、临时数据等）

use std::time::Duration;

use gpui::{SharedString, Timer, Window};
use gpui_component::notification::Notification;
use gpui_component::theme::ThemeMode;
use gpui_component::WindowExt;
use r_wg::application::{BackendAdminService, ConfigLibraryService, TunnelSessionService};
use r_wg::dns::{DnsMode, DnsPreset};

use crate::ui::features::themes::{self, AppearancePolicy};

use super::{
    ConfigInspectorTab, ConfigsState, PersistenceState, ProxiesViewMode, RouteFamilyFilter,
    RouteMapMode, RuntimeState, SelectionState, SidebarItem, StatsState, TrafficPeriod,
    UiPrefsState, UiSessionState, UiState,
};

/// WgApp 结构体
///
/// 这是 GPUI 应用的主状态容器，包含所有子模块的状态和服务。
/// 它提供了大量 helper 方法用于查询和修改各种 UI 状态。
pub(crate) struct WgApp {
    /// 隧道会话服务，用于与后端引擎交互
    pub(crate) tunnel_session: TunnelSessionService,
    /// 特权后端管理服务，用于安装/移除后端服务
    pub(crate) backend_admin: BackendAdminService,
    /// 配置库服务，用于管理配置文件的 CRUD 操作
    pub(crate) config_library: ConfigLibraryService,
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
    pub(crate) fn new(
        tunnel_session: TunnelSessionService,
        appearance_policy: AppearancePolicy,
        resolved_theme_mode: ThemeMode,
        theme_light_key: Option<SharedString>,
        theme_dark_key: Option<SharedString>,
        theme_light_name: Option<SharedString>,
        theme_dark_name: Option<SharedString>,
    ) -> Self {
        let ui_prefs = UiPrefsState::new(
            appearance_policy,
            resolved_theme_mode,
            theme_light_key,
            theme_dark_key,
            theme_light_name,
            theme_dark_name,
        );
        Self {
            tunnel_session,
            backend_admin: BackendAdminService::new(),
            config_library: ConfigLibraryService::new(),
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

    pub(crate) fn current_configs_inspector_tab(
        &self,
        cx: &mut gpui::Context<Self>,
    ) -> ConfigInspectorTab {
        self.ui
            .configs_workspace
            .as_ref()
            .map(|workspace| workspace.read(cx).inspector_tab)
            .unwrap_or(self.ui_prefs.preferred_inspector_tab)
    }

    pub(crate) fn persist_preferred_inspector_tab(
        &mut self,
        value: ConfigInspectorTab,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        if self.ui_prefs.preferred_inspector_tab == value {
            return false;
        }
        self.ui_prefs.preferred_inspector_tab = value;
        self.persist_state_async(cx);
        true
    }

    pub(crate) fn persist_configs_panel_widths(
        &mut self,
        library_width: f32,
        inspector_width: f32,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        let library_width = library_width.clamp(240.0, 420.0);
        let inspector_width = inspector_width.clamp(280.0, 440.0);
        if self.ui_prefs.configs_library_width == library_width
            && self.ui_prefs.configs_inspector_width == inspector_width
        {
            return false;
        }
        self.ui_prefs.configs_library_width = library_width;
        self.ui_prefs.configs_inspector_width = inspector_width;
        self.persist_state_async(cx);
        true
    }

    pub(crate) fn persist_route_map_panel_widths(
        &mut self,
        inventory_width: f32,
        inspector_width: f32,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        let inventory_width = inventory_width.clamp(240.0, 360.0);
        let inspector_width = inspector_width.clamp(280.0, 420.0);
        if self.ui_prefs.route_map_inventory_width == inventory_width
            && self.ui_prefs.route_map_inspector_width == inspector_width
        {
            return false;
        }
        self.ui_prefs.route_map_inventory_width = inventory_width;
        self.ui_prefs.route_map_inspector_width = inspector_width;
        self.persist_state_async(cx);
        true
    }

    pub(crate) fn set_appearance_policy_pref(
        &mut self,
        value: AppearancePolicy,
        window: Option<&mut Window>,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.ui_prefs.appearance_policy != value {
            self.ui_prefs.appearance_policy = value;
            let refresh_all_windows = window.is_none();
            self.apply_theme_prefs(window, cx);
            if refresh_all_windows {
                cx.refresh_windows();
            }
            self.persist_state_async(cx);
        }
        cx.notify();
    }

    pub(crate) fn set_theme_palette_pref(
        &mut self,
        mode: ThemeMode,
        value: Option<SharedString>,
        window: Option<&mut Window>,
        cx: &mut gpui::Context<Self>,
    ) {
        let slot = match mode {
            ThemeMode::Light => &mut self.ui_prefs.theme_light_key,
            ThemeMode::Dark => &mut self.ui_prefs.theme_dark_key,
        };

        if *slot != value {
            *slot = value;
            match mode {
                ThemeMode::Light => self.ui_prefs.theme_light_name = None,
                ThemeMode::Dark => self.ui_prefs.theme_dark_name = None,
            }

            let active_mode_changed = self.ui_prefs.resolved_theme_mode == mode;
            let refresh_all_windows = active_mode_changed && window.is_none();
            self.apply_theme_prefs(if active_mode_changed { window } else { None }, cx);
            if refresh_all_windows {
                cx.refresh_windows();
            }
            self.persist_state_async(cx);
        }
        cx.notify();
    }

    pub(crate) fn reset_theme_prefs(
        &mut self,
        window: Option<&mut Window>,
        cx: &mut gpui::Context<Self>,
    ) {
        let changed = self.ui_prefs.theme_light_key.take().is_some()
            || self.ui_prefs.theme_dark_key.take().is_some()
            || self.ui_prefs.theme_light_name.take().is_some()
            || self.ui_prefs.theme_dark_name.take().is_some();

        if changed {
            let refresh_all_windows = window.is_none();
            self.apply_theme_prefs(window, cx);
            if refresh_all_windows {
                cx.refresh_windows();
            }
            self.persist_state_async(cx);
        }
        cx.notify();
    }

    pub(crate) fn apply_theme_prefs(
        &mut self,
        window: Option<&mut Window>,
        cx: &mut gpui::Context<Self>,
    ) {
        let storage = self.configs.ensure_storage().ok();
        let light = themes::resolve_theme_preference(
            ThemeMode::Light,
            self.ui_prefs.theme_light_key.as_deref().map(|key| &**key),
            self.ui_prefs
                .theme_light_name
                .as_deref()
                .map(|name| &**name),
            storage.as_ref(),
            cx,
        );
        let dark = themes::resolve_theme_preference(
            ThemeMode::Dark,
            self.ui_prefs.theme_dark_key.as_deref().map(|key| &**key),
            self.ui_prefs.theme_dark_name.as_deref().map(|name| &**name),
            storage.as_ref(),
            cx,
        );
        self.ui_prefs.theme_light_key = Some(light.entry.key.clone());
        self.ui_prefs.theme_dark_key = Some(dark.entry.key.clone());
        self.ui_prefs.theme_light_name = Some(light.entry.name.clone());
        self.ui_prefs.theme_dark_name = Some(dark.entry.name.clone());
        self.ui_prefs.resolved_theme_mode = themes::apply_resolved_theme_preferences(
            self.ui_prefs.appearance_policy,
            light.entry.config.clone(),
            dark.entry.config.clone(),
            window,
            cx,
        );
    }

    pub(crate) fn set_log_auto_follow_pref(&mut self, value: bool, cx: &mut gpui::Context<Self>) {
        if self.ui_prefs.log_auto_follow != value {
            self.ui_prefs.log_auto_follow = value;
            self.persist_state_async(cx);
        }
        cx.notify();
    }

    pub(crate) fn set_dns_mode_pref(&mut self, value: DnsMode, cx: &mut gpui::Context<Self>) {
        if self.ui_prefs.dns_mode != value {
            self.ui_prefs.dns_mode = value;
            self.persist_state_async(cx);
        }
        cx.notify();
    }

    pub(crate) fn set_dns_preset_pref(&mut self, value: DnsPreset, cx: &mut gpui::Context<Self>) {
        if self.ui_prefs.dns_preset != value {
            self.ui_prefs.dns_preset = value;
            self.persist_state_async(cx);
        }
        cx.notify();
    }

    pub(crate) fn set_preferred_inspector_tab(
        &mut self,
        value: ConfigInspectorTab,
        cx: &mut gpui::Context<Self>,
    ) {
        self.persist_preferred_inspector_tab(value, cx);
        if let Some(workspace) = self.ui.configs_workspace.clone() {
            workspace.update(cx, |workspace, cx| {
                if workspace.set_inspector_tab(value) {
                    cx.notify();
                }
            });
        }
        cx.notify();
    }

    pub(crate) fn set_preferred_traffic_period(
        &mut self,
        value: TrafficPeriod,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.ui_prefs.preferred_traffic_period != value {
            self.ui_prefs.preferred_traffic_period = value;
            self.persist_state_async(cx);
        }
        self.ui_session.traffic_period = value;
        cx.notify();
    }

    pub(crate) fn set_session_traffic_period(
        &mut self,
        value: TrafficPeriod,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.ui_session.traffic_period != value {
            self.ui_session.traffic_period = value;
            cx.notify();
        }
    }

    pub(crate) fn set_proxies_view_mode_pref(
        &mut self,
        value: ProxiesViewMode,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.ui_prefs.proxies_view_mode != value {
            self.ui_prefs.proxies_view_mode = value;
            self.persist_state_async(cx);
        }
        cx.notify();
    }

    pub(crate) fn set_sidebar_active(&mut self, value: SidebarItem, cx: &mut gpui::Context<Self>) {
        if self.ui_session.sidebar_active != value {
            self.ui_session.sidebar_active = value;
            cx.notify();
        }
    }

    pub(crate) fn set_sidebar_collapsed(&mut self, value: bool, cx: &mut gpui::Context<Self>) {
        if self.ui_session.sidebar_collapsed != value {
            self.ui_session.sidebar_collapsed = value;
            cx.notify();
        }
    }

    pub(crate) fn toggle_sidebar_collapsed(&mut self, cx: &mut gpui::Context<Self>) {
        let next = !self.ui_session.sidebar_collapsed;
        self.set_sidebar_collapsed(next, cx);
    }

    pub(crate) fn set_sidebar_overlay_open(&mut self, value: bool, cx: &mut gpui::Context<Self>) {
        if self.ui_session.sidebar_overlay_open != value {
            self.ui_session.sidebar_overlay_open = value;
            cx.notify();
        }
    }

    pub(crate) fn open_sidebar_overlay(&mut self, cx: &mut gpui::Context<Self>) {
        self.set_sidebar_overlay_open(true, cx);
    }

    pub(crate) fn close_sidebar_overlay(&mut self, cx: &mut gpui::Context<Self>) {
        self.set_sidebar_overlay_open(false, cx);
    }

    pub(crate) fn set_route_map_mode(&mut self, value: RouteMapMode, cx: &mut gpui::Context<Self>) {
        if self.ui_session.route_map_mode != value {
            self.ui_session.route_map_mode = value;
            cx.notify();
        }
    }

    pub(crate) fn set_route_map_family_filter(
        &mut self,
        value: RouteFamilyFilter,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.ui_session.route_map_family_filter != value {
            self.ui_session.route_map_family_filter = value;
            cx.notify();
        }
    }

    pub(crate) fn set_route_map_selected_item(
        &mut self,
        value: Option<SharedString>,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.ui_session.route_map_selected_item != value {
            self.ui_session.route_map_selected_item = value;
            cx.notify();
        }
    }

    pub(crate) fn set_route_map_glossary_open(
        &mut self,
        value: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.ui_session.route_map_glossary_open != value {
            self.ui_session.route_map_glossary_open = value;
            cx.notify();
        }
    }

    pub(crate) fn sync_route_map_search_query(
        &mut self,
        value: impl Into<SharedString>,
        cx: &mut gpui::Context<Self>,
    ) {
        let value = value.into();
        if self.ui.route_map_search.raw_query == value {
            return;
        }
        self.ui.route_map_search.raw_query = value;
        self.ui.route_map_search.enqueue();
        if self.ui.route_map_search.worker_active {
            return;
        }
        self.ui.route_map_search.worker_active = true;

        cx.spawn(async move |view, cx| loop {
            Timer::after(Duration::from_millis(150)).await;

            let Some(query) = view
                .update(cx, |this, _| {
                    match this.ui.route_map_search.take_queued_revision() {
                        Some(_) => Some(this.ui.route_map_search.raw_query.clone()),
                        None => {
                            this.ui.route_map_search.worker_active = false;
                            None
                        }
                    }
                })
                .ok()
                .flatten()
            else {
                break;
            };

            let _ = view.update(cx, |this, cx| {
                if this.ui.route_map_search.debounced_query != query {
                    this.ui.route_map_search.debounced_query = query;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(crate) fn set_show_alternate_theme_preview(
        &mut self,
        value: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.ui_session.show_alternate_theme_preview != value {
            self.ui_session.show_alternate_theme_preview = value;
            cx.notify();
        }
    }

    pub(crate) fn push_success_toast(
        &mut self,
        message: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        window.push_notification(Notification::success(message.into()), cx);
    }
}
