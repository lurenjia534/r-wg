use std::env::consts::{ARCH, OS};
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Local};
use gpui::{
    div, prelude::FluentBuilder as _, px, Axis, Context, Div, Entity, Hsla, IntoElement,
    ParentElement, SharedString, Styled, Timer, Window,
};
use gpui_component::theme::{Theme, ThemeConfig, ThemeMode};
use gpui_component::{
    button::{Button, ButtonGroup, ButtonVariant, ButtonVariants},
    description_list::DescriptionList,
    dialog::DialogButtonProps,
    group_box::GroupBoxVariant,
    h_flex,
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
    setting::{SettingField, SettingGroup, SettingItem, SettingPage, Settings},
    v_flex, ActiveTheme as _, Disableable as _, Selectable, Sizable as _, StyledExt as _,
    WindowExt,
};
use r_wg::backend::wg::PrivilegedServiceAction;
use r_wg::dns::{DnsMode, DnsPreset};

use super::super::state::{
    BackendDiagnostic, BackendHealth, ConfigInspectorTab, SidebarItem, TrafficPeriod, WgApp,
};
use super::super::themes;
use super::widgets::backend_status_tag;

pub(crate) fn render_advanced(_app: &mut WgApp, cx: &mut Context<WgApp>) -> Div {
    let app_handle = cx.entity();

    let general_page = SettingPage::new("General")
        .description("Appearance and remembered app defaults.")
        .default_open(true)
        .group(
            SettingGroup::new()
                .title("Appearance")
                .description(
                    "Keep mode switching fast while storing a separate palette for light and dark.",
                )
                .item(theme_mode_item(app_handle.clone()))
                .item(theme_palette_item(app_handle.clone(), ThemeMode::Light))
                .item(theme_palette_item(app_handle.clone(), ThemeMode::Dark))
                .item(reset_theme_item(app_handle.clone()))
                .item(theme_preview_item(app_handle.clone())),
        )
        .group(
            SettingGroup::new()
                .title("Workspace")
                .description("Choose which right-side panel opens first in Configs.")
                .item(inspector_tab_item(app_handle.clone())),
        );

    let network_page = SettingPage::new("Network")
        .description("Defaults used when tunnel configs do not fully define DNS behavior.")
        .default_open(true)
        .group(
            SettingGroup::new()
                .title("DNS")
                .description("Keep DNS handling predictable across imported configs.")
                .item(dns_mode_item(app_handle.clone()))
                .item(dns_preset_item(app_handle.clone())),
        );

    let monitoring_page = SettingPage::new("Monitoring")
        .description("Remembered monitoring behavior and chart defaults.")
        .default_open(true)
        .group(
            SettingGroup::new()
                .title("Logs")
                .description("Control how the runtime log viewer behaves.")
                .item(log_auto_follow_item(app_handle.clone())),
        )
        .group(
            SettingGroup::new()
                .title("Traffic")
                .description("Choose the default range for charts and summaries.")
                .item(traffic_period_item(app_handle.clone())),
        );

    let system_page = SettingPage::new("System")
        .description(
            "Manage the helper service required for routes, DNS changes, and tunnel startup.",
        )
        .default_open(true)
        .group(
            SettingGroup::new()
                .title("Privileged Backend")
                .description("Helper service status, diagnostics, and recovery actions.")
                .item(privileged_backend_item(app_handle.clone()))
                .item(troubleshooting_item()),
        );

    let settings = Settings::new("advanced-settings")
        .with_group_variant(GroupBoxVariant::Fill)
        .sidebar_width(px(210.0))
        .sidebar_style(&settings_sidebar_style(cx))
        .page(general_page)
        .page(network_page)
        .page(monitoring_page)
        .page(system_page);

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.0))
        .rounded_lg()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().tiles)
        .overflow_hidden()
        .child(render_settings_shell_header(cx))
        .child(
            div()
                .flex_1()
                .min_h(px(0.0))
                .w_full()
                .flex()
                .justify_center()
                .child(div().h_full().w_full().max_w(px(1040.0)).child(settings)),
        )
}

fn render_settings_shell_header(cx: &mut Context<WgApp>) -> Div {
    div()
        .px_5()
        .py_4()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            v_flex()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .font_semibold()
                        .text_color(cx.theme().muted_foreground)
                        .child("SETTINGS"),
                )
                .child(div().text_xl().font_semibold().child("Preferences"))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("Manage appearance, defaults, and system integration in one place."),
                ),
        )
}

fn settings_sidebar_style(cx: &mut Context<WgApp>) -> gpui::StyleRefinement {
    let mut style = div()
        .bg(cx.theme().sidebar.alpha(0.72))
        .border_color(cx.theme().sidebar_border.alpha(0.28));
    style.style().clone()
}

