use gpui::{Context, SharedString, Window};

use crate::ui::state::WgApp;

pub(crate) fn apply_draft_validation(app: &mut WgApp, cx: &mut Context<WgApp>) {
    let workspace = app.ensure_configs_workspace(cx);
    workspace.update(cx, |workspace, cx| {
        workspace.apply_draft_validation(app.runtime.running_id);
        cx.notify();
    });
    app.refresh_configs_workspace_row_flags(cx);
    app.sync_tools_active_config_snapshot(cx);
}

pub(crate) fn set_saved_draft(
    app: &mut WgApp,
    source_id: u64,
    name: SharedString,
    text: SharedString,
    cx: &mut Context<WgApp>,
) {
    let workspace = app.ensure_configs_workspace(cx);
    let workspace_name = name.clone();
    let workspace_text = text.clone();
    workspace.update(cx, |workspace, cx| {
        workspace.set_saved_draft(source_id, workspace_name, workspace_text);
        cx.notify();
    });
    app.refresh_configs_workspace_row_flags(cx);
    apply_draft_validation(app, cx);
    app.sync_tools_active_config_snapshot(cx);
}

pub(crate) fn set_unsaved_draft(
    app: &mut WgApp,
    name: SharedString,
    text: SharedString,
    cx: &mut Context<WgApp>,
) {
    let workspace = app.ensure_configs_workspace(cx);
    let workspace_name = name.clone();
    let workspace_text = text.clone();
    workspace.update(cx, |workspace, cx| {
        workspace.set_unsaved_draft(workspace_name, workspace_text);
        cx.notify();
    });
    app.refresh_configs_workspace_row_flags(cx);
    apply_draft_validation(app, cx);
    app.sync_tools_active_config_snapshot(cx);
}

pub(crate) fn sync_draft_from_inputs(app: &mut WgApp, cx: &mut Context<WgApp>) {
    let Some((name_input, config_input)) = app.configs_inputs(cx) else {
        return;
    };
    let name = name_input.read(cx).value();
    let text = config_input.read(cx).value();
    app.sync_draft_from_values(name, text, cx);
}

pub(crate) fn discard_current_draft(
    app: &mut WgApp,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) {
    let draft = app.configs_draft_snapshot(cx);
    if let Some(source_id) = draft.source_id.or(app.selection.selected_id) {
        app.set_selected_config_id(Some(source_id), cx);
        app.load_config_into_inputs(source_id, window, cx);
    } else {
        app.set_selected_config_id(None, cx);
        app.clear_inputs(window, cx);
    }
}
