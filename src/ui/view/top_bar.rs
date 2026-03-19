use gpui::*;
use gpui_component::{
    button::{Button, ButtonGroup, ButtonVariants},
    h_flex, ActiveTheme as _, Disableable as _, Icon, IconName, Selectable, Sizable as _,
};

use super::super::state::{SidebarItem, WgApp};
use super::super::themes::AppearancePolicy;
use super::data::ViewData;

/// 顶部工具栏骨架：标题、配置切换、模式按钮、状态图标。
pub(crate) fn render_top_bar(app: &mut WgApp, data: &ViewData, cx: &mut Context<WgApp>) -> Div {
    let ui_font = "Plus Jakarta Sans";
    let (eyebrow, title_text, subtitle_text) = top_bar_copy(app);

    let title = div().child(
        h_flex().items_start().child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .font_family(ui_font)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(cx.theme().muted_foreground)
                        .child(eyebrow),
                )
                .child(
                    div()
                        .text_lg()
                        .font_family(ui_font)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(cx.theme().foreground)
                        .child(title_text),
                )
                .child(
                    div()
                        .text_sm()
                        .font_family(ui_font)
                        .text_color(cx.theme().muted_foreground)
                        .child(subtitle_text),
                ),
        ),
    );

    let config_valid = data.parse_error.is_none() && data.parsed_config.is_some();
    let can_start = config_valid
        && data.has_saved_source
        && !data.draft_dirty
        && !app.runtime.running
        && !app.runtime.busy;
    let can_stop = app.runtime.running && !app.runtime.busy;

    let appearance_policy = app.ui_prefs.appearance_policy;
    let is_dark = cx.theme().is_dark();
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
            Button::new("theme-system")
                .label("Auto")
                .selected(appearance_policy == AppearancePolicy::System)
                .tooltip("Follow system appearance")
                .on_click(cx.listener(|this, _, window, cx| {
                    this.set_appearance_policy_pref(AppearancePolicy::System, Some(window), cx);
                })),
        )
        .child(
            Button::new("theme-light")
                .icon(Icon::new(IconName::Sun).size_4())
                .selected(appearance_policy == AppearancePolicy::Light)
                .tooltip("Switch to light mode")
                .on_click(cx.listener(|this, _, window, cx| {
                    this.set_appearance_policy_pref(AppearancePolicy::Light, Some(window), cx);
                })),
        )
        .child(
            Button::new("theme-dark")
                .icon(Icon::new(IconName::Moon).size_4())
                .selected(appearance_policy == AppearancePolicy::Dark)
                .tooltip("Switch to dark mode")
                .on_click(cx.listener(|this, _, window, cx| {
                    this.set_appearance_policy_pref(AppearancePolicy::Dark, Some(window), cx);
                })),
        );

    let on_tooltip = if !data.has_saved_source {
        "Save this draft before starting"
    } else if data.draft_dirty {
        "Save changes before starting"
    } else if config_valid {
        "Start tunnel"
    } else {
        "Select a valid config first"
    };
    let off_tooltip = if app.runtime.running {
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
                .selected(app.runtime.running)
                .disabled(!can_start)
                .tooltip(on_tooltip)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.handle_start_stop(window, cx);
                })),
        )
        .child(
            Button::new("mode-off")
                .label("Off")
                .selected(!app.runtime.running)
                .disabled(!can_stop)
                .tooltip(off_tooltip)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.handle_start_stop(window, cx);
                })),
        );

    let status_chip = {
        let (label, dot_color, text_color, bg, border) = if app.runtime.running {
            (
                "Connected",
                cx.theme().success,
                cx.theme().success,
                cx.theme().success.alpha(0.16),
                cx.theme().success.alpha(0.3),
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
        .tooltip("Open preferences")
        .on_click(cx.listener(|this, _, window, cx| {
            this.request_sidebar_active(SidebarItem::Advanced, window, cx);
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
                .child(h_flex().items_center().gap_2().child(theme_toggle))
                .child(vertical_divider(cx))
                .child(
                    h_flex().items_center().gap_2().child(
                        div()
                            .text_xs()
                            .font_family(ui_font)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(cx.theme().muted_foreground)
                            .child("Tunnel"),
                    ),
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

fn top_bar_copy(app: &WgApp) -> (&'static str, String, String) {
    match app.ui_session.sidebar_active {
        SidebarItem::Overview => (
            "DASHBOARD",
            "Overview".to_string(),
            if app.runtime.running {
                format!(
                    "Monitoring {} and the currently selected config.",
                    app.runtime
                        .running_name
                        .as_deref()
                        .unwrap_or("the active tunnel")
                )
            } else {
                "Runtime health, traffic, and selected config context.".to_string()
            },
        ),
        SidebarItem::Proxies => (
            "LIBRARY",
            "Proxies".to_string(),
            "Browse saved configs and current tunnel selection.".to_string(),
        ),
        SidebarItem::Logs => (
            "MONITORING",
            "Logs".to_string(),
            "Follow runtime events and backend activity.".to_string(),
        ),
        SidebarItem::Dns => (
            "NETWORK",
            "DNS".to_string(),
            "Inspect resolver policy and applied presets.".to_string(),
        ),
        SidebarItem::About => (
            "SYSTEM",
            "About".to_string(),
            "Capabilities, release notes, and local environment details.".to_string(),
        ),
        SidebarItem::Advanced => (
            "SETTINGS",
            "Preferences".to_string(),
            "Manage appearance, defaults, and system integration.".to_string(),
        ),
        SidebarItem::RouteMap => (
            "ANALYSIS",
            "Route Map".to_string(),
            "Explain how the selected config reaches each destination.".to_string(),
        ),
        SidebarItem::Configs => (
            "WORKSPACE",
            "Configs".to_string(),
            "Edit, validate, and save tunnel definitions.".to_string(),
        ),
        _ => (
            "WORKSPACE",
            "Panel".to_string(),
            "Current application section.".to_string(),
        ),
    }
}
