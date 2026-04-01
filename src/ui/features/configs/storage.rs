use std::collections::HashSet;
use std::path::PathBuf;

use gpui::{AppContext, Context, SharedString, Window};
use r_wg::application::{
    ConfigLibraryService, ConfigSourceKind, DeleteConfigsDecision, DeleteConfigsRequest,
    DeletePolicy, ExistingStoredConfig, PostDeleteSelection, PostDeleteSelectionRequest,
    RenameConfigDecision, RenameConfigRequest, SaveTargetRequest,
};

use crate::ui::persistence;
use crate::ui::state::{ConfigSource, EndpointFamily, PendingDraftAction, TunnelConfig, WgApp};

use super::{
    app::text_hash,
    dialogs, draft, endpoint_family,
    state::{DraftValidationState, EditorOperation, LoadedConfigState},
};

pub(crate) fn insert_or_update_config(
    app: &mut WgApp,
    config: TunnelConfig,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) {
    let idx = match app.configs.find_index_by_id(config.id) {
        Some(idx) => {
            app.configs[idx] = config;
            idx
        }
        None => {
            app.configs.push(config);
            app.configs.len() - 1
        }
    };

    let config_id = app.configs[idx].id;
    app.set_selected_config_id(Some(config_id), cx);
    let updated_config = app.configs[idx].clone();
    app.upsert_configs_workspace_library_row(&updated_config, cx);
    if let Some(text) = app.configs[idx].text.clone() {
        draft::set_saved_draft(
            app,
            config_id,
            app.configs[idx].name.clone().into(),
            text,
            cx,
        );
    }
    load_config_into_inputs(app, config_id, window, cx);
    if app.configs[idx].endpoint_family == EndpointFamily::Unknown {
        let text = app.configs[idx].text.clone();
        let path = app.configs[idx].storage_path.clone();
        endpoint_family::schedule_endpoint_family_refresh(app, config_id, text, path, cx);
    }
}

