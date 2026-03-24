use super::*;

impl WgApp {
    pub(super) fn run_pending_draft_action(
        &mut self,
        action: PendingDraftAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            PendingDraftAction::SelectConfig(config_id) => {
                if self.selection.selected_id == Some(config_id) {
                    return;
                }
                self.set_selected_config_id(Some(config_id), cx);
                self.load_config_into_inputs(config_id, window, cx);
                self.persist_state_async(cx);
                self.set_status("Loaded tunnel");
                cx.notify();
            }
            PendingDraftAction::ActivateSidebar(item) => {
                self.set_sidebar_active(item, cx);
                self.close_sidebar_overlay(cx);
            }
            PendingDraftAction::NewDraft => {
                self.set_selected_config_id(None, cx);
                self.clear_inputs(window, cx);
                let workspace = self.ensure_configs_workspace(cx);
                workspace.update(cx, |workspace, cx| {
                    if workspace.set_primary_pane(ConfigsPrimaryPane::Editor) {
                        cx.notify();
                    }
                });
                self.set_status("New draft");
                cx.notify();
            }
            PendingDraftAction::Import => {
                self.handle_import_click(window, cx);
            }
            PendingDraftAction::Paste => {
                self.handle_paste_click(window, cx);
            }
            PendingDraftAction::DeleteCurrent => {
                self.open_delete_current_config_dialog(window, cx);
            }
            PendingDraftAction::RestartTunnel => {
                self.runtime.queue_pending_start(
                    self.selection
                        .build_pending_start(&self.configs, &self.runtime),
                );
                self.handle_start_stop_core(cx);
            }
        }
    }

    pub(crate) fn confirm_discard_or_save(
        &mut self,
        action: PendingDraftAction,
        window: &mut Window,
        cx: &mut Context<Self>,
        title: impl Into<SharedString>,
        body: impl Into<SharedString>,
    ) {
        if !self.configs_draft_snapshot(cx).is_dirty() {
            self.run_pending_draft_action(action, window, cx);
            return;
        }

        let app_handle = cx.entity();
        let title = title.into();
        let body = body.into();

        window.open_dialog(cx, move |dialog, _window, _cx| {
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
                        .text_color(_cx.theme().muted_foreground)
                        .child("Save your edits, discard them, or cancel this action."),
                )
                .footer(move |_ok, _cancel, _window, _cx| {
                    let save_handle = app_handle_save.clone();
                    let discard_handle = app_handle_discard.clone();
                    let save_button = Button::new("draft-dialog-save").label("Save").on_click(
                        move |_, window, cx| {
                            save_handle.update(cx, |app, cx| {
                                app.set_configs_pending_action(Some(action), cx);
                                app.save_draft(false, window, cx);
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
                                        app.discard_current_draft(window, cx);
                                        app.run_pending_draft_action(action, window, cx);
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
                        app.save_draft(false, window, cx);
                    });
                    true
                })
        });
    }

    pub(crate) fn request_sidebar_active(
        &mut self,
        item: SidebarItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.ui_session.sidebar_active == item {
            return;
        }
        self.confirm_discard_or_save(
            PendingDraftAction::ActivateSidebar(item),
            window,
            cx,
            "Leave Configs?",
            "You have unsaved changes in the current config draft.",
        );
    }

    pub(crate) fn handle_new_draft_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_discard_or_save(
            PendingDraftAction::NewDraft,
            window,
            cx,
            "Create new draft?",
            "Creating a new draft will replace the current unsaved draft.",
        );
    }

    pub(super) fn open_delete_current_config_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(config_id) = self.selection.selected_id else {
            self.set_error("Select a tunnel first");
            cx.notify();
            return;
        };
        let config_name = self
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
                        app.handle_confirmed_delete_current(window, cx);
                    });
                    true
                })
        });
    }

    fn handle_confirmed_delete_current(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(config_id) = self.selection.selected_id else {
            self.set_error("Select a tunnel first");
            cx.notify();
            return;
        };
        self.delete_configs_blocking_running(&[config_id], window, cx);
    }
}
