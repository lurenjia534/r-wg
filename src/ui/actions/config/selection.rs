use crate::ui::features::configs::controller;

use super::*;

impl WgApp {
    pub(crate) fn load_config_into_inputs(
        &mut self,
        config_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        controller::load_config_into_inputs(self, config_id, window, cx);
    }

    pub(crate) fn select_tunnel(
        &mut self,
        config_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        controller::select_tunnel(self, config_id, window, cx);
    }

    pub(crate) fn selected_config(&self) -> Option<&TunnelConfig> {
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
            workspace.draft = ConfigDraftState::new();
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
