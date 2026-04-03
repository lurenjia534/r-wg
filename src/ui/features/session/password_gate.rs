use std::time::Duration;

use gpui::{div, AppContext, Context, ParentElement, Styled, Window};
use gpui_component::{
    button::ButtonVariant,
    dialog::DialogButtonProps,
    input::{Input, InputState},
    ActiveTheme as _, Sizable as _, WindowExt,
};
use subtle::ConstantTimeEq;
use zeroize::Zeroize;

use crate::ui::state::{PendingStart, WgApp};

use super::controller;

const CONNECT_PASSWORD_CREDENTIAL_URL: &str = "r-wg://connect-password";
const CONNECT_PASSWORD_CREDENTIAL_USERNAME: &str = "connect-password";
const CONNECT_PASSWORD_MIN_CHARS: usize = 8;

#[derive(Clone, Copy)]
pub(crate) enum ConnectPasswordAction {
    StartSelected {
        config_id: u64,
        restart_delay: Option<Duration>,
    },
    QueuePendingStart {
        config_id: u64,
    },
    RestartAfterStop {
        config_id: u64,
    },
}

pub(crate) fn connect_password_window_required_message() -> &'static str {
    "Connection password is required. Open the main window to connect."
}

pub(crate) fn connect_password_missing_message() -> &'static str {
    "Connection password protection is enabled, but no password is configured. Open Preferences to set one."
}

