// UI 视图入口：负责把三栏布局拼装起来，并将派生数据交给各面板渲染。
mod about;
mod advanced;
mod configs;
mod data;
mod dns;
mod left_panel;
mod logs;
mod overview;
mod proxies;
mod proxies_grid;
mod top_bar;
mod widgets;

use gpui::*;
use gpui_component::{scroll::ScrollableElement as _, ActiveTheme as _, Root};

use super::state::{SidebarItem, WgApp};
use data::ViewData;

impl WgApp {
    fn shared_view_data(&self, cx: &mut Context<Self>) -> ViewData {
        if let Some(workspace) = self.ui.configs_workspace.as_ref() {
            let snapshot = workspace.read(cx);
            ViewData::from_editor(self, &snapshot.draft, snapshot.operation.as_ref())
        } else {
            self.compat_editor_view_data()
        }
    }

    fn compat_editor_view_data(&self) -> ViewData {
        ViewData::from_editor(self, &self.editor.draft, self.editor.operation.as_ref())
    }
}

impl Render for WgApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.start_load_persisted_state(window, cx);

        let root_data = (self.ui_session.sidebar_active != SidebarItem::Configs)
            .then(|| self.shared_view_data(cx));

        {
            let main = div()
                .size_full()
                .flex()
                .flex_row()
                .bg(linear_gradient(
                    130.0,
                    linear_color_stop(cx.theme().background, 0.0),
                    linear_color_stop(cx.theme().muted, 1.0),
                ))
                .text_color(cx.theme().foreground)
                // 左侧：隧道列表 + 操作按钮
                .child(left_panel::render_left_panel(self, cx))
                .child({
                    let main_body = match self.ui_session.sidebar_active {
                        SidebarItem::Overview => {
                            let data = root_data
                                .as_ref()
                                .expect("root data should exist outside Configs");
                            let overview_data = data::OverviewData::new(self, &data);
                            overview::render_overview(&overview_data, cx).into_any_element()
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
                            proxies::render_proxies(self, window, cx).into_any_element()
                        }
                        SidebarItem::Logs => logs::render_logs(self, window, cx).into_any_element(),
                        SidebarItem::Dns => dns::render_dns(self, cx).into_any_element(),
                        SidebarItem::About => {
                            about::render_about(self, window, cx).into_any_element()
                        }
                        SidebarItem::Advanced => {
                            advanced::render_advanced(self, cx).into_any_element()
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
                                let body =
                                    if self.ui_session.sidebar_active == SidebarItem::Overview {
                                        body.overflow_y_scrollbar().into_any_element()
                                    } else {
                                        body.into_any_element()
                                    };
                                body
                            })
                            .into_any_element()
                    }
                });

            let mut overlays = div().absolute().top_0().left_0().size_full();
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