fn theme_mode_item(app: Entity<WgApp>) -> SettingItem {
    SettingItem::new(
        "Theme Mode",
        SettingField::render(move |_, _window, cx| {
            let current = app.read(cx).ui_prefs.theme_mode;
            let light_handle = app.clone();
            let dark_handle = app.clone();

            div().child(
                ButtonGroup::new("advanced-theme-mode")
                    .outline()
                    .small()
                    .compact()
                    .child(
                        Button::new("advanced-theme-light")
                            .label("Light")
                            .selected(current == ThemeMode::Light)
                            .on_click(move |_, _, cx| {
                                let _ = light_handle.update(cx, |app, cx| {
                                    app.set_theme_mode_pref(ThemeMode::Light, None, cx);
                                });
                            }),
                    )
                    .child(
                        Button::new("advanced-theme-dark")
                            .label("Dark")
                            .selected(current == ThemeMode::Dark)
                            .on_click(move |_, _, cx| {
                                let _ = dark_handle.update(cx, |app, cx| {
                                    app.set_theme_mode_pref(ThemeMode::Dark, None, cx);
                                });
                            }),
                    ),
            )
        }),
    )
    .description("Keep theme selection aligned with the toolbar controls.")
}

fn theme_palette_item(app: Entity<WgApp>, mode: ThemeMode) -> SettingItem {
    let title = match mode {
        ThemeMode::Light => "Light Palette",
        ThemeMode::Dark => "Dark Palette",
    };
    let description = match mode {
        ThemeMode::Light => "Used when the quick toggle or app state returns to light mode.",
        ThemeMode::Dark => "Used when the quick toggle or app state returns to dark mode.",
    };

    SettingItem::new(
        title,
        SettingField::render(move |_, _window, cx| {
            render_theme_palette_field(app.clone(), mode, cx)
        }),
    )
    .description(description)
}

fn reset_theme_item(app: Entity<WgApp>) -> SettingItem {
    SettingItem::new(
        "Reset Palettes",
        SettingField::render(move |_, _window, _cx| {
            div().child(
                Button::new("advanced-theme-reset")
                    .label("Use Default Light and Default Dark")
                    .outline()
                    .small()
                    .compact()
                    .on_click({
                        let app = app.clone();
                        move |_, window, cx| {
                            let _ = app.update(cx, |app, cx| {
                                app.reset_theme_prefs(Some(window), cx);
                            });
                        }
                    }),
            )
        }),
    )
    .description("Clear stored palette names and fall back to the registry defaults for each mode.")
}

fn theme_preview_item(app: Entity<WgApp>) -> SettingItem {
    SettingItem::new(
        "Preview",
        SettingField::render(move |_, _window, cx| render_theme_preview_field(app.clone(), cx)),
    )
    .layout(Axis::Vertical)
    .description(
        "Preview the currently stored light and dark palettes without leaving Preferences.",
    )
}

fn render_theme_palette_field(app: Entity<WgApp>, mode: ThemeMode, cx: &mut gpui::App) -> Div {
    let preferred_name = app
        .read(cx)
        .ui_prefs
        .theme_palette_name(mode)
        .map(|name| name.to_string());
    let current_label =
        themes::resolved_theme_name(mode, preferred_name.as_deref(), cx).to_string();
    let selected_name = preferred_name.clone();
    let button_id = match mode {
        ThemeMode::Light => "advanced-theme-light-palette",
        ThemeMode::Dark => "advanced-theme-dark-palette",
    };
    let set_handle = app;

    div().child(
        Button::new(button_id)
            .label(current_label)
            .outline()
            .small()
            .compact()
            .dropdown_caret(true)
            .dropdown_menu_with_anchor(gpui::Corner::TopRight, move |menu: PopupMenu, _, cx| {
                let menu =
                    themes::available_themes(mode, cx)
                        .into_iter()
                        .fold(menu, |menu, theme| {
                            let checked = selected_name
                                .as_deref()
                                .map(|selected| theme.name.eq_ignore_ascii_case(selected))
                                .unwrap_or(false);
                            menu.item(
                                PopupMenuItem::new(theme.name.to_string())
                                    .checked(checked)
                                    .on_click({
                                        let set_handle = set_handle.clone();
                                        let name = theme.name.clone();
                                        move |_, window, cx| {
                                            let _ = set_handle.update(cx, |app, cx| {
                                                app.set_theme_palette_pref(
                                                    mode,
                                                    Some(name.clone()),
                                                    Some(window),
                                                    cx,
                                                );
                                            });
                                        }
                                    }),
                            )
                        });

                menu.item(PopupMenuItem::separator()).item(
                    PopupMenuItem::new("Use Default")
                        .checked(selected_name.is_none())
                        .on_click({
                            let set_handle = set_handle.clone();
                            move |_, window, cx| {
                                let _ = set_handle.update(cx, |app, cx| {
                                    app.set_theme_palette_pref(mode, None, Some(window), cx);
                                });
                            }
                        }),
                )
            }),
    )
}

