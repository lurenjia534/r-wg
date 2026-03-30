use super::*;

impl WgApp {
    pub(crate) fn upsert_configs_workspace_library_row(
        &mut self,
        config: &TunnelConfig,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.ui.configs_workspace.clone() else {
            return;
        };
        let config = config.clone();
        let running_id = self.runtime.running_id;
        let running_name = self.runtime.running_name.clone();
        workspace.update(cx, |workspace, cx| {
            if workspace.upsert_library_row(&config, running_id, running_name.as_deref()) {
                cx.notify();
            }
        });
    }

    pub(crate) fn remove_configs_workspace_library_rows(
        &mut self,
        ids: &HashSet<u64>,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.ui.configs_workspace.clone() else {
            return;
        };
        let ids = ids.clone();
        workspace.update(cx, |workspace, cx| {
            if workspace.remove_library_rows(&ids) {
                cx.notify();
            }
        });
    }

    pub(crate) fn append_configs_workspace_library_rows(
        &mut self,
        configs: &[TunnelConfig],
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.ui.configs_workspace.clone() else {
            return;
        };
        let configs = configs.to_vec();
        let running_id = self.runtime.running_id;
        let running_name = self.runtime.running_name.clone();
        workspace.update(cx, |workspace, cx| {
            if workspace.append_library_rows(&configs, running_id, running_name.as_deref()) {
                cx.notify();
            }
        });
    }

    pub(crate) fn refresh_configs_workspace_row_flags(&mut self, cx: &mut Context<Self>) {
        let Some(workspace) = self.ui.configs_workspace.clone() else {
            return;
        };
        let running_id = self.runtime.running_id;
        let running_name = self.runtime.running_name.clone();
        workspace.update(cx, |workspace, cx| {
            if workspace.refresh_library_row_flags(running_id, running_name.as_deref()) {
                cx.notify();
            }
        });
    }

    pub(crate) fn set_selected_config_id(
        &mut self,
        selected_id: Option<u64>,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.selection.selected_id == selected_id {
            return false;
        }
        self.selection.selected_id = selected_id;
        self.selection.selection_revision = self.selection.selection_revision.wrapping_add(1);
        self.sync_configs_selection_snapshot(cx);
        self.sync_tools_active_config_snapshot(cx);
        true
    }

    pub(crate) fn sync_configs_selection_snapshot(&mut self, cx: &mut Context<Self>) {
        let Some(workspace) = self.ui.configs_workspace.clone() else {
            return;
        };
        let has_selection = self.selection.selected_id.is_some();
        workspace.update(cx, |workspace, cx| {
            if workspace.has_selection != has_selection {
                workspace.has_selection = has_selection;
                cx.notify();
            }
        });
    }

    pub(crate) fn set_editor_operation(
        &mut self,
        operation: Option<EditorOperation>,
        cx: &mut Context<Self>,
    ) {
        let workspace = self.ensure_configs_workspace(cx);
        workspace.update(cx, |workspace, cx| {
            workspace.operation = operation;
            cx.notify();
        });
    }

    pub(crate) fn configs_draft_snapshot(&self, cx: &mut Context<Self>) -> ConfigDraftState {
        self.ui
            .configs_workspace
            .as_ref()
            .map(|workspace| workspace.read(cx).draft.clone())
            .unwrap_or_else(ConfigDraftState::new)
    }

    pub(crate) fn configs_operation_snapshot(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<EditorOperation> {
        self.ui
            .configs_workspace
            .as_ref()
            .map(|workspace| workspace.read(cx).operation.clone())
            .unwrap_or(None)
    }

    pub(crate) fn configs_is_busy(&self, cx: &mut Context<Self>) -> bool {
        self.configs_operation_snapshot(cx).is_some()
    }

    pub(crate) fn set_configs_pending_action(
        &mut self,
        action: Option<PendingDraftAction>,
        cx: &mut Context<Self>,
    ) {
        if self.ui.configs_workspace.is_none() {
            let _ = self.ensure_configs_workspace(cx);
        }
        if let Some(workspace) = self.ui.configs_workspace.clone() {
            workspace.update(cx, |workspace, cx| {
                workspace.pending_action = action;
                cx.notify();
            });
        }
    }

    pub(crate) fn take_configs_pending_action(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<PendingDraftAction> {
        if let Some(workspace) = self.ui.configs_workspace.clone() {
            let mut action = None;
            workspace.update(cx, |workspace, cx| {
                action = workspace.pending_action.take();
                cx.notify();
            });
            return action;
        }
        None
    }

    pub(crate) fn ensure_proxy_search_input(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.ui.proxy_search_input.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search nodes"));
            self.ui.proxy_search_input = Some(input);
        }
    }
}
