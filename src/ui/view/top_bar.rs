use gpui::*;
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Selectable, Sizable as _, StyledExt, button::{Button,
    ButtonGroup, ButtonVariants},
    h_flex, tag::Tag,
};

use super::super::state::WgApp;

/// 顶部工具栏骨架：标题、配置切换、模式按钮、状态图标。
pub(crate) fn render_top_bar(_app: &mut WgApp, cx: &mut Context<WgApp>) -> Div {
    let title = h_flex()
        .items_center()
        .gap_2()
        .child(Icon::new(IconName::LayoutDashboard).size_5())
        .child(div().text_lg().font_semibold().child("r-wg Dashboard"));

    let profile = h_flex()
        .items_center()
        .gap_1()
        .px_3()
        .py_1()
        .rounded_full()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().secondary)
        .text_color(cx.theme().foreground)
        .child("Work")
        .child(Icon::new(IconName::ChevronDown).size_3());

    let modes = ButtonGroup::new("mode-group")
        .outline()
        .compact()
        .small()
        .child(Button::new("mode-on").label("On").selected(true))
        .child(Button::new("mode-off").label("Off"));

    let status_tag = Tag::success().small().rounded_full().child("On");

    let tools = h_flex()
        .items_center()
        .gap_2()
        .child(icon_button("notif", IconName::Bell))
        .child(icon_button("health", IconName::CircleCheck))
        .child(icon_button("settings", IconName::Settings));

    h_flex()
        .items_center()
        .justify_between()
        .gap_4()
        .px_3()
        .py_2()
        .rounded_lg()
        .border_1()
        .border_color(cx.theme().title_bar_border)
        .bg(cx.theme().title_bar)
        .child(title)
        .child(
            h_flex()
                .items_center()
                .gap_3()
                .child(profile)
                .child(modes),
        )
        .child(
            h_flex()
                .items_center()
                .gap_3()
                .child(status_tag)
                .child(tools),
        )
}

fn icon_button(id: &'static str, icon: IconName) -> Button {
    Button::new(id)
        .ghost()
        .xsmall()
        .icon(Icon::new(icon).size_4())
}