fn render_theme_preview_field(app: Entity<WgApp>, cx: &mut gpui::App) -> Div {
    let (light_name, dark_name) = {
        let app = app.read(cx);
        (
            app.ui_prefs
                .theme_palette_name(ThemeMode::Light)
                .map(|name| name.to_string()),
            app.ui_prefs
                .theme_palette_name(ThemeMode::Dark)
                .map(|name| name.to_string()),
        )
    };
    let light_preview = theme_preview_tokens(themes::resolve_theme_config(
        ThemeMode::Light,
        light_name.as_deref(),
        cx,
    ));
    let dark_preview = theme_preview_tokens(themes::resolve_theme_config(
        ThemeMode::Dark,
        dark_name.as_deref(),
        cx,
    ));

    v_flex()
        .w_full()
        .gap_3()
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child("Status colors stay semantic. Palettes only change the surrounding surfaces and emphasis."),
        )
        .child(
            h_flex()
                .items_start()
                .flex_wrap()
                .gap_3()
                .child(render_theme_preview_card(&light_preview))
                .child(render_theme_preview_card(&dark_preview)),
        )
}

struct ThemePreviewTokens {
    mode: ThemeMode,
    name: SharedString,
    background: Hsla,
    panel: Hsla,
    border: Hsla,
    foreground: Hsla,
    muted_foreground: Hsla,
    accent: Hsla,
    accent_foreground: Hsla,
    success: Hsla,
    success_foreground: Hsla,
    danger: Hsla,
    danger_foreground: Hsla,
    input_border: Hsla,
}

fn theme_preview_tokens(config: std::rc::Rc<ThemeConfig>) -> ThemePreviewTokens {
    let mut theme = Theme::default();
    theme.apply_config(&config);

    ThemePreviewTokens {
        mode: config.mode,
        name: config.name.clone(),
        background: theme.background,
        panel: theme.group_box,
        border: theme.border,
        foreground: theme.foreground,
        muted_foreground: theme.muted_foreground,
        accent: theme.accent,
        accent_foreground: theme.accent_foreground,
        success: theme.success,
        success_foreground: theme.success_foreground,
        danger: theme.danger,
        danger_foreground: theme.danger_foreground,
        input_border: theme.input,
    }
}

fn render_theme_preview_card(preview: &ThemePreviewTokens) -> Div {
    let mode_label = match preview.mode {
        ThemeMode::Light => "LIGHT",
        ThemeMode::Dark => "DARK",
    };

    div()
        .flex_1()
        .min_w(px(260.0))
        .rounded(px(18.0))
        .border_1()
        .border_color(preview.border)
        .bg(preview.background)
        .p_4()
        .child(
            v_flex()
                .gap_3()
                .child(
                    h_flex()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .child(
                            v_flex()
                                .gap_1()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_semibold()
                                        .text_color(preview.muted_foreground)
                                        .child(mode_label),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .font_semibold()
                                        .text_color(preview.foreground)
                                        .child(preview.name.clone()),
                                ),
                        )
                        .child(
                            div()
                                .px_2()
                                .py_1()
                                .rounded_full()
                                .bg(preview.accent.alpha(0.22))
                                .text_color(preview.accent)
                                .text_xs()
                                .font_semibold()
                                .child("Preview"),
                        ),
                )
                .child(
                    div()
                        .rounded(px(14.0))
                        .border_1()
                        .border_color(preview.border)
                        .bg(preview.panel)
                        .p_3()
                        .child(
                            v_flex()
                                .gap_3()
                                .child(
                                    v_flex()
                                        .gap_1()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_semibold()
                                                .text_color(preview.foreground)
                                                .child("Infrastructure Theme"),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(preview.muted_foreground)
                                                .child("Readable surfaces, restrained contrast, and stable status cues."),
                                        ),
                                )
                                .child(
                                    h_flex()
                                        .items_center()
                                        .gap_2()
                                        .child(
                                            div()
                                                .px_3()
                                                .py_1()
                                                .rounded(px(10.0))
                                                .bg(preview.accent)
                                                .text_color(preview.accent_foreground)
                                                .text_xs()
                                                .font_semibold()
                                                .child("Primary"),
                                        )
                                        .child(
                                            div()
                                                .px_3()
                                                .py_1()
                                                .rounded(px(10.0))
                                                .border_1()
                                                .border_color(preview.border)
                                                .bg(preview.background)
                                                .text_color(preview.foreground)
                                                .text_xs()
                                                .font_semibold()
                                                .child("Secondary"),
                                        ),
                                )
                                .child(
                                    div()
                                        .rounded(px(10.0))
                                        .border_1()
                                        .border_color(preview.input_border)
                                        .bg(preview.background)
                                        .px_3()
                                        .py_2()
                                        .text_xs()
                                        .text_color(preview.muted_foreground)
                                        .child("DNS override, health notes, and other form surfaces."),
                                )
                                .child(
                                    h_flex()
                                        .items_center()
                                        .gap_2()
                                        .flex_wrap()
                                        .child(preview_color_chip("Accent", preview.accent, preview.accent_foreground))
                                        .child(preview_color_chip("Success", preview.success, preview.success_foreground))
                                        .child(preview_color_chip("Danger", preview.danger, preview.danger_foreground)),
                                )
                                .child(
                                    h_flex()
                                        .items_center()
                                        .gap_2()
                                        .flex_wrap()
                                        .child(preview_swatch("Canvas", preview.background, preview.foreground))
                                        .child(preview_swatch("Panel", preview.panel, preview.foreground))
                                        .child(preview_swatch("Border", preview.border, preview.foreground))
                                        .child(preview_swatch("Input", preview.input_border, preview.foreground)),
                                ),
                        ),
                ),
        )
}