pub(crate) fn load_config_into_inputs(
    app: &mut WgApp,
    config_id: u64,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) {
    app.ensure_inputs(window, cx);

    let Some((name_input, config_input)) = app.configs_inputs(cx) else {
        return;
    };

    let Some(config) = app.configs.get_by_id(config_id) else {
        return;
    };
    let name = config.name.clone();
    let text = config.text.clone();
    let path = config.storage_path.clone();
    let endpoint_family = config.endpoint_family;

    if let Some(text) = text {
        let current_text_hash = text_hash(text.as_ref());
        if let Some(loaded) = &app.selection.loaded_config {
            if loaded.name == name && loaded.text_hash == current_text_hash {
                return;
            }
        }

        app.set_editor_operation(Some(EditorOperation::LoadingConfig), cx);
        draft::set_saved_draft(app, config_id, name.clone().into(), text.clone(), cx);

        name_input.update(cx, |input, cx| {
            input.set_value(name.clone(), window, cx);
        });
        config_input.update(cx, |input, cx| {
            input.set_value(text.clone(), window, cx);
        });
        app.set_editor_operation(None, cx);
        app.selection.loading_config_id = None;
        app.selection.loading_config_path = None;
        app.selection.loaded_config = Some(LoadedConfigState {
            name,
            text_hash: current_text_hash,
        });
        if endpoint_family == EndpointFamily::Unknown {
            endpoint_family::schedule_endpoint_family_refresh(app, config_id, Some(text), path, cx);
        }
        return;
    }

    if let Some(text) = app.cached_config_text(&path) {
        let current_text_hash = text_hash(text.as_ref());
        if let Some(loaded) = &app.selection.loaded_config {
            if loaded.name == name && loaded.text_hash == current_text_hash {
                return;
            }
        }

        app.set_editor_operation(Some(EditorOperation::LoadingConfig), cx);
        draft::set_saved_draft(app, config_id, name.clone().into(), text.clone(), cx);

        name_input.update(cx, |input, cx| {
            input.set_value(name.clone(), window, cx);
        });
        config_input.update(cx, |input, cx| {
            input.set_value(text.clone(), window, cx);
        });
        app.set_editor_operation(None, cx);
        app.selection.loading_config_id = None;
        app.selection.loading_config_path = None;
        app.selection.loaded_config = Some(LoadedConfigState {
            name,
            text_hash: current_text_hash,
        });
        if endpoint_family == EndpointFamily::Unknown {
            endpoint_family::schedule_endpoint_family_refresh(app, config_id, Some(text), path, cx);
        }
        return;
    }

    app.selection.loading_config_id = Some(config_id);
    app.selection.loading_config_path = Some(path.clone());
    app.set_editor_operation(Some(EditorOperation::LoadingConfig), cx);
    if endpoint_family == EndpointFamily::Unknown {
        app.selection.endpoint_family_loading.insert(config_id);
    }
    app.selection.loaded_config = None;
    name_input.update(cx, |input, cx| {
        input.set_value(name.clone(), window, cx);
    });
    config_input.update(cx, |input, cx| {
        if input.text().len() > 0 {
            input.set_value("", window, cx);
        }
    });
    app.set_status("Loading config...");
    cx.notify();

    let view = cx.weak_entity();
    let config_library = app.config_library.clone();
    window
        .spawn(cx, async move |cx| {
            let path_for_match = path.clone();
            let path_for_cache = path.clone();
            let read_task = cx.background_spawn(async move {
                let text = config_library.read_config_text(&path)?;
                let family = endpoint_family::resolve_endpoint_family_from_text(text.clone()).await;
                Ok::<_, String>((text, family))
            });
            let result = read_task.await;
            view.update_in(cx, |this, window, cx| {
                let Some(config) = this.configs.get_by_id(config_id) else {
                    this.selection.endpoint_family_loading.remove(&config_id);
                    if this.selection.loading_config_id == Some(config_id) {
                        this.selection.loading_config_id = None;
                        this.selection.loading_config_path = None;
                    }
                    return;
                };
                if config.storage_path != path_for_match {
                    this.selection.endpoint_family_loading.remove(&config_id);
                    if this.selection.loading_config_id == Some(config_id)
                        && this.selection.loading_config_path.as_ref() == Some(&path_for_match)
                    {
                        this.selection.loading_config_id = None;
                        this.selection.loading_config_path = None;
                    }
                    return;
                }
                let should_write_ui = this.selection.selected_id == Some(config_id)
                    && this.selection.loading_config_id == Some(config_id)
                    && this.selection.loading_config_path.as_ref() == Some(&path_for_match);

                match result {
                    Ok((text, family)) => {
                        let text: SharedString = text.into();
                        this.cache_config_text(path_for_cache, text.clone());
                        if let Some(config) = this.configs.get_mut_by_id(config_id) {
                            config.endpoint_family = family;
                        }
                        this.selection.endpoint_family_loading.remove(&config_id);
                        if should_write_ui {
                            this.selection.loading_config_id = None;
                            this.selection.loading_config_path = None;
                            draft::set_saved_draft(
                                this,
                                config_id,
                                name.clone().into(),
                                text.clone(),
                                cx,
                            );
                            if let Some(config_input) = this.configs_config_input(cx) {
                                config_input.update(cx, |input, cx| {
                                    input.set_value(text.clone(), window, cx);
                                });
                            }
                            if let Some(name_input) = this.configs_name_input(cx) {
                                name_input.update(cx, |input, cx| {
                                    input.set_value(name.clone(), window, cx);
                                });
                            }
                            let current_text_hash = text_hash(text.as_ref());
                            this.selection.loaded_config = Some(LoadedConfigState {
                                name,
                                text_hash: current_text_hash,
                            });
                            this.set_editor_operation(None, cx);
                            this.set_status("Loaded config");
                        }
                        cx.notify();
                    }
                    Err(err) => {
                        this.selection.endpoint_family_loading.remove(&config_id);
                        if should_write_ui {
                            this.selection.loading_config_id = None;
                            this.selection.loading_config_path = None;
                            this.set_editor_operation(None, cx);
                            this.set_error(err);
                            cx.notify();
                        }
                    }
                }
            })
            .ok();
        })
        .detach();
}

