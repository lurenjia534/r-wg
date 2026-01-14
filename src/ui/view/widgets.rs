use gpui::*;

use super::data::ConfigStatus;
use super::super::state::WgApp;

/// 配置状态徽标（Valid/Invalid），没有状态时返回空元素。
pub(crate) fn status_badge(status: Option<&ConfigStatus>) -> AnyElement {
    match status {
        Some(status) => div()
            .px_2()
            .py_1()
            .rounded_full()
            .text_xs()
            .bg(rgb(status.color))
            .child(status.label)
            .into_any_element(),
        None => div().into_any_element(),
    }
}

/// 右侧面板顶部标签按钮（状态/日志）。
pub(crate) fn tab_button(
    label: &'static str,
    active: bool,
    cx: &mut Context<WgApp>,
    set_tab: fn(&mut WgApp),
) -> Stateful<Div> {
    let mut button = div()
        .px_3()
        .py_1()
        .rounded_full()
        .text_sm()
        .bg(if active { rgb(0x2b6f55) } else { rgb(0x22272e) })
        .text_color(if active { rgb(0xe6e6e6) } else { rgb(0x8a939c) })
        .child(label)
        .id(label);

    button = button.on_click(cx.listener(move |this, _event, _window, cx| {
        set_tab(this);
        cx.notify();
    }));
    button
}
