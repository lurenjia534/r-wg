use super::*;

impl WgApp {
    pub(super) fn apply_draft_validation(&mut self, cx: &mut Context<Self>) {
        let workspace = self.ensure_configs_workspace(cx);
        workspace.update(cx, |workspace, cx| {
            workspace.apply_draft_validation(self.runtime.running_id);
            cx.notify();
        });
        self.refresh_configs_workspace_row_flags(cx);
        self.sync_tools_active_config_snapshot(cx);
    }

    pub(super) fn set_saved_draft(
        &mut self,
        source_id: u64,
        name: SharedString,
        text: SharedString,
        cx: &mut Context<Self>,
    ) {
        let workspace = self.ensure_configs_workspace(cx);
        let workspace_name = name.clone();
        let workspace_text = text.clone();
        workspace.update(cx, |workspace, cx| {
            workspace.set_saved_draft(source_id, workspace_name, workspace_text);
            cx.notify();
        });
        self.refresh_configs_workspace_row_flags(cx);
        self.apply_draft_validation(cx);
        self.sync_tools_active_config_snapshot(cx);
    }

    pub(super) fn set_unsaved_draft(
        &mut self,
        name: SharedString,
        text: SharedString,
        cx: &mut Context<Self>,
    ) {
        let workspace = self.ensure_configs_workspace(cx);
        let workspace_name = name.clone();
        let workspace_text = text.clone();
        workspace.update(cx, |workspace, cx| {
            workspace.set_unsaved_draft(workspace_name, workspace_text);
            cx.notify();
        });
        self.refresh_configs_workspace_row_flags(cx);
        self.apply_draft_validation(cx);
        self.sync_tools_active_config_snapshot(cx);
    }

    pub(crate) fn sync_draft_from_inputs(&mut self, cx: &mut Context<Self>) {
        let Some((name_input, config_input)) = self.configs_inputs(cx) else {
            return;
        };
        let name = name_input.read(cx).value();
        let text = config_input.read(cx).value();
        self.sync_draft_from_values(name, text, cx);
    }

    pub(super) fn discard_current_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let draft = self.configs_draft_snapshot(cx);
        if let Some(source_id) = draft.source_id.or(self.selection.selected_id) {
            self.set_selected_config_id(Some(source_id), cx);
            self.load_config_into_inputs(source_id, window, cx);
        } else {
            self.set_selected_config_id(None, cx);
            self.clear_inputs(window, cx);
        }
    }
}
