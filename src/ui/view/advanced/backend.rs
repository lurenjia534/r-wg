use gpui::prelude::FluentBuilder as _;
use std::time::Duration;

use gpui::{div, Axis, Div, Entity, ParentElement, SharedString, Styled, Window};
use gpui_component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_component::description_list::DescriptionList;
use gpui_component::dialog::DialogButtonProps;
use gpui_component::setting::{SettingField, SettingItem};
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Disableable as _, Sizable as _, WindowExt};
use r_wg::backend::wg::PrivilegedServiceAction;

use crate::ui::i18n::{tr, Language};
use crate::ui::state::{BackendHealth, SidebarItem, WgApp};
use crate::ui::view::widgets::backend_status_tag;

use super::backend_diagnostics::{
    active_wireguard_backend_label, backend_checked_label, backend_recommended_action,
    backend_recovery_note, build_backend_diagnostics_text, format_checked_timestamp,
    helper_control_endpoint, helper_platform_detail, should_show_remove_action,
    should_show_repair_action,
};

pub(super) fn privileged_backend_item(app: Entity<WgApp>, language: Language) -> SettingItem {
    SettingItem::new(
        tr(language, "Service Status"),
        SettingField::render(move |_, window, cx| {
            render_privileged_backend_panel(app.clone(), window, cx)
        }),
    )
    .layout(Axis::Vertical)
}

pub(super) fn troubleshooting_item(app: Entity<WgApp>) -> SettingItem {
    SettingItem::new(
        "Troubleshooting",
        SettingField::render(move |_, _, cx| {
            #[cfg(target_os = "linux")]
            let (running, repair_busy) = {
                let app = app.read(cx);
                (
                    app.runtime.running,
                    app.ui
                        .backend
                        .is_working_action(PrivilegedServiceAction::StartupRepair),
                )
            };
            #[cfg(target_os = "linux")]
            let repair_handle = app.clone();

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
                .when(cfg!(target_os = "linux"), |this| {
                    #[cfg(target_os = "linux")]
                    {
                        this.child(
                            h_flex()
                                .items_center()
                                .gap_2()
                                .pt_1()
                                .child(
                                    Button::new("backend-startup-repair")
                                        .label("Run Startup Repair")
                                        .outline()
                                        .small()
                                        .compact()
                                        .loading(repair_busy)
                                        .disabled(running || repair_busy)
                                        .on_click(move |_, _, cx| {
                                            repair_handle.update(cx, |app, cx| {
                                                app.run_privileged_backend_action(
                                                    PrivilegedServiceAction::StartupRepair,
                                                    cx,
                                                );
                                            });
                                        }),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(if running {
                                            "Stop the current tunnel before repairing stale startup state."
                                        } else {
                                            "Clears stale routes, DNS state, kill switch state, and journaled kernel WireGuard links."
                                        }),
                                ),
                        )
                    }
                    #[cfg(not(target_os = "linux"))]
                    {
                        this
                    }
                })
        }),
    )
    .layout(Axis::Vertical)
    .description("What Refresh, Repair, and Remove do.")
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
                            v_flex().gap_1().child(
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
                        div().child(
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
                                        .on_click(move |_, window, cx| {
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
            .button_props(
                DialogButtonProps::default()
                    .ok_text("Remove")
                    .ok_variant(ButtonVariant::Danger)
                    .show_cancel(true)
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
            cx.background_executor()
                .timer(Duration::from_secs(10))
                .await;

            let keep_running = app.update(cx, |app, cx| {
                if app.ui_session.sidebar_active == SidebarItem::Advanced {
                    cx.notify();
                    true
                } else {
                    false
                }
            });

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
    let wireguard_backend = active_wireguard_backend_label(app);

    let details = DescriptionList::new()
        .columns(1)
        .item("Active WireGuard Backend", wireguard_backend, 1)
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
