use gpui::{Context, Window};

use crate::ui::state::WgApp;

impl WgApp {
    pub(crate) fn command_new_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        crate::ui::features::configs::dialogs::handle_new_draft_click(self, window, cx);
    }

    pub(crate) fn command_import_config(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        crate::ui::features::configs::import_export::handle_import_click(self, window, cx);
    }

    pub(crate) fn command_export_config(&mut self, cx: &mut Context<Self>) {
        crate::ui::features::configs::import_export::handle_export_click(self, cx);
    }

    pub(crate) fn command_paste_config(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        crate::ui::features::configs::import_export::handle_paste_click(self, window, cx);
    }

    pub(crate) fn command_copy_config(&mut self, cx: &mut Context<Self>) {
        crate::ui::features::configs::import_export::handle_copy_click(self, cx);
    }

    pub(crate) fn command_save_config(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        crate::ui::features::configs::storage::handle_save_click(self, window, cx);
    }

    pub(crate) fn command_save_and_restart_config(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        crate::ui::features::configs::storage::handle_save_and_restart_click(self, window, cx);
    }

    pub(crate) fn command_save_config_as_new(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        crate::ui::features::configs::storage::handle_save_as_click(self, window, cx);
    }

    pub(crate) fn command_rename_config(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        crate::ui::features::configs::storage::handle_rename_click(self, window, cx);
    }

    pub(crate) fn command_delete_config(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        crate::ui::features::configs::storage::handle_delete_click(self, window, cx);
    }
}