pub(crate) fn toggle_connect_password_requirement(
    app: &mut WgApp,
    enabled: bool,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) {
    if !enabled {
        app.set_connect_password_required_pref(false, cx);
        app.set_status("Connection password requirement disabled");
        return;
    }

    app.set_status("Checking connection password...");
    cx.notify();

    let view = cx.weak_entity();
    let read_task = cx.read_credentials(CONNECT_PASSWORD_CREDENTIAL_URL);
    window
        .spawn(cx, async move |cx| {
            let result = read_task.await;
            view.update_in(cx, |this, window, cx| {
                match result {
                    Ok(Some(_)) => {
                        this.set_connect_password_required_pref(true, cx);
                        this.set_status("Connection password requirement enabled");
                    }
                    Ok(None) => {
                        open_connect_password_editor_dialog(this, true, None, window, cx);
                    }
                    Err(err) => {
                        this.set_error(format!("System credential store unavailable: {err}"));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
}

pub(crate) fn open_connect_password_editor(
    app: &mut WgApp,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) {
    app.set_status("Loading connection password settings...");
    cx.notify();

    let view = cx.weak_entity();
    let read_task = cx.read_credentials(CONNECT_PASSWORD_CREDENTIAL_URL);
    window
        .spawn(cx, async move |cx| {
            let result = read_task.await;
            view.update_in(cx, |this, window, cx| match result {
                Ok(Some((_, password))) => {
                    open_connect_password_editor_dialog(this, false, Some(password), window, cx);
                }
                Ok(None) => {
                    open_connect_password_editor_dialog(this, false, None, window, cx);
                }
                Err(err) => {
                    this.set_error(format!("System credential store unavailable: {err}"));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
}

pub(crate) fn remove_connect_password(
    _app: &mut WgApp,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) {
    let view = cx.weak_entity();
    window.open_dialog(cx, move |dialog, _window, _cx| {
        let view = view.clone();
        dialog
            .title(div().text_lg().child("Remove connection password?"))
            .confirm()
            .button_props(
                DialogButtonProps::default()
                    .ok_text("Remove")
                    .ok_variant(ButtonVariant::Danger)
                    .cancel_text("Cancel"),
            )
            .child(div().text_sm().child(
                "Remove the saved connection password and turn off the startup requirement.",
            ))
            .on_ok(move |_, window, cx| {
                let view = view.clone();
                let delete_task = cx.delete_credentials(CONNECT_PASSWORD_CREDENTIAL_URL);
                view.update(cx, |app, _| {
                    app.set_status("Removing connection password...");
                })
                .ok();
                window
                    .spawn(cx, async move |cx| {
                        let result = delete_task.await;
                        view.update_in(cx, |this, window, cx| {
                            match result {
                                Ok(()) => {
                                    this.set_connect_password_required_pref(false, cx);
                                    this.set_status("Connection password removed");
                                    this.push_success_toast(
                                        "Connection password removed",
                                        window,
                                        cx,
                                    );
                                }
                                Err(err) => {
                                    this.set_error(format!(
                                        "Remove connection password failed: {err}"
                                    ));
                                }
                            }
                            cx.notify();
                        })
                        .ok();
                    })
                    .detach();
                true
            })
    });
}

pub(crate) fn request_connect_password_action(
    app: &mut WgApp,
    action: ConnectPasswordAction,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) {
    app.set_status("Checking connection password...");
    cx.notify();

    let view = cx.weak_entity();
    let read_task = cx.read_credentials(CONNECT_PASSWORD_CREDENTIAL_URL);
    window
        .spawn(cx, async move |cx| {
            let result = read_task.await;
            view.update_in(cx, |this, window, cx| match result {
                Ok(Some((_, password))) => {
                    open_connect_password_prompt_dialog(action, password, window, cx);
                }
                Ok(None) => {
                    this.set_error(connect_password_missing_message());
                    cx.notify();
                }
                Err(err) => {
                    this.set_error(format!("System credential store unavailable: {err}"));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
}

fn open_connect_password_editor_dialog(
    app: &mut WgApp,
    enable_after_save: bool,
    existing_password: Option<Vec<u8>>,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) {
    let current_password_input = existing_password.as_ref().map(|_| {
        cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder("Current password")
        })
    });
    let new_password_input = cx.new(|cx| {
        InputState::new(window, cx)
            .masked(true)
            .placeholder("At least 8 characters")
    });
    let confirm_password_input = cx.new(|cx| {
        InputState::new(window, cx)
            .masked(true)
            .placeholder("Repeat the new password")
    });

    let app_handle = cx.entity();
    let title = if existing_password.is_some() {
        "Change connection password"
    } else {
        "Set connection password"
    };
    let ok_text = if existing_password.is_some() {
        "Update"
    } else {
        "Save"
    };

    window.open_dialog(cx, move |dialog, _window, dlg_cx| {
        let current_password_input = current_password_input.clone();
        let new_password_input = new_password_input.clone();
        let confirm_password_input = confirm_password_input.clone();
        let app_handle = app_handle.clone();
        let existing_password = existing_password.clone();

        let mut dialog = dialog
            .title(div().text_lg().child(title))
            .confirm()
            .button_props(
                DialogButtonProps::default()
                    .ok_text(ok_text)
                    .ok_variant(ButtonVariant::Primary)
                    .cancel_text("Cancel"),
            )
            .child(
                div()
                    .text_sm()
                    .child("Store the password in your system credential manager. It will be required before new tunnel starts."),
            );

        if let Some(input) = current_password_input.clone() {
            dialog = dialog.child(password_field("Current password", &input, dlg_cx));
        }

        dialog
            .child(password_field("New password", &new_password_input, dlg_cx))
            .child(password_field(
                "Confirm password",
                &confirm_password_input,
                dlg_cx,
            ))
            .on_ok(move |_, window, cx| {
                let mut current_password = current_password_input.as_ref().map(|input| {
                    input.read(cx).value().to_string().into_bytes()
                });
                let mut new_password = new_password_input.read(cx).value().to_string().into_bytes();
                let mut confirm_password =
                    confirm_password_input.read(cx).value().to_string().into_bytes();

                if new_password.is_empty() || confirm_password.is_empty() {
                    app_handle.update(cx, |app, cx| {
                        app.set_error("Enter the new password twice");
                        cx.notify();
                    });
                    clear_password_input(&new_password_input, window, cx);
                    clear_password_input(&confirm_password_input, window, cx);
                    wipe_bytes(&mut new_password);
                    wipe_bytes(&mut confirm_password);
                    if let Some(current_password) = current_password.as_mut() {
                        wipe_bytes(current_password);
                    }
                    return false;
                }

                if count_chars(&new_password) < CONNECT_PASSWORD_MIN_CHARS {
                    app_handle.update(cx, |app, cx| {
                        app.set_error("Connection password must be at least 8 characters");
                        cx.notify();
                    });
                    clear_password_input(&new_password_input, window, cx);
                    clear_password_input(&confirm_password_input, window, cx);
                    wipe_bytes(&mut new_password);
                    wipe_bytes(&mut confirm_password);
                    if let Some(current_password) = current_password.as_mut() {
                        wipe_bytes(current_password);
                    }
                    return false;
                }

                if !constant_time_eq(&new_password, &confirm_password) {
                    app_handle.update(cx, |app, cx| {
                        app.set_error("Passwords do not match");
                        cx.notify();
                    });
                    clear_password_input(&new_password_input, window, cx);
                    clear_password_input(&confirm_password_input, window, cx);
                    wipe_bytes(&mut new_password);
                    wipe_bytes(&mut confirm_password);
                    if let Some(current_password) = current_password.as_mut() {
                        wipe_bytes(current_password);
                    }
                    return false;
                }

                if let Some(existing_password) = existing_password.as_ref() {
                    let is_valid = current_password
                        .as_ref()
                        .map(|value| constant_time_eq(value, existing_password))
                        .unwrap_or(false);
                    if !is_valid {
                        app_handle.update(cx, |app, cx| {
                            app.set_error("Current password is incorrect");
                            cx.notify();
                        });
                        if let Some(input) = current_password_input.as_ref() {
                            clear_password_input(input, window, cx);
                        }
                        wipe_bytes(&mut new_password);
                        wipe_bytes(&mut confirm_password);
                        if let Some(current_password) = current_password.as_mut() {
                            wipe_bytes(current_password);
                        }
                        return false;
                    }
                }

                let write_task = cx.write_credentials(
                    CONNECT_PASSWORD_CREDENTIAL_URL,
                    CONNECT_PASSWORD_CREDENTIAL_USERNAME,
                    &new_password,
                );
                wipe_bytes(&mut new_password);
                wipe_bytes(&mut confirm_password);
                if let Some(current_password) = current_password.as_mut() {
                    wipe_bytes(current_password);
                }

                let app_handle = app_handle.clone();
                window
                    .spawn(cx, async move |cx| {
                        let result = write_task.await;
                        app_handle
                            .update_in(cx, |app, window, cx| {
                                match result {
                                    Ok(()) => {
                                        if enable_after_save {
                                            app.set_connect_password_required_pref(true, cx);
                                        }
                                        app.set_status("Connection password saved");
                                        app.push_success_toast(
                                            "Connection password saved",
                                            window,
                                            cx,
                                        );
                                    }
                                    Err(err) => {
                                        app.set_error(format!(
                                            "Save connection password failed: {err}"
                                        ));
                                    }
                                }
                                cx.notify();
                            })
                            .ok();
                    })
                    .detach();
                true
            })
    });

    app.set_status("Ready");
    cx.notify();
}

fn open_connect_password_prompt_dialog(
    action: ConnectPasswordAction,
    stored_password: Vec<u8>,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) {
    let password_input = cx.new(|cx| {
        InputState::new(window, cx)
            .masked(true)
            .placeholder("Enter connection password")
    });
    let app_handle = cx.entity();

    window.open_dialog(cx, move |dialog, _window, dlg_cx| {
        let password_input = password_input.clone();
        let app_handle = app_handle.clone();
        let stored_password = stored_password.clone();

        dialog
            .title(div().text_lg().child("Connection password"))
            .confirm()
            .button_props(
                DialogButtonProps::default()
                    .ok_text("Unlock")
                    .ok_variant(ButtonVariant::Primary)
                    .cancel_text("Cancel"),
            )
            .child(
                div()
                    .text_sm()
                    .child("Enter the connection password before starting a WireGuard tunnel."),
            )
            .child(password_field("Password", &password_input, dlg_cx))
            .on_ok(move |_, window, cx| {
                let mut entered_password = password_input.read(cx).value().to_string().into_bytes();
                let is_valid = constant_time_eq(&entered_password, &stored_password);
                clear_password_input(&password_input, window, cx);
                wipe_bytes(&mut entered_password);

                if !is_valid {
                    app_handle.update(cx, |app, cx| {
                        app.set_error("Connection password is incorrect");
                        cx.notify();
                    });
                    return false;
                }

                app_handle.update(cx, |app, cx| match action {
                    ConnectPasswordAction::StartSelected {
                        config_id,
                        restart_delay,
                    } => {
                        controller::start_config_by_id(
                            app,
                            config_id,
                            restart_delay,
                            cx,
                            "Select a tunnel first",
                            true,
                        );
                    }
                    ConnectPasswordAction::QueuePendingStart { config_id } => {
                        if app.runtime.queue_pending_start(Some(PendingStart {
                            config_id,
                            password_authorized: true,
                        })) {
                            app.set_status("Stopping... (queued start)");
                            cx.notify();
                        }
                    }
                    ConnectPasswordAction::RestartAfterStop { config_id } => {
                        app.runtime.queue_pending_start(Some(PendingStart {
                            config_id,
                            password_authorized: true,
                        }));
                        controller::handle_start_stop(app, window, cx);
                    }
                });
                true
            })
    });
}

fn password_field(
    label: &'static str,
    input: &gpui::Entity<InputState>,
    cx: &mut gpui::App,
) -> impl gpui::IntoElement {
    div()
        .pt_2()
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(Input::new(input).mask_toggle().small().w_full())
}

fn clear_password_input(input: &gpui::Entity<InputState>, window: &mut Window, cx: &mut gpui::App) {
    input.update(cx, |input, cx| {
        input.set_value("", window, cx);
    });
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    bool::from(left.ct_eq(right))
}

fn count_chars(bytes: &[u8]) -> usize {
    // Passwords originate from the text input widget and are expected to be valid UTF-8.
    // Treat invalid bytes as an invalid password by returning 0.
    std::str::from_utf8(bytes)
        .map(|text| text.chars().count())
        .unwrap_or(0)
}

fn wipe_bytes(bytes: &mut Vec<u8>) {
    bytes.zeroize();
    bytes.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_eq_accepts_equal_values() {
        assert!(constant_time_eq(
            b"correct horse battery staple",
            b"correct horse battery staple"
        ));
    }

    #[test]
    fn constant_time_eq_rejects_different_lengths_and_values() {
        assert!(!constant_time_eq(b"secret", b"secret2"));
        assert!(!constant_time_eq(b"secret", b"secreu"));
        assert!(!constant_time_eq(b"", b"non-empty"));
    }

    #[test]
    fn count_chars_uses_unicode_scalar_count() {
        assert_eq!(count_chars("密码abc".as_bytes()), 5);
        assert_eq!(count_chars("🙂🙂".as_bytes()), 2);
    }

    #[test]
    fn count_chars_returns_zero_for_invalid_utf8() {
        assert_eq!(count_chars(&[0xff, 0xfe, 0xfd]), 0);
    }

    #[test]
    fn wipe_bytes_clears_buffer() {
        let mut bytes = b"top-secret".to_vec();
        wipe_bytes(&mut bytes);
        assert!(bytes.is_empty());
    }
}
