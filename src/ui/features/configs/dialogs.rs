use gpui::{div, Context, IntoElement, ParentElement, SharedString, Styled, Window};
use gpui_component::{
    button::{Button, ButtonVariant, ButtonVariants as _},
    dialog::DialogButtonProps,
    ActiveTheme as _, WindowExt,
};

use crate::ui::state::{ConfigsPrimaryPane, PendingDraftAction, SidebarItem, WgApp};

use super::draft;
use super::import_export::{handle_import_click, handle_paste_click};
use super::storage::{delete_configs_blocking_running, load_config_into_inputs, save_draft};

pub(crate) fn run_pending_draft_action(
    app: &mut WgApp,
    action: PendingDraftAction,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) {
    match action {
        PendingDraftAction::SelectConfig(config_id) => {
            if app.selection.selected_id == Some(config_id) {
                return;
            }
            app.set_selected_config_id(Some(config_id), cx);
            load_config_into_inputs(app, config_id, window, cx);
            app.persist_state_async(cx);
            app.set_status("Loaded tunnel");
            cx.notify();
        }
        PendingDraftAction::ActivateSidebar(item) => {
            app.set_sidebar_active(item, cx);
            app.close_sidebar_overlay(cx);
        }
        PendingDraftAction::NewDraft => {
            app.set_selected_config_id(None, cx);
            app.clear_inputs(window, cx);
            let workspace = app.ensure_configs_workspace(cx);
            workspace.update(cx, |workspace, cx| {
                if workspace.set_primary_pane(ConfigsPrimaryPane::Editor) {
                    cx.notify();
                }
            });
            app.set_status("New draft");
            cx.notify();
        }
        PendingDraftAction::Import => handle_import_click(app, window, cx),
        PendingDraftAction::Paste => handle_paste_click(app, window, cx),
        PendingDraftAction::DeleteCurrent => open_delete_current_config_dialog(app, window, cx),
        PendingDraftAction::RestartTunnel => {
            app.runtime.queue_pending_start(
                app.selection
                    .build_pending_start(&app.configs, &app.runtime),
            );
            app.handle_start_stop_core(cx);
        }
    }
}

pub(crate) fn confirm_discard_or_save(
    app: &mut WgApp,
    action: PendingDraftAction,
    window: &mut Window,
    cx: &mut Context<WgApp>,
    title: impl Into<SharedString>,
    body: impl Into<SharedString>,
) {
    if !app.configs_draft_snapshot(cx).is_dirty() {
        run_pending_draft_action(app, action, window, cx);
        return;
    }

    let app_handle = cx.entity();
    let title = title.into();
    let body = body.into();

    window.open_dialog(cx, move |dialog, _window, dlg_cx| {
        let app_handle_save = app_handle.clone();
        let app_handle_discard = app_handle.clone();
        let app_handle_ok = app_handle.clone();
        dialog
            .title(div().text_lg().child(title.clone()))
            .confirm()
            .button_props(
                DialogButtonProps::default()
                    .ok_text("Save")
                    .ok_variant(ButtonVariant::Primary)
                    .cancel_text("Cancel"),
            )
            .child(div().text_sm().child(body.clone()))
            .child(
                div()
                    .text_xs()
                    .text_color(dlg_cx.theme().muted_foreground)
                    .child("Save your edits, discard them, or cancel this action."),
            )
            .footer(move |_ok, _cancel, _window, _cx| {
                let save_handle = app_handle_save.clone();
                let discard_handle = app_handle_discard.clone();
                let save_button = Button::new("draft-dialog-save").label("Save").on_click(
                    move |_, window, cx| {
                        save_handle.update(cx, |app, cx| {
                            app.set_configs_pending_action(Some(action), cx);
                            save_draft(app, false, window, cx);
                        });
                        window.close_dialog(cx);
                    },
                );
                let discard_button = Button::new("draft-dialog-discard")
                    .label("Discard")
                    .danger()
                    .on_click(move |_, window, cx| {
                        window.on_next_frame({
                            let discard_handle = discard_handle.clone();
                            move |window, cx| {
                                discard_handle.update(cx, |app, cx| {
                                    app.set_configs_pending_action(None, cx);
                                    draft::discard_current_draft(app, window, cx);
                                    run_pending_draft_action(app, action, window, cx);
                                });
                            }
                        });
                        window.close_dialog(cx);
                    });
                let cancel_button = Button::new("draft-dialog-cancel")
                    .label("Cancel")
                    .outline()
                    .on_click(|_, window, cx| {
                        window.close_dialog(cx);
                    });
                vec![
                    cancel_button.into_any_element(),
                    discard_button.into_any_element(),
                    save_button.into_any_element(),
                ]
            })
            .on_ok(move |_, window, cx| {
                app_handle_ok.update(cx, |app, cx| {
                    app.set_configs_pending_action(Some(action), cx);
                    save_draft(app, false, window, cx);
                });
                true
            })
    });
}

pub(crate) fn request_sidebar_active(
    app: &mut WgApp,
    item: SidebarItem,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) {
    if app.ui_session.sidebar_active == item {
        return;
    }
    confirm_discard_or_save(
        app,
        PendingDraftAction::ActivateSidebar(item),
        window,
        cx,
        "Leave Configs?",
        "You have unsaved changes in the current config draft.",
    );
}

pub(crate) fn handle_new_draft_click(
    app: &mut WgApp,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) {
    confirm_discard_or_save(
        app,
        PendingDraftAction::NewDraft,
        window,
        cx,
        "Create new draft?",
        "Creating a new draft will replace the current unsaved draft.",
    );
}

pub(crate) fn open_delete_current_config_dialog(
    app: &mut WgApp,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) {
    let Some(config_id) = app.selection.selected_id else {
        app.set_error("Select a tunnel first");
        cx.notify();
        return;
    };
    let config_name = app
        .configs
        .get_by_id(config_id)
        .map(|cfg| cfg.name.clone())
        .unwrap_or_else(|| "this config".to_string());
    let app_handle = cx.entity();

    window.open_dialog(cx, move |dialog, _window, _cx| {
        let delete_handle = app_handle.clone();
        dialog
            .title(div().text_lg().child("Delete config?"))
            .confirm()
            .button_props(
                DialogButtonProps::default()
                    .ok_text("Delete")
                    .ok_variant(ButtonVariant::Danger)
                    .cancel_text("Cancel"),
            )
            .child(
                div()
                    .text_sm()
                    .child(format!("Delete \"{config_name}\"? This cannot be undone.")),
            )
            .on_ok(move |_, window, cx| {
                delete_handle.update(cx, |app, cx| {
                    handle_confirmed_delete_current(app, window, cx);
                });
                true
            })
    });
}

fn handle_confirmed_delete_current(app: &mut WgApp, window: &mut Window, cx: &mut Context<WgApp>) {
    let Some(config_id) = app.selection.selected_id else {
        app.set_error("Select a tunnel first");
        cx.notify();
        return;
    };
    delete_configs_blocking_running(app, &[config_id], window, cx);
}
