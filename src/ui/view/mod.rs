// UI 视图入口：负责把三栏布局拼装起来，并将派生数据交给各面板渲染。
mod about;
mod configs;
mod data;
mod dns;
mod left_panel;
mod logs;
mod overview;
mod proxies;
mod right_panel;
mod top_bar;
mod widgets;

use gpui::*;
use gpui_component::ActiveTheme as _;

use super::state::{SidebarItem, WgApp};
use data::ViewData;

impl Render for WgApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 输入控件按需延迟创建，避免在构造阶段就绑定窗口上下文。
        self.ensure_inputs(window, cx);
        self.start_load_persisted_state(window, cx);

        // 复制 Entity 句柄，避免后续借用冲突。
        let name_input = self
            .name_input
            .clone()
            .expect("name input should be initialized");
        let config_input = self
            .config_input
            .clone()
            .expect("config input should be initialized");

        // 统一计算状态/解析结果等派生信息。
        let data = ViewData::new(self);

        div()
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
            .child(left_panel::render_left_panel(self, &data, cx))
            .child({
                let main_body = match self.sidebar_active {
                    SidebarItem::Overview => {
                        overview::render_overview(self, &data, cx).into_any_element()
                    }
                    SidebarItem::Configs => div()
                        .flex()
                        .flex_row()
                        .gap_3()
                        .flex_1()
                        .min_h(px(0.0))
                        .child(configs::render_configs_editor(
                            self,
                            &data,
                            &name_input,
                            &config_input,
                            cx,
                        ))
                        .child(right_panel::render_right_panel(self, &data, cx))
                        .into_any_element(),
                    SidebarItem::Proxies => {
                        proxies::render_proxies(self, window, cx).into_any_element()
                    }
                    SidebarItem::Logs => logs::render_logs(self, window, cx).into_any_element(),
                    SidebarItem::Dns => dns::render_dns(self, cx).into_any_element(),
                    SidebarItem::About => about::render_about(self, cx).into_any_element(),
                    _ => overview::render_placeholder(cx).into_any_element(),
                };

                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .flex_grow()
                    .min_h(px(0.0))
                    .p_3()
                    // 顶部工具栏
                    .child(top_bar::render_top_bar(self, &data, cx))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h(px(0.0))
                            .child(main_body),
                    )
            })
    }
}
