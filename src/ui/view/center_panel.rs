use gpui::*;
use gpui_component::input::{Input, InputState};

use super::data::ViewData;
use super::widgets::status_badge;
use super::super::components::card;

/// 中间配置面板：显示隧道名与配置内容输入框。
pub(crate) fn render_center_panel(
    data: &ViewData,
    name_input: &Entity<InputState>,
    config_input: &Entity<InputState>,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .flex_grow()
        .p_3()
        .rounded_lg()
        .bg(rgb(0x141b22))
        .border_1()
        .border_color(rgb(0x202a33))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(div().text_xl().child("Configuration"))
                .child(status_badge(data.config_status.as_ref())),
        )
        .child(card(
            "Tunnel Name",
            div()
                .w_full()
                .px_2()
                .py_1()
                .rounded_md()
                .bg(rgb(0x1a2026))
                .child(Input::new(name_input).appearance(false).bordered(false)),
        ))
        .child(
            card(
                "Config",
                div()
                    .w_full()
                    .flex_grow()
                    .min_h(px(320.0))
                    .p_2()
                    .rounded_md()
                    .bg(rgb(0x1a2026))
                    .child(Input::new(config_input).appearance(false).bordered(false).h_full()),
            )
            .flex_grow(),
        )
}
