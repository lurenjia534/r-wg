use gpui::{div, Axis, Entity, ParentElement, Styled};
use gpui_component::button::{Button, ButtonGroup, ButtonVariants as _};
use gpui_component::setting::{SettingField, SettingItem};
use gpui_component::switch::Switch;
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Selectable, Sizable, Size};
use r_wg::backend::wg::QuantumMode;

use crate::ui::features::session::password_gate;
use crate::ui::state::{ConfigInspectorTab, TrafficPeriod, WgApp};

use super::system::{
    dns_mode_from_value, dns_mode_options, dns_mode_value, render_dns_preset_field,
};

// Log, DNS mode, traffic range, and inspector default controls.

pub(super) fn log_auto_follow_item(app: Entity<WgApp>) -> SettingItem {
    let get_handle = app.clone();
    let set_handle = app;

    SettingItem::new(
        "Auto Follow Logs",
        SettingField::switch(
            move |cx| get_handle.read(cx).ui_prefs.log_auto_follow,
            move |value, cx| {
                set_handle.update(cx, |app, cx| {
                    app.set_log_auto_follow_pref(value, cx);
                });
            },
        ),
    )
    .description("Keep the log pane pinned to the latest runtime events.")
}

pub(super) fn connect_password_item(app: Entity<WgApp>) -> SettingItem {
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

pub(super) fn dns_mode_item(app: Entity<WgApp>) -> SettingItem {
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
                set_handle.update(cx, |app, cx| {
                    app.set_dns_mode_pref(next, cx);
                });
            },
        ),
    )
    .description("Choose whether config DNS, system DNS, or presets take precedence.")
}

pub(super) fn dns_preset_item(app: Entity<WgApp>) -> SettingItem {
    SettingItem::new(
        "DNS Preset",
        SettingField::render(move |_, _window, cx| render_dns_preset_field(app.clone(), cx)),
    )
    .description("Only used when DNS mode fills or overrides resolver records.")
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
        "Currently supports only Mullvad single-hop WireGuard tunnels. Enabling this for other providers will fail.",
    )
}

pub(super) fn traffic_period_item(app: Entity<WgApp>) -> SettingItem {
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
                                today_handle.update(cx, |app, cx| {
                                    app.set_preferred_traffic_period(TrafficPeriod::Today, cx);
                                });
                            }),
                    )
                    .child(
                        Button::new("advanced-traffic-this-month")
                            .label("This Month")
                            .selected(current == TrafficPeriod::ThisMonth)
                            .on_click(move |_, _, cx| {
                                month_handle.update(cx, |app, cx| {
                                    app.set_preferred_traffic_period(TrafficPeriod::ThisMonth, cx);
                                });
                            }),
                    )
                    .child(
                        Button::new("advanced-traffic-last-month")
                            .label("Last Month")
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

pub(super) fn inspector_tab_item(app: Entity<WgApp>) -> SettingItem {
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
                            .label("Diagnostics")
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
                            .label("Activity")
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