pub(crate) fn save_draft(
    app: &mut WgApp,
    force_new: bool,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) {
    app.ensure_inputs(window, cx);
    draft::sync_draft_from_inputs(app, cx);
    draft::apply_draft_validation(app, cx);
    let draft = app.configs_draft_snapshot(cx);

    let text = draft.text.clone();
    let storage = match app.configs.ensure_storage() {
        Ok(storage) => storage,
        Err(err) => {
            app.set_error(err);
            cx.notify();
            return;
        }
    };
    let existing_configs = existing_stored_configs(app);
    let next_id = app.configs.next_config_id();
    let next_storage_path = persistence::config_path(&storage, next_id);
    let target_plan = match app.config_library.plan_save_target(SaveTargetRequest {
        requested_name: draft.name.as_ref(),
        text: text.as_ref(),
        source_id: draft.source_id,
        force_new,
        existing_configs: &existing_configs,
        next_id,
        next_storage_path,
    }) {
        Ok(plan) => plan,
        Err(err) => {
            app.set_error(err.message());
            cx.notify();
            return;
        }
    };

    if text.as_ref().trim().is_empty() {
        app.set_error("Config text is required");
        cx.notify();
        return;
    }

    let endpoint_family = match &draft.validation {
        DraftValidationState::Valid {
            endpoint_family, ..
        } => *endpoint_family,
        DraftValidationState::Invalid { line, message, .. } => {
            app.set_error(match line {
                Some(line) => format!("Invalid config: line {line}: {message}"),
                None => format!("Invalid config: {message}"),
            });
            cx.notify();
            return;
        }
        DraftValidationState::Idle => {
            app.set_error("Config text is required");
            cx.notify();
            return;
        }
    };

    let id = target_plan.id;
    let name = target_plan.name;
    let storage_path = target_plan.storage_path;
    let source = if target_plan.is_new {
        config_source_from_kind(target_plan.source)
    } else {
        app.configs
            .find_by_id(id)
            .map(|config| config.source)
            .unwrap_or_else(|| config_source_from_kind(target_plan.source))
    };
    let name_lower = name.to_lowercase();
    let text_for_write = text.to_string();
    let text_for_state = text.clone();

    app.set_editor_operation(Some(EditorOperation::Saving), cx);
    app.set_status("Saving config...");
    cx.notify();

    let storage_path_for_write = storage_path.clone();
    let view = cx.weak_entity();
    let config_library = app.config_library.clone();
    window
        .spawn(cx, async move |cx| {
            let write_task = cx.background_spawn(async move {
                config_library.write_config_text(&storage_path_for_write, &text_for_write)
            });
            let result = write_task.await;
            view.update_in(cx, |this, window, cx| {
                this.set_editor_operation(None, cx);
                match result {
                    Ok(()) => {
                        insert_or_update_config(
                            this,
                            TunnelConfig {
                                id,
                                name: name.to_string(),
                                name_lower,
                                text: Some(text_for_state),
                                source,
                                storage_path,
                                endpoint_family,
                            },
                            window,
                            cx,
                        );
                        this.persist_state_async(cx);
                        if force_new {
                            this.set_status(format!("Saved {name} as a new config"));
                        } else {
                            this.set_status("Saved tunnel");
                        }
                        if let Some(action) = this.take_configs_pending_action(cx) {
                            dialogs::run_pending_draft_action(this, action, window, cx);
                        }
                    }
                    Err(err) => {
                        this.set_error(err);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
}

pub(crate) fn handle_save_click(app: &mut WgApp, window: &mut Window, cx: &mut Context<WgApp>) {
    save_draft(app, false, window, cx);
}

pub(crate) fn handle_save_and_restart_click(
    app: &mut WgApp,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) {
    app.set_configs_pending_action(Some(PendingDraftAction::RestartTunnel), cx);
    save_draft(app, false, window, cx);
}

pub(crate) fn handle_save_as_click(app: &mut WgApp, window: &mut Window, cx: &mut Context<WgApp>) {
    save_draft(app, true, window, cx);
}

pub(crate) fn handle_rename_click(app: &mut WgApp, window: &mut Window, cx: &mut Context<WgApp>) {
    app.ensure_inputs(window, cx);
    draft::sync_draft_from_inputs(app, cx);
    draft::apply_draft_validation(app, cx);
    let draft = app.configs_draft_snapshot(cx);
    let existing_configs = existing_stored_configs(app);
    let rename = match app.config_library.plan_rename(RenameConfigRequest {
        requested_name: draft.name.as_ref(),
        source_id: draft.source_id,
        selected_id: app.selection.selected_id,
        existing_configs: &existing_configs,
    }) {
        Ok(RenameConfigDecision::Unchanged) => {
            app.set_status("Name unchanged");
            cx.notify();
            return;
        }
        Ok(RenameConfigDecision::Rename {
            config_id,
            previous_name,
            name,
        }) => (config_id, previous_name, name),
        Err(err) => {
            app.set_error(err.message());
            cx.notify();
            return;
        }
    };
    let (config_id, old_name, new_name) = rename;
    let Some(idx) = app.configs.find_index_by_id(config_id) else {
        app.set_error("Selected tunnel no longer exists");
        cx.notify();
        return;
    };

    app.configs[idx].name = new_name.clone();
    app.configs[idx].name_lower = new_name.to_lowercase();
    let updated_config = app.configs[idx].clone();
    app.upsert_configs_workspace_library_row(&updated_config, cx);
    if let Some(loaded) = &mut app.selection.loaded_config {
        if loaded.name == old_name {
            loaded.name = new_name.to_string();
        }
    }
    if draft.source_id == Some(config_id) {
        let workspace = app.ensure_configs_workspace(cx);
        let base_name: SharedString = new_name.clone().into();
        workspace.update(cx, |workspace, cx| {
            workspace.draft.base_name = base_name;
            cx.notify();
        });
        draft::apply_draft_validation(app, cx);
    }
    if app.runtime.running_name.as_deref() == Some(old_name.as_str()) {
        app.runtime.running_name = Some(new_name.clone());
        app.runtime.runtime_revision = app.runtime.runtime_revision.wrapping_add(1);
    }
    app.set_status(format!("Renamed to {new_name}"));
    app.persist_state_async(cx);
    cx.notify();
}

pub(crate) fn handle_delete_click(app: &mut WgApp, window: &mut Window, cx: &mut Context<WgApp>) {
    let Some(_config_id) = app.selection.selected_id else {
        app.set_error("Select a tunnel first");
        cx.notify();
        return;
    };
    if app.configs_draft_snapshot(cx).is_dirty() {
        dialogs::confirm_discard_or_save(
            app,
            PendingDraftAction::DeleteCurrent,
            window,
            cx,
            "Delete config?",
            "You have unsaved changes in the current config draft.",
        );
        return;
    }
    dialogs::open_delete_current_config_dialog(app, window, cx);
}

pub(crate) fn delete_configs_blocking_running(
    app: &mut WgApp,
    ids: &[u64],
    window: &mut Window,
    cx: &mut Context<WgApp>,
) {
    delete_configs_internal(app, ids, DeletePolicy::BlockRunning, window, cx);
}

pub(crate) fn delete_configs_skip_running(
    app: &mut WgApp,
    ids: &[u64],
    window: &mut Window,
    cx: &mut Context<WgApp>,
) {
    delete_configs_internal(app, ids, DeletePolicy::SkipRunning, window, cx);
}

fn delete_configs_internal(
    app: &mut WgApp,
    ids: &[u64],
    policy: DeletePolicy,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) {
    let existing_configs = existing_stored_configs(app);
    let plan = match app.config_library.plan_delete(DeleteConfigsRequest {
        requested_ids: ids,
        existing_configs: &existing_configs,
        running_id: app.runtime.running_id,
        running_name: app.runtime.running_name.as_deref(),
        policy,
    }) {
        DeleteConfigsDecision::NoSelection => {
            app.set_error("No configs selected");
            cx.notify();
            return;
        }
        DeleteConfigsDecision::BlockedRunning => {
            app.set_error("Stop the tunnel before deleting");
            cx.notify();
            return;
        }
        DeleteConfigsDecision::OnlySkippedRunning { skipped_running } => {
            app.set_status(
                app.config_library
                    .delete_status_message(&[], skipped_running.len()),
            );
            cx.notify();
            return;
        }
        DeleteConfigsDecision::Delete(plan) => plan,
    };
    let to_delete_ids: HashSet<u64> = plan.deleted_ids.iter().copied().collect();
    let deleted_names = plan.deleted_names;
    let deleted_paths = plan.deleted_paths;
    let skipped_running = plan.skipped_running;

    app.set_editor_operation(Some(EditorOperation::Deleting), cx);
    let prev_selected_id = app.selection.selected_id;
    let prev_selected_idx = prev_selected_id.and_then(|id| app.configs.find_index_by_id(id));

    for id in &to_delete_ids {
        app.stats.traffic.remove_config(*id);
    }

    app.configs.retain(|cfg| !to_delete_ids.contains(&cfg.id));
    app.remove_configs_workspace_library_rows(&to_delete_ids, cx);

    let deleted_paths_set: HashSet<PathBuf> = deleted_paths.iter().cloned().collect();
    app.selection
        .config_text_cache
        .retain(|path, _| !deleted_paths_set.contains(path));
    app.selection
        .config_text_cache_order
        .retain(|path| !deleted_paths_set.contains(path));
    app.selection
        .proxy_selected_ids
        .retain(|id| !to_delete_ids.contains(id));
    app.selection
        .endpoint_family_loading
        .retain(|id| !to_delete_ids.contains(id));
    app.selection.loading_config_id = None;
    app.selection.loading_config_path = None;

    let remaining_ids = app
        .configs
        .iter()
        .map(|config| config.id)
        .collect::<Vec<_>>();
    match app
        .config_library
        .plan_post_delete_selection(PostDeleteSelectionRequest {
            remaining_ids: &remaining_ids,
            previous_selected_id: prev_selected_id,
            previous_selected_index: prev_selected_idx,
        }) {
        PostDeleteSelection::Clear => {
            app.set_selected_config_id(None, cx);
            app.clear_inputs(window, cx);
        }
        PostDeleteSelection::Keep(selected_id) => {
            app.set_selected_config_id(Some(selected_id), cx);
        }
        PostDeleteSelection::SelectFallback(selected_id) => {
            app.set_selected_config_id(Some(selected_id), cx);
            load_config_into_inputs(app, selected_id, window, cx);
        }
    }

    app.set_status(
        app.config_library
            .delete_status_message(&deleted_names, skipped_running.len()),
    );
    app.persist_state_async(cx);
    app.set_editor_operation(None, cx);
    cx.notify();

    cx.spawn(async move |view, cx| {
        let config_library = ConfigLibraryService::new();
        let delete_task =
            cx.background_spawn(async move { config_library.delete_config_files(&deleted_paths) });
        if let Err(err) = delete_task.await {
            view.update(cx, |this, cx| {
                this.set_error(err);
                cx.notify();
            })
            .ok();
        }
    })
    .detach();
}

fn existing_stored_configs(app: &WgApp) -> Vec<ExistingStoredConfig<'_>> {
    app.configs
        .iter()
        .map(|config| ExistingStoredConfig {
            id: config.id,
            name: config.name.as_str(),
            storage_path: &config.storage_path,
            source: config_source_kind(&config.source),
        })
        .collect()
}

fn config_source_kind(source: &ConfigSource) -> ConfigSourceKind {
    match source {
        ConfigSource::File { .. } => ConfigSourceKind::File,
        ConfigSource::Paste => ConfigSourceKind::Paste,
    }
}

fn config_source_from_kind(source: ConfigSourceKind) -> ConfigSource {
    match source {
        ConfigSourceKind::File => ConfigSource::File { origin_path: None },
        ConfigSourceKind::Paste => ConfigSource::Paste,
    }
}

impl WgApp {
    pub(crate) fn handle_save_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        handle_save_click(self, window, cx);
    }

    pub(crate) fn handle_save_and_restart_click(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        handle_save_and_restart_click(self, window, cx);
    }

    pub(crate) fn handle_save_as_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        handle_save_as_click(self, window, cx);
    }

    pub(crate) fn handle_rename_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        handle_rename_click(self, window, cx);
    }

    pub(crate) fn handle_delete_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        handle_delete_click(self, window, cx);
    }

    pub(crate) fn delete_configs_blocking_running(
        &mut self,
        ids: &[u64],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        delete_configs_blocking_running(self, ids, window, cx);
    }

    pub(crate) fn delete_configs_skip_running(
        &mut self,
        ids: &[u64],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        delete_configs_skip_running(self, ids, window, cx);
    }

    pub(crate) fn next_config_name(&self, base: &str) -> String {
        ConfigLibraryService::new()
            .next_available_name(self.configs.iter().map(|cfg| cfg.name.as_str()), base)
    }
}
