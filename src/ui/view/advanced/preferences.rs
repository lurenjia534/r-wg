use std::time::{Duration, SystemTime};

use chrono::{DateTime, Local};
use gpui::prelude::FluentBuilder as _;
use gpui::{div, Axis, Entity, ParentElement, SharedString, Styled, Window};
use gpui_component::button::{Button, ButtonGroup, ButtonVariant, ButtonVariants as _};
use gpui_component::dialog::DialogButtonProps;
use gpui_component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_component::setting::{SettingField, SettingItem};
use gpui_component::switch::Switch;
use gpui_component::{
    h_flex, v_flex, ActiveTheme as _, Disableable as _, Selectable, Sizable, Size, WindowExt,
};
use r_wg::backend::wg::{DaitaMode, QuantumMode, WireGuardBackendPreference};

use crate::ui::features::session::password_gate;
use crate::ui::i18n::{tr, Language, LanguagePreference};
use crate::ui::state::{ConfigInspectorTab, DaitaResourcesHealth, TrafficPeriod, WgApp};

use super::system::{
    dns_mode_from_value, dns_mode_options, dns_mode_value, render_dns_preset_field,
};

// Log, DNS mode, traffic range, and inspector default controls.

pub(super) fn language_item(app: Entity<WgApp>, language: Language) -> SettingItem {
    SettingItem::new(
        tr(language, "Language"),
        SettingField::render(move |_, _window, cx| {
            let current = app.read(cx).ui_prefs.language_preference;
            let system_handle = app.clone();
            let english_handle = app.clone();
            let chinese_handle = app.clone();
            let language = app.read(cx).language();

            div().child(
                ButtonGroup::new("advanced-language")
                    .outline()
                    .small()
                    .compact()
                    .child(
                        Button::new("advanced-language-system")
                            .label(tr(language, "System"))
                            .selected(current == LanguagePreference::System)
                            .on_click(move |_, _, cx| {
                                system_handle.update(cx, |app, cx| {
                                    app.set_language_preference(LanguagePreference::System, cx);
                                });
                            }),
                    )
                    .child(
                        Button::new("advanced-language-english")
                            .label(tr(language, "English"))
                            .selected(current == LanguagePreference::English)
                            .on_click(move |_, _, cx| {
                                english_handle.update(cx, |app, cx| {
                                    app.set_language_preference(LanguagePreference::English, cx);
                                });
                            }),
                    )
                    .child(
                        Button::new("advanced-language-zh-cn")
                            .label(tr(language, "Simplified Chinese"))
                            .selected(current == LanguagePreference::ChineseSimplified)
                            .on_click(move |_, _, cx| {
                                chinese_handle.update(cx, |app, cx| {
                                    app.set_language_preference(
                                        LanguagePreference::ChineseSimplified,
                                        cx,
                                    );
                                });
                            }),
                    ),
            )
        }),
    )
    .description(tr(
        language,
        "Set the UI language. System follows your OS locale when it is available.",
    ))
}

pub(super) fn log_viewer_enabled_item(app: Entity<WgApp>, language: Language) -> SettingItem {
    let get_handle = app.clone();
    let set_handle = app;

    SettingItem::new(
        tr(language, "Enable Log Viewer"),
        SettingField::switch(
            move |cx| get_handle.read(cx).ui_prefs.log_viewer_enabled,
            move |value, cx| {
                set_handle.update(cx, |app, cx| {
                    app.set_log_viewer_enabled_pref(value, cx);
                });
            },
        ),
    )
    .description(tr(
        language,
        "Collect local log lines and sync backend logs when the Logs page is open.",
    ))
}

pub(super) fn log_auto_follow_item(app: Entity<WgApp>, language: Language) -> SettingItem {
    let get_handle = app.clone();
    let set_handle = app;

    SettingItem::new(
        tr(language, "Auto Follow Logs"),
        SettingField::switch(
            move |cx| get_handle.read(cx).ui_prefs.log_auto_follow,
            move |value, cx| {
                set_handle.update(cx, |app, cx| {
                    app.set_log_auto_follow_pref(value, cx);
                });
            },
        ),
    )
    .description(tr(
        language,
        "Keep the log pane pinned to the latest runtime events.",
    ))
}

