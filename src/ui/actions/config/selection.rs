use super::*;

impl WgApp {
    pub(crate) fn insert_or_update_config(
        &mut self,
        config: TunnelConfig,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let idx = match self.configs.find_index_by_id(config.id) {
            Some(idx) => {
                self.configs[idx] = config;
                idx
            }
            None => {
                self.configs.push(config);
                self.configs.len() - 1
            }
        };

        let config_id = self.configs[idx].id;
        self.set_selected_config_id(Some(config_id), cx);
        let updated_config = self.configs[idx].clone();
        self.upsert_configs_workspace_library_row(&updated_config, cx);
        if let Some(text) = self.configs[idx].text.clone() {
            self.set_saved_draft(config_id, self.configs[idx].name.clone().into(), text, cx);
        }
        self.load_config_into_inputs(config_id, window, cx);
        if self.configs[idx].endpoint_family == EndpointFamily::Unknown {
            let text = self.configs[idx].text.clone();
            let path = self.configs[idx].storage_path.clone();
            self.schedule_endpoint_family_refresh(config_id, text, path, cx);
        }
    }

    pub(crate) fn load_config_into_inputs(
        &mut self,
        config_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ensure_inputs(window, cx);

        let Some((name_input, config_input)) = self.configs_inputs(cx) else {
            return;
        };

        let Some(config) = self.configs.get_by_id(config_id) else {
            return;
        };
        let name = config.name.clone();
        let text = config.text.clone();
        let path = config.storage_path.clone();
        let endpoint_family = config.endpoint_family;

        if let Some(text) = text {
            let text_hash = text_hash(text.as_ref());
            if let Some(loaded) = &self.selection.loaded_config {
                if loaded.name == name && loaded.text_hash == text_hash {
                    return;
                }
            }

            self.set_editor_operation(Some(EditorOperation::LoadingConfig), cx);
            self.set_saved_draft(config_id, name.clone().into(), text.clone(), cx);

            name_input.update(cx, |input, cx| {
                input.set_value(name.clone(), window, cx);
            });
            config_input.update(cx, |input, cx| {
                input.set_value(text.clone(), window, cx);
            });
            self.set_editor_operation(None, cx);
            self.selection.loading_config_id = None;
            self.selection.loading_config_path = None;
            self.selection.loaded_config = Some(LoadedConfigState { name, text_hash });
            if endpoint_family == EndpointFamily::Unknown {
                self.schedule_endpoint_family_refresh(config_id, Some(text), path, cx);
            }
            return;
        }

        if let Some(text) = self.cached_config_text(&path) {
            let text_hash = text_hash(text.as_ref());
            if let Some(loaded) = &self.selection.loaded_config {
                if loaded.name == name && loaded.text_hash == text_hash {
                    return;
                }
            }

            self.set_editor_operation(Some(EditorOperation::LoadingConfig), cx);
            self.set_saved_draft(config_id, name.clone().into(), text.clone(), cx);

            name_input.update(cx, |input, cx| {
                input.set_value(name.clone(), window, cx);
            });
            config_input.update(cx, |input, cx| {
                input.set_value(text.clone(), window, cx);
            });
            self.set_editor_operation(None, cx);
            self.selection.loading_config_id = None;
            self.selection.loading_config_path = None;
            self.selection.loaded_config = Some(LoadedConfigState { name, text_hash });
            if endpoint_family == EndpointFamily::Unknown {
                self.schedule_endpoint_family_refresh(config_id, Some(text), path, cx);
            }
            return;
        }

        self.selection.loading_config_id = Some(config_id);
        self.selection.loading_config_path = Some(path.clone());
        self.set_editor_operation(Some(EditorOperation::LoadingConfig), cx);
        if endpoint_family == EndpointFamily::Unknown {
            self.selection.endpoint_family_loading.insert(config_id);
        }
        self.selection.loaded_config = None;
        name_input.update(cx, |input, cx| {
            input.set_value(name.clone(), window, cx);
        });
        config_input.update(cx, |input, cx| {
            if input.text().len() > 0 {
                input.set_value("", window, cx);
            }
        });
        self.set_status("Loading config...");
        cx.notify();

        let view = cx.weak_entity();
        window
            .spawn(cx, async move |cx| {
                let path_for_match = path.clone();
                let path_for_cache = path.clone();
                let read_task = cx.background_spawn(async move {
                    let text = std::fs::read_to_string(&path)?;
                    let family = resolve_endpoint_family_from_text(text.clone()).await;
                    Ok::<_, std::io::Error>((text, family))
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
                                this.set_saved_draft(
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
                                let text_hash = text_hash(text.as_ref());
                                this.selection.loaded_config =
                                    Some(LoadedConfigState { name, text_hash });
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
                                this.set_error(format!("Read failed: {err}"));
                                cx.notify();
                            }
                        }
                    }
                })
                .ok();
            })
            .detach();
    }

    pub(crate) fn select_tunnel(
        &mut self,
        config_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selection.selected_id == Some(config_id) {
            return;
        }
        self.confirm_discard_or_save(
            PendingDraftAction::SelectConfig(config_id),
            window,
            cx,
            "Switch config?",
            "You have unsaved changes in the current config draft.",
        );
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
