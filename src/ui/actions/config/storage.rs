use super::naming::next_available_name;
use super::*;

impl WgApp {
    pub(super) fn save_draft(
        &mut self,
        force_new: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ensure_inputs(window, cx);
        self.sync_draft_from_inputs(cx);
        self.apply_draft_validation(cx);
        let draft = self.configs_draft_snapshot(cx);

        let name = draft.name.to_string();
        let name = name.trim();
        if name.is_empty() {
            self.set_error("Tunnel name is required");
            cx.notify();
            return;
        }

        let text = draft.text.clone();
        if text.as_ref().trim().is_empty() {
            self.set_error("Config text is required");
            cx.notify();
            return;
        }

        let endpoint_family = match &draft.validation {
            DraftValidationState::Valid {
                endpoint_family, ..
            } => *endpoint_family,
            DraftValidationState::Invalid { line, message, .. } => {
                self.set_error(match line {
                    Some(line) => format!("Invalid config: line {line}: {message}"),
                    None => format!("Invalid config: {message}"),
                });
                cx.notify();
                return;
            }
            DraftValidationState::Idle => {
                self.set_error("Config text is required");
                cx.notify();
                return;
            }
        };

        let source_id = if force_new { None } else { draft.source_id };

        if self
            .configs
            .iter()
            .any(|entry| entry.name == name && Some(entry.id) != source_id)
        {
            self.set_error("Tunnel name already exists");
            cx.notify();
            return;
        }

        let name = name.to_string();
        let storage = match self.configs.ensure_storage() {
            Ok(storage) => storage,
            Err(err) => {
                self.set_error(err);
                cx.notify();
                return;
            }
        };

        let (id, storage_path, source) = match source_id.and_then(|id| self.configs.find_by_id(id))
        {
            Some(cfg) => (cfg.id, cfg.storage_path, cfg.source),
            None => {
                let id = self.configs.alloc_config_id();
                let storage_path = persistence::config_path(&storage, id);
                (id, storage_path, ConfigSource::Paste)
            }
        };

        let name_lower = name.to_lowercase();
        let text_for_write = text.to_string();
        let text_for_state = text.clone();

        self.set_editor_operation(Some(EditorOperation::Saving), cx);
        self.set_status("Saving config...");
        cx.notify();

        let storage_path_for_write = storage_path.clone();
        let view = cx.weak_entity();
        window
            .spawn(cx, async move |cx| {
                let write_task = cx.background_spawn(async move {
                    persistence::write_config_text(&storage_path_for_write, &text_for_write)
                });
                let result = write_task.await;
                view.update_in(cx, |this, window, cx| {
                    this.set_editor_operation(None, cx);
                    match result {
                        Ok(()) => {
                            this.insert_or_update_config(
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
                                this.run_pending_draft_action(action, window, cx);
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

    pub(crate) fn handle_save_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_draft(false, window, cx);
    }

    pub(crate) fn handle_save_and_restart_click(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_configs_pending_action(Some(PendingDraftAction::RestartTunnel), cx);
        self.save_draft(false, window, cx);
    }

    pub(crate) fn handle_save_as_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_draft(true, window, cx);
    }

    pub(crate) fn handle_rename_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ensure_inputs(window, cx);
        self.sync_draft_from_inputs(cx);
        self.apply_draft_validation(cx);
        let draft = self.configs_draft_snapshot(cx);
        let new_name = draft.name.to_string();
        let new_name = new_name.trim();
        if new_name.is_empty() {
            self.set_error("Tunnel name is required");
            cx.notify();
            return;
        }

        let Some(config_id) = draft.source_id.or(self.selection.selected_id) else {
            self.set_error("Select a tunnel first");
            cx.notify();
            return;
        };
        let Some(idx) = self.configs.find_index_by_id(config_id) else {
            self.set_error("Selected tunnel no longer exists");
            cx.notify();
            return;
        };
        let old_name = self.configs[idx].name.clone();
        if old_name == new_name {
            self.set_status("Name unchanged");
            cx.notify();
            return;
        }
        if self.configs.iter().any(|cfg| cfg.name == new_name) {
            self.set_error("Tunnel name already exists");
            cx.notify();
            return;
        }

        self.configs[idx].name = new_name.to_string();
        self.configs[idx].name_lower = new_name.to_lowercase();
        let updated_config = self.configs[idx].clone();
        self.upsert_configs_workspace_library_row(&updated_config, cx);
        if let Some(loaded) = &mut self.selection.loaded_config {
            if loaded.name == old_name {
                loaded.name = new_name.to_string();
            }
        }
        if draft.source_id == Some(config_id) {
            let workspace = self.ensure_configs_workspace(cx);
            let base_name: SharedString = new_name.to_string().into();
            workspace.update(cx, |workspace, cx| {
                workspace.draft.base_name = base_name;
                cx.notify();
            });
            self.apply_draft_validation(cx);
        }
        if self.runtime.running_name.as_deref() == Some(old_name.as_str()) {
            self.runtime.running_name = Some(new_name.to_string());
            self.runtime.runtime_revision = self.runtime.runtime_revision.wrapping_add(1);
        }
        self.set_status(format!("Renamed to {new_name}"));
        self.persist_state_async(cx);
        cx.notify();
    }

    pub(crate) fn handle_delete_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(_config_id) = self.selection.selected_id else {
            self.set_error("Select a tunnel first");
            cx.notify();
            return;
        };
        if self.configs_draft_snapshot(cx).is_dirty() {
            self.confirm_discard_or_save(
                PendingDraftAction::DeleteCurrent,
                window,
                cx,
                "Delete config?",
                "You have unsaved changes in the current config draft.",
            );
            return;
        }
        self.open_delete_current_config_dialog(window, cx);
    }

    pub(crate) fn delete_configs_blocking_running(
        &mut self,
        ids: &[u64],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.delete_configs_internal(ids, DeletePolicy::BlockRunning, window, cx);
    }

    pub(crate) fn delete_configs_skip_running(
        &mut self,
        ids: &[u64],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.delete_configs_internal(ids, DeletePolicy::SkipRunning, window, cx);
    }

    fn delete_configs_internal(
        &mut self,
        ids: &[u64],
        policy: DeletePolicy,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if ids.is_empty() {
            self.set_error("Select a tunnel first");
            cx.notify();
            return;
        }

        let ids: HashSet<u64> = ids.iter().copied().collect();
        let running_id = self.runtime.running_id;
        let running_name = self.runtime.running_name.clone();

        let mut to_delete_ids = HashSet::new();
        let mut deleted_names = Vec::new();
        let mut deleted_paths = Vec::new();
        let mut skipped_running = Vec::new();

        for cfg in &self.configs {
            if !ids.contains(&cfg.id) {
                continue;
            }
            let is_running =
                running_id == Some(cfg.id) || running_name.as_deref() == Some(cfg.name.as_str());
            if is_running {
                match policy {
                    DeletePolicy::BlockRunning => {
                        self.set_error("Stop the tunnel before deleting");
                        cx.notify();
                        return;
                    }
                    DeletePolicy::SkipRunning => {
                        skipped_running.push(cfg.name.clone());
                        continue;
                    }
                }
            }

            to_delete_ids.insert(cfg.id);
            deleted_names.push(cfg.name.clone());
            deleted_paths.push(cfg.storage_path.clone());
        }

        if to_delete_ids.is_empty() {
            if !skipped_running.is_empty() {
                self.set_status(format_delete_status(&[], skipped_running.len()));
            } else {
                self.set_error("No configs selected");
            }
            cx.notify();
            return;
        }

        self.set_editor_operation(Some(EditorOperation::Deleting), cx);
        let prev_selected_id = self.selection.selected_id;
        let prev_selected_idx = prev_selected_id.and_then(|id| self.configs.find_index_by_id(id));

        for id in &to_delete_ids {
            self.stats.traffic.remove_config(*id);
        }

        self.configs.retain(|cfg| !to_delete_ids.contains(&cfg.id));
        self.remove_configs_workspace_library_rows(&to_delete_ids, cx);

        let deleted_paths_set: HashSet<PathBuf> = deleted_paths.iter().cloned().collect();
        self.selection
            .config_text_cache
            .retain(|path, _| !deleted_paths_set.contains(path));
        self.selection
            .config_text_cache_order
            .retain(|path| !deleted_paths_set.contains(path));
        self.selection
            .proxy_selected_ids
            .retain(|id| !to_delete_ids.contains(id));
        self.selection
            .endpoint_family_loading
            .retain(|id| !to_delete_ids.contains(id));
        self.selection.loading_config_id = None;
        self.selection.loading_config_path = None;

        if self.configs.is_empty() {
            self.set_selected_config_id(None, cx);
            self.clear_inputs(window, cx);
        } else if let Some(prev_id) = prev_selected_id {
            if self.configs.get_by_id(prev_id).is_some() {
                self.set_selected_config_id(Some(prev_id), cx);
            } else if let Some(prev_idx) = prev_selected_idx {
                let idx = prev_idx.min(self.configs.len() - 1);
                let fallback_id = self.configs[idx].id;
                self.set_selected_config_id(Some(fallback_id), cx);
                self.load_config_into_inputs(fallback_id, window, cx);
            } else {
                self.set_selected_config_id(None, cx);
                self.clear_inputs(window, cx);
            }
        } else {
            self.set_selected_config_id(None, cx);
            self.clear_inputs(window, cx);
        }

        self.set_status(format_delete_status(&deleted_names, skipped_running.len()));
        self.persist_state_async(cx);
        self.set_editor_operation(None, cx);
        cx.notify();

        cx.spawn(async move |view, cx| {
            let delete_task = cx.background_spawn(async move {
                let mut first_error: Option<std::io::Error> = None;
                for path in deleted_paths {
                    match std::fs::remove_file(&path) {
                        Ok(()) => {}
                        Err(err) if err.kind() == ErrorKind::NotFound => {}
                        Err(err) => {
                            first_error = Some(err);
                            break;
                        }
                    }
                }
                first_error
            });
            if let Some(err) = delete_task.await {
                view.update(cx, |this, cx| {
                    this.set_error(format!("Remove file failed: {err}"));
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    pub(crate) fn next_config_name(&self, base: &str) -> String {
        next_available_name(self.configs.iter().map(|cfg| cfg.name.as_str()), base)
    }
}