fn preview_color_chip(label: &'static str, background: Hsla, foreground: Hsla) -> Div {
    div()
        .px_3()
        .py_1()
        .rounded_full()
        .bg(background)
        .text_color(foreground)
        .text_xs()
        .font_semibold()
        .child(label)
}

fn preview_swatch(label: &'static str, color: Hsla, text_color: Hsla) -> Div {
    h_flex()
        .items_center()
        .gap_2()
        .child(div().size(px(12.0)).rounded_full().bg(color))
        .child(div().text_xs().text_color(text_color).child(label))
}

fn log_auto_follow_item(app: Entity<WgApp>) -> SettingItem {
    let get_handle = app.clone();
    let set_handle = app;

    SettingItem::new(
        "Auto Follow Logs",
        SettingField::switch(
            move |cx| get_handle.read(cx).ui_prefs.log_auto_follow,
            move |value, cx| {
                let _ = set_handle.update(cx, |app, cx| {
                    app.set_log_auto_follow_pref(value, cx);
                });
            },
        ),
    )
    .description("Keep the log pane pinned to the latest runtime events.")
}

fn dns_mode_item(app: Entity<WgApp>) -> SettingItem {
    let options = dns_mode_options();
    let get_handle = app.clone();
    let set_handle = app;

    SettingItem::new(
        "DNS Mode",
        SettingField::dropdown(
            options,
            move |cx| dns_mode_value(get_handle.read(cx).ui_prefs.dns_mode),
            move |value, cx| {
                let next = dns_mode_from_value(&value);
                let _ = set_handle.update(cx, |app, cx| {
                    app.set_dns_mode_pref(next, cx);
                });
            },
        ),
    )
    .description("Choose whether config DNS, system DNS, or presets take precedence.")
}

fn dns_preset_item(app: Entity<WgApp>) -> SettingItem {
    SettingItem::new(
        "DNS Preset",
        SettingField::render(move |_, _window, cx| render_dns_preset_field(app.clone(), cx)),
    )
    .description("Only used when DNS mode fills or overrides resolver records.")
}

fn traffic_period_item(app: Entity<WgApp>) -> SettingItem {
    SettingItem::new(
        "Preferred Traffic Range",
        SettingField::render(move |_, _window, cx| {
            let current = app.read(cx).ui_prefs.preferred_traffic_period;
            let today_handle = app.clone();
            let month_handle = app.clone();
            let last_month_handle = app.clone();

            div().child(
                ButtonGroup::new("advanced-traffic-period")
                    .outline()
                    .small()
                    .compact()
                    .child(
                        Button::new("advanced-traffic-today")
                            .label("Today")
                            .selected(current == TrafficPeriod::Today)
                            .on_click(move |_, _, cx| {
                                let _ = today_handle.update(cx, |app, cx| {
                                    app.set_preferred_traffic_period(TrafficPeriod::Today, cx);
                                });
                            }),
                    )
                    .child(
                        Button::new("advanced-traffic-this-month")
                            .label("This Month")
                            .selected(current == TrafficPeriod::ThisMonth)
                            .on_click(move |_, _, cx| {
                                let _ = month_handle.update(cx, |app, cx| {
                                    app.set_preferred_traffic_period(TrafficPeriod::ThisMonth, cx);
                                });
                            }),
                    )
                    .child(
                        Button::new("advanced-traffic-last-month")
                            .label("Last Month")
                            .selected(current == TrafficPeriod::LastMonth)
                            .on_click(move |_, _, cx| {
                                let _ = last_month_handle.update(cx, |app, cx| {
                                    app.set_preferred_traffic_period(TrafficPeriod::LastMonth, cx);
                                });
                            }),
                    ),
            )
        }),
    )
    .description("Applies now and stays remembered for future sessions.")
}

fn inspector_tab_item(app: Entity<WgApp>) -> SettingItem {
    SettingItem::new(
        "Inspector View",
        SettingField::render(move |_, _window, cx| {
            let current = app.read(cx).ui_prefs.preferred_inspector_tab;
            let preview_handle = app.clone();
            let diagnostics_handle = app.clone();
            let activity_handle = app.clone();

            div().child(
                ButtonGroup::new("advanced-inspector-default")
                    .outline()
                    .small()
                    .compact()
                    .child(
                        Button::new("advanced-inspector-preview")
                            .label("Preview")
                            .selected(current == ConfigInspectorTab::Preview)
                            .on_click(move |_, _, cx| {
                                let _ = preview_handle.update(cx, |app, cx| {
                                    app.set_preferred_inspector_tab(
                                        ConfigInspectorTab::Preview,
                                        cx,
                                    );
                                });
                            }),
                    )
                    .child(
                        Button::new("advanced-inspector-diagnostics")
                            .label("Diagnostics")
                            .selected(current == ConfigInspectorTab::Diagnostics)
                            .on_click(move |_, _, cx| {
                                let _ = diagnostics_handle.update(cx, |app, cx| {
                                    app.set_preferred_inspector_tab(
                                        ConfigInspectorTab::Diagnostics,
                                        cx,
                                    );
                                });
                            }),
                    )
                    .child(
                        Button::new("advanced-inspector-activity")
                            .label("Activity")
                            .selected(current == ConfigInspectorTab::Activity)
                            .on_click(move |_, _, cx| {
                                let _ = activity_handle.update(cx, |app, cx| {
                                    app.set_preferred_inspector_tab(
                                        ConfigInspectorTab::Activity,
                                        cx,
                                    );
                                });
                            }),
                    ),
            )
        }),
    )
    .description("Controls which Inspector view opens first in Configs.")
}

