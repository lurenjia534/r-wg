// UI 视图入口：负责把三栏布局拼装起来，并将派生数据交给各面板渲染。
mod about;
mod advanced;
mod configs;
mod dns;
mod left_panel;
mod logs;
mod overview;
mod route_map;
mod shared;
mod top_bar;
mod widgets;

use gpui::*;
use gpui_component::{
    button::Button, scroll::ScrollableElement as _, ActiveTheme as _, Icon, IconName, Root,
    Sizable as _,
};

use super::state::{ConfigDraftState, SidebarItem, WgApp};
use super::themes::AppearancePolicy;
use shared::ViewData;

pub(crate) use widgets::{PageShell, PageShellHeader};

impl WgApp {
    fn shared_view_data(&self, cx: &mut Context<Self>) -> ViewData {
        if let Some(workspace) = self.ui.configs_workspace.as_ref() {
            let snapshot = workspace.read(cx);
            ViewData::from_editor(self, &snapshot.draft, snapshot.operation.as_ref())
        } else {
            self.fallback_shared_view_data()
        }
    }

    fn fallback_shared_view_data(&self) -> ViewData {
        let draft = ConfigDraftState::new();
        ViewData::from_editor(self, &draft, None)
    }

    fn ensure_theme_appearance_observer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ui.theme_appearance_observer_ready {
            return;
        }

        self.ui.theme_appearance_observer_ready = true;
        cx.observe_window_appearance(window, |this, window, cx| {
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

impl Render for WgApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.start_load_persisted_state(window, cx);
        self.ensure_theme_appearance_observer(window, cx);

        let root_data = (self.ui_session.sidebar_active != SidebarItem::Configs)
            .then(|| self.shared_view_data(cx));

        {
            let left_panel = left_panel::ensure_left_panel(cx.entity(), window, cx);
            left_panel::sync_left_panel(&left_panel, self, cx);
            left_panel::sync_left_panel_overlay(self, window, cx);
            let use_sidebar_overlay = left_panel::sidebar_uses_overlay(window);
            let show_sidebar_overlay_trigger =
                use_sidebar_overlay && !self.ui_session.sidebar_overlay_open;

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

            if !use_sidebar_overlay {
                main = main.child(left_panel.clone());
            }

            main = main.child({
                let main_body = match self.ui_session.sidebar_active {
                    SidebarItem::Overview => {
                        let overview_page = overview::ensure_overview_page(cx.entity(), window, cx);
                        overview_page.into_any_element()
                    }
                    SidebarItem::Configs => {
                        let workspace = self.ensure_configs_workspace(cx);
                        div()
                            .flex()
                            .flex_1()
                            .min_h(px(0.0))
                            .child(workspace)
                            .into_any_element()
                    }
                    SidebarItem::Proxies => {
                        super::features::render_proxies(self, window, cx).into_any_element()
                    }
                    SidebarItem::Logs => logs::render_logs(self, window, cx).into_any_element(),
                    SidebarItem::Dns => dns::render_dns(self, cx).into_any_element(),
                    SidebarItem::About => about::render_about(self, window, cx).into_any_element(),
                    SidebarItem::Advanced => advanced::render_advanced(self, cx).into_any_element(),
                    SidebarItem::RouteMap => {
                        let data = root_data
                            .as_ref()
                            .expect("root data should exist outside Configs");
                        route_map::render_route_map(self, data, window, cx).into_any_element()
                    }
                    _ => overview::render_placeholder(cx).into_any_element(),
                };

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

                            if self.ui_session.sidebar_active == SidebarItem::Overview {
                                body.overflow_y_scrollbar().into_any_element()
                            } else {
                                body.into_any_element()
                            }
                        })
                        .into_any_element()
                }
            });

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

            let mut overlays = div().absolute().top_0().left_0().size_full();
            if let Some(left_panel_overlay) =
                left_panel::render_left_panel_overlay(&left_panel, self, window, cx)
            {
                overlays = overlays.child(left_panel_overlay);
            }
            if let Some(sheet_layer) = Root::render_sheet_layer(window, cx) {
                overlays = overlays.child(sheet_layer);
            }
            if let Some(dialog_layer) = Root::render_dialog_layer(window, cx) {
                overlays = overlays.child(dialog_layer);
            }
            if let Some(notification_layer) = Root::render_notification_layer(window, cx) {
                overlays = overlays.child(notification_layer);
            }

            div().relative().size_full().child(main).child(overlays)
        }
    }
}
