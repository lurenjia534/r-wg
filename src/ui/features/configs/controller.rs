use gpui::{Context, Window};

use crate::ui::state::{PendingDraftAction, WgApp};

use super::dialogs;

pub(crate) use super::import_export::{
    handle_copy_click, handle_export_click, handle_import_click, handle_paste_click,
};
pub(crate) use super::storage::{
    delete_configs_blocking_running, delete_configs_skip_running, handle_delete_click,
    handle_rename_click, handle_save_and_restart_click, handle_save_as_click, handle_save_click,
    load_config_into_inputs, save_draft,
};

pub(crate) fn select_tunnel(
    app: &mut WgApp,
    config_id: u64,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) {
    if app.selection.selected_id == Some(config_id) {
        return;
    }
    dialogs::confirm_discard_or_save(
        app,
        PendingDraftAction::SelectConfig(config_id),
        window,
        cx,
        "Switch config?",
        "You have unsaved changes in the current config draft.",
    );
}