fn privileged_backend_item(app: Entity<WgApp>) -> SettingItem {
    SettingItem::new(
        "Service Status",
        SettingField::render(move |_, window, cx| {
            render_privileged_backend_panel(app.clone(), window, cx)
        }),
    )
    .layout(Axis::Vertical)
}

fn troubleshooting_item() -> SettingItem {
    SettingItem::new(
        "Troubleshooting",
        SettingField::render(|_, _, cx| {
            v_flex()
                .w_full()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("Refresh re-checks helper status only. No system changes."),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(
                            "Repair reinstalls helper integration and fixes protocol or permission drift.",
                        ),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("Remove uninstalls the helper integration only. Tunnel configs are kept."),
                )
        }),
    )
    .layout(Axis::Vertical)
    .description("What Refresh, Repair, and Remove do.")
}

fn render_dns_preset_field(app: Entity<WgApp>, cx: &mut gpui::App) -> Div {
    let (mode, preset) = {
        let app = app.read(cx);
        (app.ui_prefs.dns_mode, app.ui_prefs.dns_preset)
    };
    let active = dns_mode_uses_preset(mode);
    let current_label = dns_preset_label(preset);
    let set_handle = app;

    let button = Button::new("advanced-dns-preset")
        .label(current_label)
        .outline()
        .small()
        .compact()
        .disabled(!active);

    let button =
        if active {
            button
                .dropdown_caret(true)
                .dropdown_menu_with_anchor(gpui::Corner::TopRight, move |menu: PopupMenu, _, _| {
                    dns_preset_options()
                        .iter()
                        .fold(menu, |menu, (value, label)| {
                            let checked = *value == dns_preset_value(preset);
                            menu.item(PopupMenuItem::new(label.clone()).checked(checked).on_click(
                                {
                                    let set_handle = set_handle.clone();
                                    let value = value.clone();
                                    move |_, _, cx| {
                                        let next = dns_preset_from_value(&value);
                                        let _ = set_handle.update(cx, |app, cx| {
                                            app.set_dns_preset_pref(next, cx);
                                        });
                                    }
                                },
                            ))
                        })
                })
                .into_any_element()
        } else {
            button.into_any_element()
        };

    v_flex()
        .w_full()
        .gap_1()
        .child(button)
        .when(!active, |this| {
            this.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("Inactive in Follow Config and System DNS modes."),
            )
        })
}