pub(super) fn connect_password_item(app: Entity<WgApp>, _language: Language) -> SettingItem {
    SettingItem::new(
        "Connect Password",
        SettingField::render(move |_, _window, cx| {
            let required = app.read(cx).ui_prefs.require_connect_password;
            let toggle_handle = app.clone();
            let manage_handle = app.clone();
            let remove_handle = app.clone();

            v_flex()
                .w_full()
                .gap_2()
                .child(
                    Switch::new("advanced-connect-password-required")
                        .label("Require password before connecting")
                        .checked(required)
                        .with_size(Size::Small)
                        .on_click(move |checked, window, cx| {
                            toggle_handle.update(cx, |app, cx| {
                                password_gate::toggle_connect_password_requirement(
                                    app, *checked, window, cx,
                                );
                            });
                        }),
                )
                .child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(
                            Button::new("advanced-connect-password-manage")
                                .label(if required {
                                    "Change Password"
                                } else {
                                    "Set Password"
                                })
                                .outline()
                                .small()
                                .compact()
                                .on_click(move |_, window, cx| {
                                    manage_handle.update(cx, |app, cx| {
                                        password_gate::open_connect_password_editor(
                                            app, window, cx,
                                        );
                                    });
                                }),
                        )
                        .child(
                            Button::new("advanced-connect-password-remove")
                                .label("Remove")
                                .danger()
                                .small()
                                .compact()
                                .on_click(move |_, window, cx| {
                                    remove_handle.update(cx, |app, cx| {
                                        password_gate::remove_connect_password(
                                            app, window, cx,
                                        );
                                    });
                                }),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(
                            "Stored in your system credential manager. When enabled, tray starts are blocked until you unlock from the main window.",
                        ),
                )
        }),
    )
    .layout(Axis::Vertical)
    .description("Require a local password before starting a WireGuard tunnel.")
}

pub(super) fn kill_switch_item(app: Entity<WgApp>, language: Language) -> SettingItem {
    SettingItem::new(
        "Kill Switch",
        SettingField::render(move |_, _window, cx| {
            let language = app.read(cx).language();
            let enabled = app.read(cx).ui_prefs.kill_switch_enabled;
            let enable_handle = app.clone();
            let disable_dialog_handle = app.clone();

            v_flex()
                .w_full()
                .gap_2()
                .child(
                    Switch::new("advanced-kill-switch")
                        .label(tr(
                            language,
                            "Block traffic outside the tunnel during protected sessions",
                        ))
                        .checked(enabled)
                        .with_size(Size::Small)
                        .on_click(move |checked, window, cx| {
                            if *checked {
                                enable_handle.update(cx, |app, cx| {
                                    app.set_kill_switch_enabled_pref(true, cx);
                                });
                            } else {
                                open_kill_switch_disable_dialog(
                                    disable_dialog_handle.clone(),
                                    window,
                                    cx,
                                );
                            }
                        }),
                )
                .when(!enabled, |this| {
                    this.child(
                        div()
                            .rounded_md()
                            .border_1()
                            .border_color(cx.theme().warning.alpha(0.42))
                            .bg(cx.theme().warning.alpha(0.08))
                            .px_3()
                            .py_2()
                            .text_sm()
                            .text_color(cx.theme().warning)
                            .child(
                                "Warning: turning this off may allow traffic or DNS requests to escape outside the VPN if the tunnel drops.",
                            ),
                    )
                })
        }),
    )
    .layout(Axis::Vertical)
    .description(
        tr(
            language,
            "Enabled by default. Full-tunnel sessions keep extra platform guardrails active to reduce leak risk.",
        ),
    )
}

pub(super) fn dns_mode_item(app: Entity<WgApp>, language: Language) -> SettingItem {
    let options = dns_mode_options();
    let get_handle = app.clone();
    let set_handle = app;

    SettingItem::new(
        tr(language, "DNS Mode"),
        SettingField::dropdown(
            options,
            move |cx| dns_mode_value(get_handle.read(cx).ui_prefs.dns_mode),
            move |value, cx| {
                let next = dns_mode_from_value(&value);
                set_handle.update(cx, |app, cx| {
                    app.set_dns_mode_pref(next, cx);
                });
            },
        ),
    )
    .description(tr(
        language,
        "Choose whether config DNS, system DNS, or presets take precedence.",
    ))
}

fn open_kill_switch_disable_dialog(
    app_handle: Entity<WgApp>,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    window.open_dialog(cx, move |dialog, _window, cx| {
        let disable_handle = app_handle.clone();
        dialog
            .title(div().text_lg().child("Turn off Kill Switch?"))
            .confirm()
            .button_props(
                DialogButtonProps::default()
                    .ok_text("Turn Off")
                    .ok_variant(ButtonVariant::Danger)
                    .cancel_text("Keep Enabled"),
            )
            .child(
                div().text_sm().child(
                    "Disabling Kill Switch can let traffic leave outside the VPN during tunnel loss or teardown.",
                ),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        "Recommended for testing only. By default this stays enabled to preserve full-tunnel leak protection.",
                    ),
            )
            .on_ok(move |_, _window, cx| {
                disable_handle.update(cx, |app, cx| {
                    app.set_kill_switch_enabled_pref(false, cx);
                });
                true
            })
    });
}

