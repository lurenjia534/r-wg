use gpui::{
    div, prelude::FluentBuilder as _, px, Axis, Context, Div, Entity, ParentElement, SharedString,
    Styled, Window,
};
use gpui_component::theme::{Theme, ThemeMode};
use gpui_component::{
    button::{Button, ButtonGroup, ButtonVariant, ButtonVariants},
    dialog::DialogButtonProps,
    group_box::GroupBoxVariant,
    h_flex,
    setting::{SettingField, SettingGroup, SettingItem, SettingPage, Settings},
    v_flex, ActiveTheme as _, Disableable as _, Selectable, Sizable as _, StyledExt as _,
    WindowExt,
};
use r_wg::backend::wg::PrivilegedServiceAction;
use r_wg::dns::{DnsMode, DnsPreset};
use std::time::{Duration, SystemTime};

use super::super::state::{BackendDiagnostic, BackendHealth, RightTab, TrafficPeriod, WgApp};
use super::widgets::backend_status_tag;

pub(crate) fn render_advanced(_app: &mut WgApp, cx: &mut Context<WgApp>) -> Div {
    let app_handle = cx.entity();

    let general_page = SettingPage::new("General")
        .description("Appearance and workspace defaults for the desktop shell.")
        .default_open(true)
        .group(
            SettingGroup::new()
                .title("Appearance")
                .description("Use the same compact control language as the top bar.")
                .item(theme_mode_item(app_handle.clone())),
        )
        .group(
            SettingGroup::new()
                .title("Workspace")
                .description(
                    "Remember which panel opens first when you return to the configs screen.",
                )
                .item(right_tab_item(app_handle.clone())),
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
                .child(div().h_full().w_full().max_w(px(1180.0)).child(settings)),
        )
}

fn render_settings_shell_header(cx: &mut Context<WgApp>) -> Div {
    div().px_5().py_4().border_b_1().border_color(cx.theme().border).child(
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
                    .child(
                        "Apply appearance, remembered defaults, and system control from one workspace.",
                    ),
            ),
    )
}

fn theme_mode_item(app: Entity<WgApp>) -> SettingItem {
    SettingItem::new(
        "Theme",
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
                                    if app.ui_prefs.theme_mode != ThemeMode::Light {
                                        app.ui_prefs.theme_mode = ThemeMode::Light;
                                        Theme::change(ThemeMode::Light, None, cx);
                                        cx.refresh_windows();
                                        app.persist_state_async(cx);
                                    }
                                    cx.notify();
                                });
                            }),
                    )
                    .child(
                        Button::new("advanced-theme-dark")
                            .label("Dark")
                            .selected(current == ThemeMode::Dark)
                            .on_click(move |_, _, cx| {
                                let _ = dark_handle.update(cx, |app, cx| {
                                    if app.ui_prefs.theme_mode != ThemeMode::Dark {
                                        app.ui_prefs.theme_mode = ThemeMode::Dark;
                                        Theme::change(ThemeMode::Dark, None, cx);
                                        cx.refresh_windows();
                                        app.persist_state_async(cx);
                                    }
                                    cx.notify();
                                });
                            }),
                    ),
            )
        }),
    )
    .description("Keep theme selection aligned with the toolbar controls.")
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
    let options = dns_preset_options();
    let get_handle = app.clone();
    let set_handle = app;

    SettingItem::new(
        "DNS Preset",
        SettingField::dropdown(
            options,
            move |cx| dns_preset_value(get_handle.read(cx).ui_prefs.dns_preset),
            move |value, cx| {
                let next = dns_preset_from_value(&value);
                let _ = set_handle.update(cx, |app, cx| {
                    app.set_dns_preset_pref(next, cx);
                });
            },
        ),
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
    .description("Used as the initial range when the traffic view opens.")
}

fn right_tab_item(app: Entity<WgApp>) -> SettingItem {
    SettingItem::new(
        "Preferred Right Panel",
        SettingField::render(move |_, _window, cx| {
            let current = app.read(cx).ui_prefs.preferred_right_tab;
            let status_handle = app.clone();
            let logs_handle = app.clone();

            div().child(
                ButtonGroup::new("advanced-right-panel-default")
                    .outline()
                    .small()
                    .compact()
                    .child(
                        Button::new("advanced-right-panel-status")
                            .label("Status")
                            .selected(current == RightTab::Status)
                            .on_click(move |_, _, cx| {
                                let _ = status_handle.update(cx, |app, cx| {
                                    app.set_preferred_right_tab(RightTab::Status, cx);
                                });
                            }),
                    )
                    .child(
                        Button::new("advanced-right-panel-logs")
                            .label("Logs")
                            .selected(current == RightTab::Logs)
                            .on_click(move |_, _, cx| {
                                let _ = logs_handle.update(cx, |app, cx| {
                                    app.set_preferred_right_tab(RightTab::Logs, cx);
                                });
                            }),
                    ),
            )
        }),
    )
    .description("Used as the initial panel when the configs workspace opens.")
}

fn privileged_backend_item(app: Entity<WgApp>) -> SettingItem {
    SettingItem::new(
        "Privileged Backend",
        SettingField::render(move |_, _window, cx| {
            render_privileged_backend_panel(app.clone(), cx)
        }),
    )
    .layout(Axis::Vertical)
    .description("Service bridge for tunnel startup, routes, DNS changes, and backend repair.")
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
    .description("Compact action guidance that stays inside System instead of becoming another boxed card.")
}

fn render_privileged_backend_panel(app: Entity<WgApp>, cx: &mut gpui::App) -> Div {
    let diagnostic = app.read(cx).ui.backend.clone();
    let busy = diagnostic.is_busy();
    let note = backend_recovery_note(&diagnostic);

    let refresh_handle = app.clone();
    let install_handle = app.clone();
    let repair_handle = app.clone();
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
                                .child(div().text_sm().font_semibold().child("Service Status"))
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
                                            let diagnostic = diagnostic.clone();
                                            move |_, _, cx| {
                                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                                    format!(
                                                        "Privileged Backend: {}\n{}",
                                                        diagnostic.summary(),
                                                        diagnostic.detail
                                                    ),
                                                ));
                                            }
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
        BackendHealth::Running => {
            "Healthy state stays quiet. Maintenance actions appear when the helper needs attention."
        }
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