fn render_privileged_backend_panel(
    app: Entity<WgApp>,
    window: &mut Window,
    cx: &mut gpui::App,
) -> Div {
    ensure_backend_freshness_ticker(app.clone(), window, cx);
    let diagnostic = app.read(cx).ui.backend.clone();
    let busy = diagnostic.is_busy();
    let note = backend_recovery_note(&diagnostic);
    let details_open = window.use_keyed_state("backend-details-open", cx, |_, _| false);
    let is_details_open = *details_open.read(cx);

    let refresh_handle = app.clone();
    let install_handle = app.clone();
    let repair_handle = app.clone();
    let copy_handle = app.clone();
    let details_handle = details_open.clone();
    let details_app_handle = app.clone();
    let remove_handle = app;

    div()
        .w_full()
        .child(
            v_flex()
                .gap_3()
                .child(
                    h_flex()
                        .items_start()
                        .justify_between()
                        .gap_3()
                        .flex_wrap()
                        .child(
                            v_flex()
                                .gap_1()
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(
                                            "Helper service used for tunnel startup, routes, and DNS changes.",
                                        ),
                                ),
                        )
                        .child(backend_status_tag(
                            &diagnostic,
                            SharedString::from(diagnostic.summary()),
                        ))
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(backend_checked_label(&diagnostic)),
                        ),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().foreground)
                        .child(diagnostic.detail.clone()),
                )
                .when_some(note, |this, note| {
                    this.child(
                        div()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(note),
                            ),
                    )
                })
                .child(
                    h_flex()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .flex_wrap()
                        .child(
                            h_flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    Button::new("backend-refresh-status")
                                        .label("Refresh")
                                        .outline()
                                        .small()
                                        .compact()
                                        .loading(matches!(
                                            diagnostic.health,
                                            BackendHealth::Checking
                                        ))
                                        .disabled(busy)
                                        .on_click(move |_, _, cx| {
                                            let _ = refresh_handle.update(cx, |app, cx| {
                                                app.refresh_privileged_backend_status(cx);
                                            });
                                        }),
                                )
                                .when(
                                    diagnostic.allows_action(PrivilegedServiceAction::Install),
                                    |this| {
                                        this.child(
                                            Button::new("backend-install")
                                                .label("Install")
                                                .small()
                                                .compact()
                                                .loading(diagnostic.is_working_action(
                                                    PrivilegedServiceAction::Install,
                                                ))
                                                .disabled(busy)
                                                .on_click(move |_, _, cx| {
                                                    let _ =
                                                        install_handle.update(cx, |app, cx| {
                                                            app.run_privileged_backend_action(
                                                                PrivilegedServiceAction::Install,
                                                                cx,
                                                            );
                                                        });
                                                }),
                                        )
                                    },
                                )
                                .when(should_show_repair_action(&diagnostic), |this| {
                                    this.child(
                                        Button::new("backend-repair")
                                            .label("Repair")
                                            .outline()
                                            .small()
                                            .compact()
                                            .loading(diagnostic.is_working_action(
                                                PrivilegedServiceAction::Repair,
                                            ))
                                            .disabled(busy)
                                            .on_click(move |_, _, cx| {
                                                let _ = repair_handle.update(cx, |app, cx| {
                                                    app.run_privileged_backend_action(
                                                        PrivilegedServiceAction::Repair,
                                                        cx,
                                                    );
                                                });
                                            }),
                                    )
                                })
                                .child(
                                    Button::new("backend-copy-diagnostics")
                                        .label("Copy Diagnostics")
                                        .outline()
                                        .small()
                                        .compact()
                                        .on_click({
                                            move |_, window, cx| {
                                                let _ = copy_handle.update(cx, |app, cx| {
                                                    cx.write_to_clipboard(
                                                        gpui::ClipboardItem::new_string(
                                                            build_backend_diagnostics_text(app),
                                                        ),
                                                    );
                                                    app.push_success_toast(
                                                        "Diagnostics copied",
                                                        window,
                                                        cx,
                                                    );
                                                });
                                            }
                                        }),
                                )
                                .child(
                                    Button::new("backend-toggle-details")
                                        .label(if is_details_open {
                                            "Hide Details"
                                        } else {
                                            "Details"
                                        })
                                        .outline()
                                        .small()
                                        .compact()
                                        .on_click(move |_, _, cx| {
                                            let _ = details_handle.update(cx, |open, _| {
                                                *open = !*open;
                                            });
                                        }),
                                ),
                        )
                        .when(should_show_remove_action(&diagnostic), |this| {
                            this.child(
                                Button::new("backend-remove")
                                    .label("Remove")
                                    .danger()
                                    .outline()
                                    .small()
                                    .compact()
                                    .loading(
                                        diagnostic.is_working_action(PrivilegedServiceAction::Remove),
                                    )
                                    .disabled(busy)
                                    .on_click(move |_, window, cx| {
                                        open_backend_remove_dialog(
                                            remove_handle.clone(),
                                            window,
                                            cx,
                                        );
                                    }),
                            )
                        }),
                ),
        )
        .when(is_details_open, |this| {
            this.child(render_backend_details(&details_app_handle, cx))
        })
}

fn open_backend_remove_dialog(app_handle: Entity<WgApp>, window: &mut Window, cx: &mut gpui::App) {
    window.open_dialog(cx, move |dialog, _window, cx| {
        let remove_handle = app_handle.clone();
        dialog
            .title(div().text_lg().child("Remove Privileged Backend?"))
            .confirm()
            .button_props(
                DialogButtonProps::default()
                    .ok_text("Remove")
                    .ok_variant(ButtonVariant::Danger)
                    .cancel_text("Cancel"),
            )
            .child(
                div()
                    .text_sm()
                    .child("Remove the helper integration from the operating system?"),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                    "Removes the helper integration only. Existing tunnel configs are not deleted.",
                ),
            )
            .on_ok(move |_, _window, cx| {
                let _ = remove_handle.update(cx, |app, cx| {
                    app.run_privileged_backend_action(PrivilegedServiceAction::Remove, cx);
                });
                true
            })
    });
}

fn backend_checked_label(diagnostic: &BackendDiagnostic) -> SharedString {
    match diagnostic.checked_at {
        Some(checked_at) => {
            let prefix = if diagnostic.is_busy() {
                "Last checked "
            } else {
                "Checked "
            };
            format!("{prefix}{}", format_checked_age(checked_at)).into()
        }
        None if diagnostic.is_busy() => "Checking now".into(),
        None => "Not checked yet".into(),
    }
}

