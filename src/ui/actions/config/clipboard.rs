use super::*;

impl WgApp {
    pub(crate) fn handle_paste_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.configs_draft_snapshot(cx).is_dirty() {
            self.confirm_discard_or_save(
                PendingDraftAction::Paste,
                window,
                cx,
                "Replace draft?",
                "Pasting a config will replace the current unsaved draft.",
            );
            return;
        }
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            self.set_error("Clipboard is empty");
            cx.notify();
            return;
        };

        if let Err(err) = config::parse_config(&text) {
            self.set_error(format!("Invalid config: {err}"));
            cx.notify();
            return;
        }
        let text: SharedString = text.into();

        let suggested_name = self
            .configs_name_input(cx)
            .map(|input| input.read(cx).value().to_string())
            .unwrap_or_default();
        let suggested_name = suggested_name.trim();
        let name = if suggested_name.is_empty() {
            self.next_config_name("pasted")
        } else if self.configs.iter().any(|cfg| cfg.name == suggested_name) {
            self.next_config_name(suggested_name)
        } else {
            suggested_name.to_string()
        };

        self.set_selected_config_id(None, cx);
        self.selection.loading_config_id = None;
        self.selection.loading_config_path = None;
        self.selection.loaded_config = None;
        self.set_unsaved_draft(name.clone().into(), text.clone(), cx);
        if let Some(name_input) = self.configs_name_input(cx) {
            name_input.update(cx, |input, cx| {
                input.set_value(name.clone(), window, cx);
            });
        }
        if let Some(config_input) = self.configs_config_input(cx) {
            config_input.update(cx, |input, cx| {
                input.set_value(text, window, cx);
            });
        }
        self.set_status("Pasted config into draft");
        cx.notify();
    }

    pub(crate) fn handle_copy_click(&mut self, cx: &mut Context<Self>) {
        let Some(selected) = self.selected_config().cloned() else {
            self.set_error("Select a tunnel first");
            cx.notify();
            return;
        };
        let cached_text = self.cached_config_text(&selected.storage_path);
        let text = selected.text.clone().or(cached_text);
        if let Some(text) = text {
            cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
            self.set_status("Config copied to clipboard");
            cx.notify();
            return;
        }

        self.set_status("Loading config...");
        cx.notify();

        cx.spawn(async move |view, cx| {
            let path_for_cache = selected.storage_path.clone();
            let read_task =
                cx.background_spawn(async move { std::fs::read_to_string(&selected.storage_path) });
            let result = read_task.await;
            view.update(cx, |this, cx| {
                if this.selection.selected_id != Some(selected.id) {
                    return;
                }
                match result {
                    Ok(text) => {
                        let text: SharedString = text.into();
                        this.cache_config_text(path_for_cache, text.clone());
                        cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
                        this.set_status("Config copied to clipboard");
                    }
                    Err(err) => {
                        this.set_error(format!("Read failed: {err}"));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
