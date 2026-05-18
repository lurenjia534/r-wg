use std::time::{Duration, SystemTime};

use chrono::{DateTime, Local};
use gpui::prelude::FluentBuilder as _;
use gpui::{div, Axis, Entity, ParentElement, SharedString, Styled, Window};
use gpui_component::button::{Button, ButtonVariant};
use gpui_component::dialog::DialogButtonProps;
use gpui_component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_component::setting::{SettingField, SettingItem};
use gpui_component::switch::Switch;
use gpui_component::{
    h_flex, v_flex, ActiveTheme as _, Disableable as _, Sizable, Size, WindowExt,
};
use r_wg::backend::wg::{DaitaMode, QuantumMode, WireGuardBackendPreference};

use crate::ui::state::{DaitaResourcesHealth, WgApp};

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
                            gpui::Anchor::TopRight,
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
                .when(
                    current == WireGuardBackendPreference::Kernel && daita_mode.is_enabled(),
                    |this| {
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
                    },
                )
                .when(
                    current == WireGuardBackendPreference::Auto && daita_mode.is_enabled(),
                    |this| {
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
                                .child("Auto will use GotaTun while DAITA is enabled."),
                        )
                    },
                )
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
            .button_props(
                DialogButtonProps::default()
                    .ok_text("Switch to GotaTun")
                    .ok_variant(ButtonVariant::Primary)
                    .show_cancel(true)
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
