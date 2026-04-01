//! UI 视图入口模块
//!
//! 本模块负责将各个面板组件拼装成完整的三栏布局，
//! 并将派生数据（ViewData）传递给各面板进行渲染。
//!
//! # 布局结构
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────┐
//! │               顶部栏 (Top Bar, 非 Configs 页面)          │
//! ├────────┬───────────────────────────────────────────────┤
//! │        │                                               │
//! │ 侧边栏 │              主内容区域                        │
//! │ (Left  │                                               │
//! │ Panel) │  - Overview (概览)                          │
//! │        │  - Configs (配置库)                          │
//! │        │  - Route Map (路由地图)                       │
//! │        │  - Tools (工具)                              │
//! │        │  - Logs (日志)                               │
//! │        │  - DNS, About, Advanced...                   │
//! │        │                                               │
//! └────────┴───────────────────────────────────────────────┘
//! ```
//!
//! # 响应式设计
//!
//! - **桌面模式**: 侧边栏始终显示
//! - **紧凑模式**: 侧边栏作为浮动覆盖层显示

mod about;
mod advanced;
mod configs;
mod dns;
mod left_panel;
mod logs;
pub(crate) mod overview;
pub(crate) mod route_map;
mod shared;
pub(crate) mod tools;
mod top_bar;
mod widgets;

use gpui::*;
use gpui_component::{
    button::Button, scroll::ScrollableElement as _, ActiveTheme as _, Icon, IconName, Root,
    Sizable as _,
};

use super::features::themes::AppearancePolicy;
use super::state::{ConfigDraftState, SidebarItem, WgApp};
pub(crate) use shared::ViewData;
pub(crate) use widgets::{PageShell, PageShellHeader};

impl WgApp {
    /// 获取共享的视图数据
    ///
    /// 这些数据是从当前配置草稿派生出来的，供不需要完整配置工作区的面板使用。
    fn shared_view_data(&self, cx: &mut Context<Self>) -> ViewData {
        if let Some(workspace) = self.ui.configs_workspace.as_ref() {
            let snapshot = workspace.read(cx);
            ViewData::from_editor(self, &snapshot.draft, snapshot.operation.as_ref())
        } else {
            self.fallback_shared_view_data()
        }
    }

    /// 当没有配置工作区时的后备视图数据
    fn fallback_shared_view_data(&self) -> ViewData {
        let draft = ConfigDraftState::new();
        ViewData::from_editor(self, &draft, None)
    }

    /// 确保注册了主题外观观察者
    ///
    /// 当系统主题变化时（如果设置为"跟随系统"），自动更新应用主题。
    fn ensure_theme_appearance_observer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ui.theme_appearance_observer_ready {
            return;
        }

        self.ui.theme_appearance_observer_ready = true;
        cx.observe_window_appearance(window, |this, window, cx| {
            // 只有在"跟随系统"模式下才响应主题变化
            if this.ui_prefs.appearance_policy != AppearancePolicy::System {
                return;
            }

            let previous_mode = this.ui_prefs.resolved_theme_mode;
            this.apply_theme_prefs(Some(window), cx);
            if this.ui_prefs.resolved_theme_mode != previous_mode {
                cx.notify();
            }
        })
        .detach();
    }
}

