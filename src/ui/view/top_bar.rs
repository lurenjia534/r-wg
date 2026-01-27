use gpui::*;
use gpui_component::theme::{Theme, ThemeMode};
use gpui_component::{
    button::{Button, ButtonGroup, ButtonVariants},
    h_flex,
    tag::Tag,
    ActiveTheme as _, Disableable as _, Icon, IconName, Selectable, Sizable as _, StyledExt,
};

use super::super::state::{SidebarItem, WgApp};
use super::data::ViewData;

/// 顶部工具栏骨架：标题、配置切换、模式按钮、状态图标。
pub(crate) fn render_top_bar(app: &mut WgApp, data: &ViewData, cx: &mut Context<WgApp>) -> Div {
    let title = h_flex()
        .items_center()
        .gap_3()
        .child(
            div()
                .size(px(40.0))
                .rounded_md()
                .bg(cx.theme().secondary)
                .border_1()
                .border_color(cx.theme().border)
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Icon::new(IconName::LayoutDashboard)
                        .size_6()
                        .text_color(cx.theme().accent),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_lg().font_semibold().child("r-wg"))
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("Dashboard"),
                ),
        );

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
                .selected(!is_dark)
                .tooltip("Switch to light mode")
                .on_click(cx.listener(|this, _, window, cx| {
                    if this.theme_mode != ThemeMode::Light {
                        // 持久化主题选择，便于下次启动恢复。
                        this.theme_mode = ThemeMode::Light;
                        Theme::change(ThemeMode::Light, Some(window), cx);
                        this.persist_state_async(cx);
                    }
                })),
        )
        .child(
            Button::new("theme-dark")
                .icon(Icon::new(IconName::Moon).size_4())
                .selected(is_dark)
                .tooltip("Switch to dark mode")
                .on_click(cx.listener(|this, _, window, cx| {
                    if this.theme_mode != ThemeMode::Dark {
                        // 持久化主题选择，便于下次启动恢复。
                        this.theme_mode = ThemeMode::Dark;
                        Theme::change(ThemeMode::Dark, Some(window), cx);
                        this.persist_state_async(cx);
                    }
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
        Tag::success().small().rounded_full().child("Connected")
    } else {
        Tag::secondary().small().rounded_full().child("Idle")
    };

    let settings_button = Button::new("settings")
        .ghost()
        .icon(Icon::new(IconName::Settings).size_5())
        .tooltip("Open settings")
        .on_click(cx.listener(|this, _, _, cx| {
            this.sidebar_active = SidebarItem::Advanced;
            cx.notify();
        }));

    let tools = h_flex()
        .items_center()
        .gap_2()
        .child(icon_button("notif", IconName::Bell))
        .child(icon_button("health", IconName::CircleCheck))
        .child(settings_button);

    h_flex()
        .items_center()
        .justify_between()
        .gap_6()
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
                .gap_2()
                .child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("Theme"),
                        )
                        .child(theme_toggle),
                )
                .child(vertical_divider(cx))
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("Tunnel"),
                )
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
        .icon(Icon::new(icon).size_5())
}

fn vertical_divider(cx: &mut Context<WgApp>) -> Div {
    div().w(px(1.0)).h(px(20.0)).bg(cx.theme().border)
}
