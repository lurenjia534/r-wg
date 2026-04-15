use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonGroup, ButtonVariants as _},
    h_flex,
    switch::Switch,
    ActiveTheme as _, Disableable as _, Icon, IconName, Selectable, Sizable as _, Size,
};

use super::super::features::themes::AppearancePolicy;
use super::super::state::{SidebarItem, WgApp};
use super::shared::ViewData;

/// 顶部工具栏只负责全局控制，不承载页面标题语义。
pub(crate) fn render_top_bar(app: &mut WgApp, data: &ViewData, cx: &mut Context<WgApp>) -> Div {
    let config_valid = data.parse_error.is_none() && data.parsed_config.is_some();
    let can_start = config_valid
        && data.has_saved_source
        && !data.draft_dirty
        && !app.runtime.running
        && !app.runtime.busy;
    let can_stop = app.runtime.running && !app.runtime.busy;

    let appearance_policy = app.ui_prefs.appearance_policy;
    let focus_name = app
        .runtime
        .running_name
        .as_deref()
        .or_else(|| {
            app.selection
                .selected_id
                .and_then(|id| app.configs.get_by_id(id))
                .map(|config| config.name.as_str())
        })
        .unwrap_or("No tunnel");
    let focus_name: SharedString = focus_name.to_string().into();

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

    let tunnel_tooltip = if !data.has_saved_source {
        "Save this draft before starting"
    } else if data.draft_dirty {
        "Save changes before starting"
    } else if app.runtime.running {
        "Stop tunnel"
    } else if config_valid {
        "Start tunnel"
    } else {
        "Select a valid config first"
    };

    let tunnel_toggle = Switch::new("tunnel-toggle")
        .checked(app.runtime.running)
        .with_size(Size::Small)
        .disabled(!can_start && !can_stop)
        .tooltip(tunnel_tooltip)
        .on_click(cx.listener(|this, _, window, cx| {
            this.command_toggle_tunnel(window, cx);
        }));

    let settings_button = Button::new("settings")
        .ghost()
        .small()
        .icon(Icon::new(IconName::Settings).size_5())
        .tooltip("Open preferences")
        .on_click(cx.listener(|this, _, window, cx| {
            this.command_open_sidebar_item(SidebarItem::Advanced, window, cx);
        }));

    h_flex().w_full().justify_end().child(
        toolbar_shell(cx).child(
            h_flex()
                .items_center()
                .gap_3()
                .child(div().flex_shrink_0().child(theme_toggle))
                .child(toolbar_divider(cx))
                .child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .min_w(px(0.0))
                        .max_w(px(280.0))
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(cx.theme().muted_foreground)
                                .child("Tunnel"),
                        )
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::SEMIBOLD)
                                .truncate()
                                .child(focus_name),
                        )
                        .child(render_status_chip(app, cx))
                        .when(
                            app.runtime.running && app.runtime.quantum_protected,
                            |this| this.child(render_quantum_chip(cx)),
                        )
                        .child(tunnel_toggle),
                )
                .child(toolbar_divider(cx))
                .child(div().flex_shrink_0().child(settings_button)),
        ),
    )
}

fn render_status_chip(app: &WgApp, cx: &mut Context<WgApp>) -> Div {
    let (label, bg, border, dot, text) = if app.runtime.busy {
        (
            "Updating",
            cx.theme().warning.alpha(0.16),
            cx.theme().warning.alpha(0.28),
            cx.theme().warning,
            cx.theme().warning,
        )
    } else if app.runtime.running {
        (
            "Connected",
            cx.theme().success.alpha(0.16),
            cx.theme().success.alpha(0.28),
            cx.theme().success,
            cx.theme().success,
        )
    } else {
        (
            "Idle",
            cx.theme()
                .background
                .alpha(if cx.theme().is_dark() { 0.44 } else { 0.86 }),
            cx.theme()
                .border
                .alpha(if cx.theme().is_dark() { 0.3 } else { 0.9 }),
            cx.theme().muted_foreground,
            cx.theme().muted_foreground,
        )
    };

    h_flex()
        .items_center()
        .gap_1p5()
        .px_2()
        .py_1()
        .rounded_full()
        .border_1()
        .border_color(border)
        .bg(bg)
        .child(div().size(px(5.0)).rounded_full().bg(dot))
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(text)
                .child(label),
        )
}

fn render_quantum_chip(cx: &mut Context<WgApp>) -> Div {
    h_flex()
        .items_center()
        .gap_1p5()
        .px_2()
        .py_1()
        .rounded_full()
        .border_1()
        .border_color(cx.theme().sidebar_primary.alpha(0.28))
        .bg(cx.theme().sidebar_primary.alpha(0.14))
        .child(
            div()
                .size(px(5.0))
                .rounded_full()
                .bg(cx.theme().sidebar_primary),
        )
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(cx.theme().sidebar_primary)
                .child("Quantum"),
        )
}

fn toolbar_shell(cx: &mut Context<WgApp>) -> Div {
    div()
        .px_3()
        .py_2()
        .rounded_lg()
        .border_1()
        .border_color(
            cx.theme()
                .border
                .alpha(if cx.theme().is_dark() { 0.42 } else { 0.9 }),
        )
        .bg(cx
            .theme()
            .background
            .alpha(if cx.theme().is_dark() { 0.42 } else { 0.88 }))
        .when(cx.theme().shadow, |this| this.shadow_sm())
}

fn toolbar_divider(cx: &mut Context<WgApp>) -> Div {
    div()
        .w(px(1.0))
        .h(px(22.0))
        .bg(cx
            .theme()
            .border
            .alpha(if cx.theme().is_dark() { 0.3 } else { 0.8 }))
}
