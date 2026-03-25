use gpui::prelude::FluentBuilder as _;
use std::env::consts::{ARCH, OS};
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Local};
use gpui::{
    div, Axis, Div, Entity, IntoElement, ParentElement, SharedString, Styled, Timer, Window,
};
use gpui_component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_component::description_list::DescriptionList;
use gpui_component::dialog::DialogButtonProps;
use gpui_component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_component::setting::{SettingField, SettingItem};
use gpui_component::{
    h_flex, v_flex, ActiveTheme as _, Disableable as _, Sizable as _, WindowExt,
};
use r_wg::backend::wg::PrivilegedServiceAction;
use r_wg::dns::{DnsMode, DnsPreset};

use crate::ui::state::{
    BackendDiagnostic, BackendHealth, SidebarItem, WgApp,
};
use crate::ui::view::widgets::backend_status_tag;

// DNS preset field and privileged backend diagnostics/recovery UI.

pub(super) fn privileged_backend_item(app: Entity<WgApp>) -> SettingItem {
    SettingItem::new(
        "Service Status",
        SettingField::render(move |_, window, cx| {
            render_privileged_backend_panel(app.clone(), window, cx)
        }),
    )
    .layout(Axis::Vertical)
}

pub(super) fn troubleshooting_item() -> SettingItem {
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

pub(super) fn render_dns_preset_field(app: Entity<WgApp>, cx: &mut gpui::App) -> Div {
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
                                        set_handle.update(cx, |app, cx| {
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
                                            refresh_handle.update(cx, |app, cx| {
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
                                                repair_handle.update(cx, |app, cx| {
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
                                                copy_handle.update(cx, |app, cx| {
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
                                            details_handle.update(cx, |open, _| {
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
                remove_handle.update(cx, |app, cx| {
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
    ticker_running.update(cx, |running, _| {
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

pub(super) fn backend_recommended_action(diagnostic: &BackendDiagnostic) -> &'static str {
    match diagnostic.health {
        BackendHealth::Running => {
            "Repair or Remove can stop the running helper before applying system changes."
        }
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

pub(super) fn should_show_repair_action(diagnostic: &BackendDiagnostic) -> bool {
    diagnostic.allows_action(PrivilegedServiceAction::Repair)
}

pub(super) fn should_show_remove_action(diagnostic: &BackendDiagnostic) -> bool {
    diagnostic.allows_action(PrivilegedServiceAction::Remove)
}

pub(super) fn backend_recovery_note(diagnostic: &BackendDiagnostic) -> Option<SharedString> {
    let note = match diagnostic.health {
        BackendHealth::Running => {
            "Repair or Remove can stop the running helper first when you need to recover or uninstall it."
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

pub(super) fn dns_mode_options() -> Vec<(SharedString, SharedString)> {
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

pub(super) fn dns_mode_value(mode: DnsMode) -> SharedString {
    match mode {
        DnsMode::FollowConfig => shared("follow_config"),
        DnsMode::UseSystemDns => shared("system"),
        DnsMode::AutoFillMissingFamilies => shared("auto_fill"),
        DnsMode::OverrideAll => shared("override"),
    }
}

pub(super) fn dns_mode_from_value(value: &SharedString) -> DnsMode {
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