pub(super) fn dns_preset_item(app: Entity<WgApp>, language: Language) -> SettingItem {
    SettingItem::new(
        tr(language, "DNS Preset"),
        SettingField::render(move |_, _window, cx| render_dns_preset_field(app.clone(), cx)),
    )
    .description("Only used when DNS mode fills or overrides resolver records.")
}

#[cfg(target_os = "linux")]
pub(super) fn wireguard_backend_item(app: Entity<WgApp>) -> SettingItem {
    SettingItem::new(
        "WireGuard Implementation",
        SettingField::render(move |_, _window, cx| {
            let current = app.read(cx).ui_prefs.wireguard_backend_preference;
            let daita_mode = app.read(cx).ui_prefs.daita_mode;
            let set_handle = app.clone();
            let current_label = wireguard_backend_label(current);

            v_flex()
                .w_full()
                .gap_2()
                .child(
                    Button::new("advanced-wireguard-backend")
                        .label(current_label)
                        .outline()
                        .small()
                        .compact()
                        .dropdown_caret(true)
                        .dropdown_menu_with_anchor(
                            gpui::Corner::TopRight,
                            move |menu: PopupMenu, _, _| {
                                wireguard_backend_options().iter().fold(
                                    menu,
                                    |menu, (value, label)| {
                                        let checked = *value == wireguard_backend_value(current);
                                        menu.item(
                                            PopupMenuItem::new(label.clone())
                                                .checked(checked)
                                                .on_click({
                                                    let set_handle = set_handle.clone();
                                                    let value = value.clone();
                                                    move |_, _, cx| {
                                                        let next =
                                                            wireguard_backend_from_value(&value);
                                                        set_handle.update(cx, |app, cx| {
                                                            app.set_wireguard_backend_preference(
                                                                next, cx,
                                                            );
                                                        });
                                                    }
                                                }),
                                        )
                                    },
                                )
                            },
                        ),
                )
                .when(current == WireGuardBackendPreference::Kernel && daita_mode.is_enabled(), |this| {
                        this.child(
                            div()
                                .rounded_md()
                                .border_1()
                                .border_color(cx.theme().warning.alpha(0.42))
                                .bg(cx.theme().warning.alpha(0.08))
                                .px_3()
                                .py_2()
                                .text_sm()
                                .text_color(cx.theme().warning)
                                .child(
                                    "DAITA requires GotaTun. Switch WireGuard implementation to Userspace or Auto to use DAITA.",
                                ),
                        )
                    })
                .when(current == WireGuardBackendPreference::Auto && daita_mode.is_enabled(), |this| {
                        this.child(
                            div()
                                .rounded_md()
                                .border_1()
                                .border_color(cx.theme().border)
                                .bg(cx.theme().background)
                                .px_3()
                                .py_2()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(
                                    "Auto will use GotaTun while DAITA is enabled.",
                                ),
                        )
                    })
        }),
    )
    .layout(Axis::Vertical)
    .description(
        "Auto prefers Linux kernel WireGuard, falls back to GotaTun when kernel support is unavailable, and uses GotaTun while DAITA is enabled.",
    )
}

#[cfg(target_os = "linux")]
fn wireguard_backend_options() -> Vec<(SharedString, SharedString)> {
    vec![
        ("auto".into(), "Auto".into()),
        ("kernel".into(), "Kernel".into()),
        ("userspace".into(), "Userspace / GotaTun".into()),
    ]
}

#[cfg(target_os = "linux")]
fn wireguard_backend_value(value: WireGuardBackendPreference) -> SharedString {
    match value {
        WireGuardBackendPreference::Auto => "auto".into(),
        WireGuardBackendPreference::Kernel => "kernel".into(),
        WireGuardBackendPreference::Userspace => "userspace".into(),
    }
}

#[cfg(target_os = "linux")]
fn wireguard_backend_from_value(value: &SharedString) -> WireGuardBackendPreference {
    match value.as_ref() {
        "kernel" => WireGuardBackendPreference::Kernel,
        "userspace" => WireGuardBackendPreference::Userspace,
        _ => WireGuardBackendPreference::Auto,
    }
}

#[cfg(target_os = "linux")]
fn wireguard_backend_label(value: WireGuardBackendPreference) -> SharedString {
    wireguard_backend_options()
        .into_iter()
        .find(|(option, _)| *option == wireguard_backend_value(value))
        .map(|(_, label)| label)
        .unwrap_or_else(|| "Auto".into())
}

