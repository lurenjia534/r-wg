use gpui::prelude::FluentBuilder as _;
use gpui::{div, Axis, Entity, ParentElement, Styled, Window};
use gpui_component::button::{Button, ButtonGroup, ButtonVariant, ButtonVariants as _};
use gpui_component::dialog::DialogButtonProps;
use gpui_component::setting::{SettingField, SettingItem};
use gpui_component::switch::Switch;
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Selectable, Sizable, Size, WindowExt};

use crate::ui::features::session::password_gate;
use crate::ui::i18n::{tr, Language, LanguagePreference};
use crate::ui::state::{ConfigInspectorTab, TrafficPeriod, WgApp};

use super::dns::{dns_mode_from_value, dns_mode_options, dns_mode_value, render_dns_preset_field};

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
            .button_props(
                DialogButtonProps::default()
                    .ok_text("Turn Off")
                    .ok_variant(ButtonVariant::Danger)
                    .show_cancel(true)
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
