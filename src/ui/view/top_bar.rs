use gpui::*;
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Selectable, Sizable as _, StyledExt,
    button::{Button, ButtonGroup, ButtonVariants},
    h_flex, tag::Tag,
};
use gpui_component::theme::{Theme, ThemeMode};

use super::data::ViewData;
use super::super::state::WgApp;

/// 顶部工具栏骨架：标题、配置切换、模式按钮、状态图标。
pub(crate) fn render_top_bar(app: &mut WgApp, data: &ViewData, cx: &mut Context<WgApp>) -> Div {
    let title = h_flex()
        .items_center()
        .gap_2()
        .child(Icon::new(IconName::LayoutDashboard).size_5())
        .child(div().text_lg().font_semibold().child("r-wg Dashboard"));

    let config_valid = data.parse_error.is_none() && data.parsed_config.is_some();
    let can_start = config_valid && !app.running && !app.busy;
    let can_stop = app.running && !app.busy;

    let is_dark = cx.theme().is_dark();
    let theme_toggle = ButtonGroup::new("theme-group")
        .outline()
        .compact()
        .small()
        .child(
            Button::new("theme-light")
                .icon(Icon::new(IconName::Sun).size_4())
                .label("Light")
                .selected(!is_dark)
                .tooltip("Switch to light mode")
                .on_click(cx.listener(|_, _, window, cx| {
                    Theme::change(ThemeMode::Light, Some(window), cx);
                })),
        )
        .child(
            Button::new("theme-dark")
                .icon(Icon::new(IconName::Moon).size_4())
                .label("Dark")
                .selected(is_dark)
                .tooltip("Switch to dark mode")
                .on_click(cx.listener(|_, _, window, cx| {
                    Theme::change(ThemeMode::Dark, Some(window), cx);
                })),
        );

    let on_tooltip = if config_valid {
        "Start tunnel"
    } else {
        "Select a valid config first"
    };
    let off_tooltip = if app.running {
        "Stop tunnel"
    } else {
        "Tunnel is not running"
    };

    let modes = ButtonGroup::new("mode-group")
        .outline()
        .compact()
        .small()
        .child(
            Button::new("mode-on")
                .label("On")
                .selected(app.running)
                .disabled(!can_start)
                .tooltip(on_tooltip)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.handle_start_stop(window, cx);
                })),
        )
        .child(
            Button::new("mode-off")
                .label("Off")
                .selected(!app.running)
                .disabled(!can_stop)
                .tooltip(off_tooltip)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.handle_start_stop(window, cx);
                })),
        );

    let status_tag = if app.running {
        Tag::success().small().rounded_full().child("On")
    } else {
        Tag::secondary().small().rounded_full().child("Off")
    };

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
                .child(theme_toggle)
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