pub(super) fn quantum_mode_item(app: Entity<WgApp>) -> SettingItem {
    let get_handle = app.clone();
    let set_handle = app;

    SettingItem::new(
        "Quantum Tunnel Upgrade",
        SettingField::switch(
            move |cx| get_handle.read(cx).ui_prefs.quantum_mode == QuantumMode::On,
            move |value, cx| {
                let next = if value {
                    QuantumMode::On
                } else {
                    QuantumMode::Off
                };
                set_handle.update(cx, |app, cx| {
                    app.set_quantum_mode_pref(next, cx);
                });
            },
        ),
    )
    .description(
        "Currently supports only Mullvad single-hop WireGuard tunnels. Startup first establishes a base tunnel, then negotiates the quantum upgrade before reporting quantum protected.",
    )
}

pub(super) fn daita_mode_item(app: Entity<WgApp>) -> SettingItem {
    SettingItem::new(
        "DAITA",
        SettingField::render(move |_, _window, cx| {
            let enabled = app.read(cx).ui_prefs.daita_mode == DaitaMode::On;
            let backend_preference = app.read(cx).ui_prefs.wireguard_backend_preference;
            let enable_handle = app.clone();
            let direct_set_handle = app.clone();

            Switch::new("advanced-daita")
                .label("Enable DAITA for Mullvad tunnels")
                .checked(enabled)
                .with_size(Size::Small)
                .on_click(move |checked, window, cx| {
                    if !*checked {
                        direct_set_handle.update(cx, |app, cx| {
                            app.set_daita_mode_pref(DaitaMode::Off, cx);
                        });
                        return;
                    }

                    if backend_preference != WireGuardBackendPreference::Kernel {
                        direct_set_handle.update(cx, |app, cx| {
                            app.set_daita_mode_pref(DaitaMode::On, cx);
                        });
                        return;
                    }

                    open_daita_requires_userspace_dialog(enable_handle.clone(), window, cx);
                })
        }),
    )
    .description(
        "Negotiate Mullvad DAITA settings for each tunnel start. Requires a Mullvad single-hop peer that advertises DAITA capability.",
    )
}

fn open_daita_requires_userspace_dialog(
    app_handle: Entity<WgApp>,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    window.open_dialog(cx, move |dialog, _window, cx| {
        let confirm_handle = app_handle.clone();
        dialog
            .title(div().text_lg().child("Switch to GotaTun for DAITA?"))
            .confirm()
            .button_props(
                DialogButtonProps::default()
                    .ok_text("Switch to GotaTun")
                    .ok_variant(ButtonVariant::Primary)
                    .cancel_text("Cancel"),
            )
            .child(
                div().text_sm().child(
                    "DAITA currently requires the userspace GotaTun implementation. It is not available with the Linux kernel WireGuard backend.",
                ),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        "Choosing Switch to GotaTun changes WireGuard Implementation to Userspace / GotaTun and enables DAITA. Cancel leaves DAITA off.",
                    ),
            )
            .on_ok(move |_, _window, cx| {
                confirm_handle.update(cx, |app, cx| {
                    app.set_wireguard_backend_preference(WireGuardBackendPreference::Userspace, cx);
                    app.set_daita_mode_pref(DaitaMode::On, cx);
                });
                true
            })
    });
}

pub(super) fn daita_resources_item(app: Entity<WgApp>) -> SettingItem {
    SettingItem::new(
        "DAITA Resources",
        SettingField::render(move |_, _window, cx| {
            let diagnostic = app.read(cx).ui.daita_resources.clone();
            let action_handle = app.clone();
            let action_label = if diagnostic.has_cache() {
                "Refresh"
            } else {
                "Download"
            };
            let note = match diagnostic.health {
                DaitaResourcesHealth::Ready => {
                    "Used for strict DAITA startup validation. Start will fail if the selected peer is not present or not DAITA-capable in this cache."
                }
                DaitaResourcesHealth::Missing => {
                    "If direct access to Mullvad's API is blocked on this network, first connect a regular Mullvad WireGuard tunnel, then download the resources here."
                }
                _ => {
                    "This cache is stored by the backend and reused for later DAITA starts. No fallback to non-DAITA mode is performed."
                }
            };

            v_flex()
                .w_full()
                .gap_2()
                .child(
                    h_flex()
                        .items_center()
                        .gap_3()
                        .flex_wrap()
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("Status: {}", diagnostic.summary())),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format_checked_label(diagnostic.checked_at)),
                        )
                        .child(
                            Button::new("advanced-daita-resources-refresh")
                                .label(action_label)
                                .outline()
                                .small()
                                .compact()
                                .loading(diagnostic.is_busy())
                                .disabled(diagnostic.is_busy())
                                .on_click(move |_, _, cx| {
                                    action_handle.update(cx, |app, cx| {
                                        app.refresh_daita_resources_cache(cx);
                                    });
                                }),
                        ),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().foreground)
                        .child(diagnostic.detail.clone()),
                )
                .when_some(diagnostic.cache_path.clone(), |this, path| {
                    this.child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("Cache path: {path}")),
                    )
                })
                .when(diagnostic.has_cache(), |this| {
                    let fetched = diagnostic
                        .fetched_at
                        .map(format_timestamp)
                        .unwrap_or_else(|| "unknown".to_string());
                    this.child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(
                                "Cached at: {fetched} · Relays: {} · DAITA-capable: {}",
                                diagnostic.relay_count, diagnostic.daita_relay_count
                            )),
                    )
                })
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(note),
                )
        }),
    )
    .layout(Axis::Vertical)
    .description("Download or refresh the Mullvad relay inventory required for DAITA startup validation.")
}

