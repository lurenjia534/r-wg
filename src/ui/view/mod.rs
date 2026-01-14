// UI 视图入口：负责把三栏布局拼装起来，并将派生数据交给各面板渲染。
mod center_panel;
mod data;
mod left_panel;
mod right_panel;
mod top_bar;
mod widgets;

use gpui::*;

use super::state::WgApp;
use data::ViewData;

impl Render for WgApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 输入控件按需延迟创建，避免在构造阶段就绑定窗口上下文。
        self.ensure_inputs(window, cx);

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
                linear_color_stop(rgb(0x0e1318), 0.0),
                linear_color_stop(rgb(0x121a21), 1.0),
            ))
            .text_color(rgb(0xe6e6e6))
            // 左侧：隧道列表 + 操作按钮
            .child(left_panel::render_left_panel(self, &data, cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .flex_grow()
                    .p_3()
                    // 顶部工具栏
                    .child(top_bar::render_top_bar(self, cx))
                    // 中间：配置编辑区 + 右侧：状态/日志面板
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_3()
                            .flex_grow()
                            .child(center_panel::render_center_panel(
                                &data,
                                &name_input,
                                &config_input,
                            ))
                            .child(right_panel::render_right_panel(self, &data, cx)),
                    ),
            )
    }
}
