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
    let display_font = "Space Grotesk";
    let ui_font = "Plus Jakarta Sans";

    let title = h_flex()
        .items_center()
        .gap_3()
        .child(
            Icon::new(IconName::LayoutDashboard)
                .size_6()
                .text_color(cx.theme().accent),
        );

    let config_valid = data.parse_error.is_none() && data.parsed_config.is_some();
    let can_start = config_valid && !app.running && !app.busy;
    let can_stop = app.running && !app.busy;

    let is_dark = cx.theme().is_dark();
    let bar_bg = linear_gradient(
        120.0,
        linear_color_stop(cx.theme().title_bar, 0.0),
        linear_color_stop(cx.theme().secondary, 1.0),
    );
    let chip_bg = if is_dark {
        cx.theme().background.alpha(0.45)
    } else {
        cx.theme().secondary
    };
    let chip_border = if is_dark {
        cx.theme().foreground.alpha(0.12)
    } else {
        cx.theme().border
    };

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

    let status_chip = {
        let (label, dot_color, text_color, bg, border) = if app.running {
            (
                "Connected",
                cx.theme().accent,
                cx.theme().accent,
                cx.theme().accent.alpha(0.18),
                cx.theme().accent.alpha(0.35),
            )
        } else {
            (
                "Idle",
                cx.theme().muted_foreground,
                cx.theme().muted_foreground,
                chip_bg,
                chip_border,
            )
        };

        h_flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_1()
            .rounded_full()
            .border_1()
            .border_color(border)
            .bg(bg)
            .child(div().size(px(6.0)).rounded_full().bg(dot_color))
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(text_color)
                    .child(label),
            )
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
        .px_2()
        .py_1()
        .rounded_full()
        .border_1()
        .border_color(chip_border)
        .bg(chip_bg)
        .child(icon_button("notif", IconName::Bell))
        .child(icon_button("health", IconName::CircleCheck))
        .child(settings_button);

    h_flex()
        .items_center()
        .justify_between()
        .gap_6()
        .child(title)
        .child(
            h_flex()
                .items_center()
                .gap_3()
                .px_3()
                .py_2()
                .rounded_full()
                .border_1()
                .border_color(chip_border)
                .bg(chip_bg)
                .child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_xs()
                                .font_family(ui_font)
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(cx.theme().muted_foreground)
                                .child("Theme"),
                        )
                        .child(theme_toggle),
                )
                .child(vertical_divider(cx))
                .child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_xs()
                                .font_family(ui_font)
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(cx.theme().muted_foreground)
                                .child("Tunnel"),
                        )
                )
                .child(modes),
        )
        .child(
            h_flex()
                .items_center()
                .gap_2()
                .child(status_chip)
                .child(tools),
        )
}

fn icon_button(id: &'static str, icon: IconName) -> Button {
    Button::new(id).ghost().icon(Icon::new(icon).size_5())
}

fn vertical_divider(cx: &mut Context<WgApp>) -> Div {
    let color = if cx.theme().is_dark() {
        cx.theme().foreground.alpha(0.12)
    } else {
        cx.theme().border
    };
    div().w(px(1.0)).h(px(22.0)).bg(color)
}