pub(super) fn traffic_period_item(app: Entity<WgApp>, language: Language) -> SettingItem {
    SettingItem::new(
        tr(language, "Preferred Traffic Range"),
        SettingField::render(move |_, _window, cx| {
            let current = app.read(cx).ui_prefs.preferred_traffic_period;
            let language = app.read(cx).language();
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
                            .label(tr(language, "Today"))
                            .selected(current == TrafficPeriod::Today)
                            .on_click(move |_, _, cx| {
                                today_handle.update(cx, |app, cx| {
                                    app.set_preferred_traffic_period(TrafficPeriod::Today, cx);
                                });
                            }),
                    )
                    .child(
                        Button::new("advanced-traffic-this-month")
                            .label(tr(language, "This Month"))
                            .selected(current == TrafficPeriod::ThisMonth)
                            .on_click(move |_, _, cx| {
                                month_handle.update(cx, |app, cx| {
                                    app.set_preferred_traffic_period(TrafficPeriod::ThisMonth, cx);
                                });
                            }),
                    )
                    .child(
                        Button::new("advanced-traffic-last-month")
                            .label(tr(language, "Last Month"))
                            .selected(current == TrafficPeriod::LastMonth)
                            .on_click(move |_, _, cx| {
                                last_month_handle.update(cx, |app, cx| {
                                    app.set_preferred_traffic_period(TrafficPeriod::LastMonth, cx);
                                });
                            }),
                    ),
            )
        }),
    )
    .description("Applies now and stays remembered for future sessions.")
}

pub(super) fn inspector_tab_item(app: Entity<WgApp>, language: Language) -> SettingItem {
    SettingItem::new(
        tr(language, "Inspector View"),
        SettingField::render(move |_, _window, cx| {
            let current = app.read(cx).ui_prefs.preferred_inspector_tab;
            let language = app.read(cx).language();
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
                            .label(tr(language, "Preview"))
                            .selected(current == ConfigInspectorTab::Preview)
                            .on_click(move |_, _, cx| {
                                preview_handle.update(cx, |app, cx| {
                                    app.set_preferred_inspector_tab(
                                        ConfigInspectorTab::Preview,
                                        cx,
                                    );
                                });
                            }),
                    )
                    .child(
                        Button::new("advanced-inspector-diagnostics")
                            .label(tr(language, "Diagnostics"))
                            .selected(current == ConfigInspectorTab::Diagnostics)
                            .on_click(move |_, _, cx| {
                                diagnostics_handle.update(cx, |app, cx| {
                                    app.set_preferred_inspector_tab(
                                        ConfigInspectorTab::Diagnostics,
                                        cx,
                                    );
                                });
                            }),
                    )
                    .child(
                        Button::new("advanced-inspector-activity")
                            .label(tr(language, "Activity"))
                            .selected(current == ConfigInspectorTab::Activity)
                            .on_click(move |_, _, cx| {
                                activity_handle.update(cx, |app, cx| {
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

fn format_checked_label(checked_at: Option<SystemTime>) -> String {
    match checked_at {
        Some(checked_at) => format!("Checked {}", format_age(checked_at)),
        None => "Not checked yet".to_string(),
    }
}

fn format_timestamp(time: SystemTime) -> String {
    let absolute = DateTime::<Local>::from(time)
        .format("%Y-%m-%d %H:%M:%S local")
        .to_string();
    format!("{absolute} ({})", format_age(time))
}

fn format_age(time: SystemTime) -> String {
    let elapsed = SystemTime::now()
        .duration_since(time)
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
