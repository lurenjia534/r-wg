use super::*;

impl WgApp {
    pub(super) fn configs_name_input(&self, cx: &mut Context<Self>) -> Option<Entity<InputState>> {
        self.ui
            .configs_workspace
            .as_ref()
            .and_then(|workspace| workspace.read(cx).name_input.clone())
    }

    pub(super) fn configs_config_input(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<Entity<InputState>> {
        self.ui
            .configs_workspace
            .as_ref()
            .and_then(|workspace| workspace.read(cx).config_input.clone())
    }

    pub(super) fn configs_inputs(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<(Entity<InputState>, Entity<InputState>)> {
        Some((self.configs_name_input(cx)?, self.configs_config_input(cx)?))
    }

    pub(crate) fn ensure_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let workspace = self.ensure_configs_workspace(cx);
        let has_inputs = workspace.read(cx).has_inputs();
        if has_inputs {
            return;
        }
        workspace.update(cx, |workspace, cx| {
            workspace.ensure_inputs(window, cx);
        });
    }

    pub(super) fn sync_draft_from_values(
        &mut self,
        name: SharedString,
        text: SharedString,
        cx: &mut Context<Self>,
    ) {
        let workspace = self.ensure_configs_workspace(cx);
        workspace.update(cx, |workspace, cx| {
            if workspace.sync_draft_from_values(name, text, self.runtime.running_id) {
                cx.notify();
            }
        });
        self.refresh_configs_workspace_row_flags(cx);
    }
}

impl ConfigsWorkspace {
    fn schedule_draft_validation(&mut self, cx: &mut Context<Self>) {
        if self.draft.text.as_ref().trim().is_empty() || self.operation.is_some() {
            return;
        }

        self.validation_generation = self.validation_generation.wrapping_add(1);
        let generation = self.validation_generation;
        cx.spawn(async move |view, cx| {
            cx.background_executor()
                .timer(DRAFT_VALIDATION_DEBOUNCE)
                .await;
            let _ = view.update(cx, |this, cx| {
                if this.validation_generation != generation || this.operation.is_some() {
                    return;
                }
                let running_id = this.app.read(cx).runtime.running_id;
                let running_name = this.app.read(cx).runtime.running_name.clone();
                this.apply_draft_validation(running_id);
                this.refresh_library_row_flags(running_id, running_name.as_deref());
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn ensure_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.library_search_input.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search configs"));
            let subscription = cx.subscribe(
                &input,
                |_, _, event: &InputEvent, cx: &mut Context<Self>| {
                    if matches!(event, InputEvent::Change) {
                        cx.notify();
                    }
                },
            );
            self.library_search_input = Some(input);
            self.library_search_subscription = Some(subscription);
        }

        if self.name_input.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Config title"));
            let subscription = cx.subscribe(
                &input,
                |this, _, event: &InputEvent, cx: &mut Context<Self>| match event {
                    InputEvent::Change => {
                        let Some(name_input) = this.name_input.as_ref() else {
                            return;
                        };
                        let Some(config_input) = this.config_input.as_ref() else {
                            return;
                        };
                        let name = name_input.read(cx).value();
                        let text = config_input.read(cx).value();
                        let running_id = this.app.read(cx).runtime.running_id;
                        let has_selection = this.app.read(cx).selection.selected_id.is_some();
                        this.sync_draft_from_values(name, text, running_id);
                        this.has_selection = has_selection;
                        this.schedule_draft_validation(cx);
                        cx.notify();
                    }
                    InputEvent::Focus => {
                        if this.set_title_editing(true) {
                            cx.notify();
                        }
                    }
                    InputEvent::Blur | InputEvent::PressEnter { .. } => {
                        if this.set_title_editing(false) {
                            cx.notify();
                        }
                    }
                },
            );
            self.name_input = Some(input);
            self.name_input_subscription = Some(subscription);
        }

        if self.config_input.is_none() {
            let placeholder = "[Interface]\nPrivateKey = ...\nAddress = 10.0.0.2/32\n\n[Peer]\nPublicKey = ...\nAllowedIPs = 0.0.0.0/0\nEndpoint = example.com:51820";
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .code_editor("toml")
                    .line_number(true)
                    .searchable(true)
                    .soft_wrap(false)
                    .tab_size(TabSize {
                        hard_tabs: false,
                        tab_size: 4,
                    })
                    .rows(16)
                    .placeholder(placeholder)
            });
            let subscription = cx.subscribe(
                &input,
                |this, _, event: &InputEvent, cx: &mut Context<Self>| {
                    if matches!(event, InputEvent::Change) {
                        let Some(name_input) = this.name_input.as_ref() else {
                            return;
                        };
                        let Some(config_input) = this.config_input.as_ref() else {
                            return;
                        };
                        let name = name_input.read(cx).value();
                        let text = config_input.read(cx).value();
                        let running_id = this.app.read(cx).runtime.running_id;
                        let has_selection = this.app.read(cx).selection.selected_id.is_some();
                        this.sync_draft_from_values(name, text, running_id);
                        this.has_selection = has_selection;
                        this.schedule_draft_validation(cx);
                        cx.notify();
                    }
                },
            );
            self.config_input = Some(input);
            self.config_input_subscription = Some(subscription);
        }
    }
}
