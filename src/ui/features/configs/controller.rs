use gpui::{Context, Window};

use crate::ui::state::{PendingDraftAction, WgApp};

use super::dialogs;

use super::storage::load_config_into_inputs;

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

impl WgApp {
    pub(crate) fn load_config_into_inputs(
        &mut self,
        config_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        load_config_into_inputs(self, config_id, window, cx);
    }

    pub(crate) fn select_tunnel(
        &mut self,
        config_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        select_tunnel(self, config_id, window, cx);
    }

    pub(crate) fn selected_config(&self) -> Option<&crate::ui::state::TunnelConfig> {
        self.selection
            .selected_id
            .and_then(|id| self.configs.get_by_id(id))
    }

    pub(crate) fn clear_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.selection.loaded_config = None;
        self.selection.loading_config_id = None;
        self.selection.loading_config_path = None;
        let workspace = self.ensure_configs_workspace(cx);
        workspace.update(cx, |workspace, cx| {
            workspace.draft = crate::ui::features::configs::state::ConfigDraftState::new();
            cx.notify();
        });
        self.refresh_configs_workspace_row_flags(cx);
        self.set_editor_operation(None, cx);
        if let Some(name_input) = self.configs_name_input(cx) {
            name_input.update(cx, |input, cx| input.set_value("", window, cx));
        }
        if let Some(config_input) = self.configs_config_input(cx) {
            config_input.update(cx, |input, cx| input.set_value("", window, cx));
        }
        self.sync_configs_selection_snapshot(cx);
        self.sync_tools_active_config_snapshot(cx);
    }
}