fn ensure_backend_freshness_ticker(app: Entity<WgApp>, window: &mut Window, cx: &mut gpui::App) {
    let ticker_running = window.use_keyed_state("backend-freshness-ticker", cx, |_, _| false);
    if *ticker_running.read(cx) {
        return;
    }
    let _ = ticker_running.update(cx, |running, _| {
        *running = true;
    });

    cx.spawn({
        let ticker_running = ticker_running.clone();
        async move |cx| loop {
            Timer::after(Duration::from_secs(10)).await;

            let keep_running = app
                .update(cx, |app, cx| {
                    if app.ui_session.sidebar_active == SidebarItem::Advanced {
                        cx.notify();
                        true
                    } else {
                        false
                    }
                })
                .unwrap_or(false);

            if !keep_running {
                let _ = ticker_running.update(cx, |running, _| {
                    *running = false;
                });
                break;
            }
        }
    })
    .detach();
}

fn format_checked_age(checked_at: SystemTime) -> String {
    let elapsed = SystemTime::now()
        .duration_since(checked_at)
        .unwrap_or(Duration::from_secs(0));
    let seconds = elapsed.as_secs();

    match seconds {
        0..=9 => "just now".to_string(),
        10..=59 => format!("{seconds}s ago"),
        60..=3_599 => format!("{} min ago", seconds / 60),
        3_600..=86_399 => format!("{} hr ago", seconds / 3_600),
        _ => format!("{} d ago", seconds / 86_400),
    }
}

fn build_backend_diagnostics_text(app: &WgApp) -> String {
    let diagnostic = &app.ui.backend;
    let checked = diagnostic
        .checked_at
        .map(format_checked_timestamp)
        .unwrap_or_else(|| "not checked yet".to_string());
    let mut lines = vec![
        format!("App: r-wg v{}", env!("CARGO_PKG_VERSION")),
        format!("Platform: {OS} / {ARCH}"),
        format!("Health: {}", diagnostic.summary()),
        format!("Checked: {checked}"),
        format!("Integration: {}", helper_platform_detail()),
        format!("Control endpoint: {}", helper_control_endpoint()),
        format!(
            "Recommended next step: {}",
            backend_recommended_action(diagnostic)
        ),
        format!("Detail: {}", diagnostic.detail),
    ];
    if let Some(last_error) = &app.ui.backend_last_error {
        lines.push(format!("Backend last error: {last_error}"));
    }

    match diagnostic.health {
        BackendHealth::VersionMismatch { expected, actual } => {
            lines.push(format!(
                "Protocol mismatch: expected v{expected}, actual v{actual}"
            ));
        }
        BackendHealth::Unreachable => {
            lines.push(format!("Unreachable message: {}", diagnostic.detail));
        }
        _ => {}
    }

    lines.join("\n")
}

fn render_backend_details(app: &Entity<WgApp>, cx: &mut gpui::App) -> Div {
    let app = app.read(cx);
    let diagnostic = &app.ui.backend;
    let checked = diagnostic
        .checked_at
        .map(format_checked_timestamp)
        .unwrap_or_else(|| "not checked yet".to_string());
    let backend_last_error = app
        .ui
        .backend_last_error
        .as_ref()
        .map(|err| err.to_string())
        .unwrap_or_else(|| "None".to_string());

    let details = DescriptionList::new()
        .columns(1)
        .item("Integration", helper_platform_detail(), 1)
        .item("Control Endpoint", helper_control_endpoint(), 1)
        .item("Health", diagnostic.summary(), 1)
        .item("Checked", checked, 1)
        .item("Recommended", backend_recommended_action(diagnostic), 1)
        .item("Backend Last Error", backend_last_error, 1);

    let details = if let BackendHealth::VersionMismatch { expected, actual } = diagnostic.health {
        details.item(
            "Protocol",
            format!("Expected v{expected}, actual v{actual}"),
            1,
        )
    } else {
        details
    };

    let details = if matches!(diagnostic.health, BackendHealth::Unreachable) {
        details.item("Transport Error", diagnostic.detail.to_string(), 1)
    } else {
        details
    };

    div()
        .pt_2()
        .border_t_1()
        .border_color(cx.theme().border)
        .child(details)
}

fn format_checked_timestamp(checked_at: SystemTime) -> String {
    let absolute = DateTime::<Local>::from(checked_at)
        .format("%Y-%m-%d %H:%M:%S local")
        .to_string();
    format!("{absolute} ({})", format_checked_age(checked_at))
}

fn backend_recommended_action(diagnostic: &BackendDiagnostic) -> &'static str {
    match diagnostic.health {
        BackendHealth::Running => "Refresh only if you want to re-check service health.",
        BackendHealth::NotInstalled => "Install the helper integration.",
        BackendHealth::Installed => "Refresh first, then Repair if the helper stays unavailable.",
        BackendHealth::AccessDenied | BackendHealth::VersionMismatch { .. } => {
            "Repair the helper integration."
        }
        BackendHealth::Unreachable => "Refresh first, then Repair if the helper stays unreachable.",
        BackendHealth::Checking => "Wait for the current probe to finish.",
        BackendHealth::Working { .. } => "Wait for the current action to finish.",
        BackendHealth::Unknown => "Refresh to probe the helper state.",
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        BackendHealth::Unsupported => "No helper actions are available on this platform.",
    }
}