/// WgApp 的渲染实现
///
/// 这是 GPUI 应用的主渲染入口，构建完整的 UI 布局。
impl Render for WgApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 加载持久化状态
        self.start_load_persisted_state(window, cx);
        
        // 确保主题观察者已注册
        self.ensure_theme_appearance_observer(window, cx);

        // 非配置模式需要派生数据
        let root_data = (self.ui_session.sidebar_active != SidebarItem::Configs)
            .then(|| self.shared_view_data(cx));

        {
            // 创建并同步左侧面板
            let left_panel = left_panel::ensure_left_panel(cx.entity(), window, cx);
            left_panel::sync_left_panel(&left_panel, self, cx);
            left_panel::sync_left_panel_overlay(self, window, cx);
            
            // 判断是否使用浮动覆盖层侧边栏
            let use_sidebar_overlay = left_panel::sidebar_uses_overlay(window);
            let show_sidebar_overlay_trigger =
                use_sidebar_overlay && !self.ui_session.sidebar_overlay_open;

            // 主容器：水平三栏布局
            let mut main = div()
                .size_full()
                .relative()
                .flex()
                .flex_row()
                .bg(linear_gradient(
                    130.0,
                    linear_color_stop(cx.theme().background, 0.0),
                    linear_color_stop(cx.theme().muted, 1.0),
                ))
                .text_color(cx.theme().foreground);

            // 非覆盖模式：将侧边栏直接添加到布局
            if !use_sidebar_overlay {
                main = main.child(left_panel.clone());
            }

            // 主内容区域
            main = main.child({
                let main_body = match self.ui_session.sidebar_active {
                    SidebarItem::Overview => {
                        // 概览页面
                        let overview_page =
                            super::features::ensure_overview_page(cx.entity(), window, cx);
                        overview_page.into_any_element()
                    }
                    SidebarItem::Configs => {
                        // 配置库页面（使用完整工作区）
                        let workspace = self.ensure_configs_workspace(cx);
                        div()
                            .flex()
                            .flex_1()
                            .min_h(px(0.0))
                            .child(workspace)
                            .into_any_element()
                    }
                    SidebarItem::Proxies => {
                        // 代理页面
                        super::features::render_proxies(self, window, cx).into_any_element()
                    }
                    SidebarItem::Logs => {
                        // 日志页面
                        logs::render_logs(self, window, cx).into_any_element()
                    }
                    SidebarItem::Dns => {
                        // DNS 设置页面
                        dns::render_dns(self, cx).into_any_element()
                    }
                    SidebarItem::About => {
                        // 关于页面
                        about::render_about(self, window, cx).into_any_element()
                    }
                    SidebarItem::Advanced => {
                        // 高级设置页面
                        advanced::render_advanced(self, cx).into_any_element()
                    }
                    SidebarItem::RouteMap => {
                        // 路由地图页面
                        let data = root_data
                            .as_ref()
                            .expect("root data should exist outside Configs");
                        super::features::render_route_map(self, data, window, cx).into_any_element()
                    }
                    SidebarItem::Tools => {
                        // 网络工具页面
                        super::features::render_tools(self, window, cx).into_any_element()
                    }
                    _ => {
                        // 占位页面
                        super::features::render_placeholder(cx).into_any_element()
                    }
                };

                // 配置页面不需要顶部栏
                if self.ui_session.sidebar_active == SidebarItem::Configs {
                    div()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .flex_grow()
                        .min_h(px(0.0))
                        .p_3()
                        .child(main_body)
                        .into_any_element()
                } else {
                    // 非配置页面需要顶部栏
                    let data = root_data
                        .as_ref()
                        .expect("root data should exist outside Configs");
                    div()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .flex_grow()
                        .min_h(px(0.0))
                        .p_3()
                        .child(top_bar::render_top_bar(self, data, cx))
                        .child({
                            let body = div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_h(px(0.0))
                                .child(main_body);

                            // 概览页面需要滚动条
                            if self.ui_session.sidebar_active == SidebarItem::Overview {
                                body.overflow_y_scrollbar().into_any_element()
                            } else {
                                body.into_any_element()
                            }
                        })
                        .into_any_element()
                }
            });

            // 覆盖层触发按钮（紧凑模式）
            if show_sidebar_overlay_trigger {
                main = main.child(
                    div().absolute().top_3().left_3().child(
                        Button::new("sidebar-overlay-trigger")
                            .outline()
                            .small()
                            .icon(Icon::new(IconName::PanelLeftOpen).size_4())
                            .tooltip("Open navigation")
                            .on_click(cx.listener(|this, _, window, cx| {
                                left_panel::toggle_sidebar(this, window, cx);
                            })),
                    ),
                );
            }

            // 叠加层渲染
            let mut overlays = div().absolute().top_0().left_0().size_full();
            
            // 左侧面板覆盖层
            if let Some(left_panel_overlay) =
                left_panel::render_left_panel_overlay(&left_panel, self, window, cx)
            {
                overlays = overlays.child(left_panel_overlay);
            }
            
            // 底部表单层
            if let Some(sheet_layer) = Root::render_sheet_layer(window, cx) {
                overlays = overlays.child(sheet_layer);
            }
            
            // 对话框层
            if let Some(dialog_layer) = Root::render_dialog_layer(window, cx) {
                overlays = overlays.child(dialog_layer);
            }
            
            // 通知层
            if let Some(notification_layer) = Root::render_notification_layer(window, cx) {
                overlays = overlays.child(notification_layer);
            }

            // 最终布局
            div().relative().size_full().child(main).child(overlays)
        }
    }
}