fn helper_platform_detail() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "Linux privileged service"
    }
    #[cfg(target_os = "windows")]
    {
        "Windows privileged service"
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        "No privileged helper on this platform"
    }
}

fn helper_control_endpoint() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "/run/r-wg/control.sock"
    }
    #[cfg(target_os = "windows")]
    {
        r"\\.\pipe\r-wg-control"
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        "Not available"
    }
}

fn should_show_repair_action(diagnostic: &BackendDiagnostic) -> bool {
    !matches!(diagnostic.health, BackendHealth::Running)
        && diagnostic.allows_action(PrivilegedServiceAction::Repair)
}

fn should_show_remove_action(diagnostic: &BackendDiagnostic) -> bool {
    !matches!(diagnostic.health, BackendHealth::Running)
        && diagnostic.allows_action(PrivilegedServiceAction::Remove)
}

fn backend_recovery_note(diagnostic: &BackendDiagnostic) -> Option<SharedString> {
    let note = match diagnostic.health {
        BackendHealth::NotInstalled => {
            "Install is the recommended next step before using desktop tunnel start, route, or DNS actions."
        }
        BackendHealth::Installed => {
            "The helper is installed but not currently live. Refresh first, then repair if the control channel stays unavailable."
        }
        BackendHealth::AccessDenied => {
            "Repair is the recommended next step when the helper exists but this account cannot reach it."
        }
        BackendHealth::VersionMismatch { .. } => {
            "Repair is the recommended next step when the installed helper protocol does not match this GUI build."
        }
        BackendHealth::Unreachable => {
            "Refresh re-checks the current state. Repair is the next step if the helper path or socket still looks stale."
        }
        _ => return None,
    };

    Some(note.into())
}

fn dns_mode_uses_preset(mode: DnsMode) -> bool {
    matches!(
        mode,
        DnsMode::AutoFillMissingFamilies | DnsMode::OverrideAll
    )
}

fn dns_preset_label(preset: DnsPreset) -> SharedString {
    dns_preset_options()
        .into_iter()
        .find(|(value, _)| *value == dns_preset_value(preset))
        .map(|(_, label)| label)
        .unwrap_or_else(|| SharedString::from("Preset"))
}

fn shared(value: &'static str) -> SharedString {
    SharedString::new_static(value)
}

fn dns_mode_options() -> Vec<(SharedString, SharedString)> {
    vec![
        (
            shared("follow_config"),
            shared(DnsMode::FollowConfig.label()),
        ),
        (shared("system"), shared(DnsMode::UseSystemDns.label())),
        (
            shared("auto_fill"),
            shared(DnsMode::AutoFillMissingFamilies.label()),
        ),
        (shared("override"), shared(DnsMode::OverrideAll.label())),
    ]
}

fn dns_mode_value(mode: DnsMode) -> SharedString {
    match mode {
        DnsMode::FollowConfig => shared("follow_config"),
        DnsMode::UseSystemDns => shared("system"),
        DnsMode::AutoFillMissingFamilies => shared("auto_fill"),
        DnsMode::OverrideAll => shared("override"),
    }
}

fn dns_mode_from_value(value: &SharedString) -> DnsMode {
    match value.as_ref() {
        "system" => DnsMode::UseSystemDns,
        "auto_fill" => DnsMode::AutoFillMissingFamilies,
        "override" => DnsMode::OverrideAll,
        _ => DnsMode::FollowConfig,
    }
}

fn dns_preset_options() -> Vec<(SharedString, SharedString)> {
    vec![
        (
            shared("cloudflare_standard"),
            shared("Cloudflare: Standard"),
        ),
        (shared("cloudflare_malware"), shared("Cloudflare: Malware")),
        (
            shared("cloudflare_malware_adult"),
            shared("Cloudflare: Malware + Adult"),
        ),
        (shared("adguard_default"), shared("AdGuard: Default")),
        (shared("adguard_unfiltered"), shared("AdGuard: Unfiltered")),
        (shared("adguard_family"), shared("AdGuard: Family")),
    ]
}

fn dns_preset_value(preset: DnsPreset) -> SharedString {
    match preset {
        DnsPreset::CloudflareStandard => shared("cloudflare_standard"),
        DnsPreset::CloudflareMalware => shared("cloudflare_malware"),
        DnsPreset::CloudflareMalwareAdult => shared("cloudflare_malware_adult"),
        DnsPreset::AdguardDefault => shared("adguard_default"),
        DnsPreset::AdguardUnfiltered => shared("adguard_unfiltered"),
        DnsPreset::AdguardFamily => shared("adguard_family"),
    }
}

fn dns_preset_from_value(value: &SharedString) -> DnsPreset {
    match value.as_ref() {
        "cloudflare_malware" => DnsPreset::CloudflareMalware,
        "cloudflare_malware_adult" => DnsPreset::CloudflareMalwareAdult,
        "adguard_default" => DnsPreset::AdguardDefault,
        "adguard_unfiltered" => DnsPreset::AdguardUnfiltered,
        "adguard_family" => DnsPreset::AdguardFamily,
        _ => DnsPreset::CloudflareStandard,
    }
}
