use gpui::{Context, Window};

use crate::ui::state::WgApp;

impl WgApp {
    pub(crate) fn command_toggle_proxy_select_mode(&mut self, cx: &mut Context<Self>) {
        self.selection.proxy_select_mode = !self.selection.proxy_select_mode;
        if !self.selection.proxy_select_mode {
            self.selection.proxy_selected_ids.clear();
        }
        cx.notify();
    }

    pub(crate) fn command_select_visible_proxies(
        &mut self,
        visible_ids: &[u64],
        cx: &mut Context<Self>,
    ) {
        self.selection.proxy_selected_ids = visible_ids.iter().copied().collect();
        cx.notify();
    }

    pub(crate) fn command_clear_proxy_selection(&mut self, cx: &mut Context<Self>) {
        self.selection.proxy_selected_ids.clear();
        cx.notify();
    }

    pub(crate) fn command_toggle_proxy_multi_selection(
        &mut self,
        config_id: u64,
        cx: &mut Context<Self>,
    ) {
        if self.selection.proxy_selected_ids.contains(&config_id) {
            self.selection.proxy_selected_ids.remove(&config_id);
        } else {
            self.selection.proxy_selected_ids.insert(config_id);
        }
        cx.notify();
    }

    pub(crate) fn command_prompt_delete_selected_proxy(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(config) = self.selected_config().cloned() else {
            self.set_error("Select a tunnel first");
            cx.notify();
            return;
        };
        crate::ui::features::proxies::controller::open_delete_dialog(
            window,
            cx,
            "Delete config?",
            format!("Delete \"{}\"? This cannot be undone.", config.name),
            None,
            vec![config.id],
            false,
            false,
        );
    }

    pub(crate) fn command_prompt_delete_selected_proxies(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selection.proxy_selected_ids.is_empty() {
            self.set_error("Select configs first");
            cx.notify();
            return;
        }
        let ids: Vec<u64> = self.selection.proxy_selected_ids.iter().copied().collect();
        let count = ids.len();
        let body = if count == 1 {
            "Delete 1 selected config? This cannot be undone.".to_string()
        } else {
            format!("Delete {count} selected configs? This cannot be undone.")
        };
        crate::ui::features::proxies::controller::open_delete_dialog(
            window,
            cx,
            "Delete selected configs?",
            body,
            Some("Running tunnels will be skipped.".to_string()),
            ids,
            true,
            true,
        );
    }

    pub(crate) fn command_delete_proxy_configs(
        &mut self,
        ids: &[u64],
        skip_running: bool,
        clear_selection: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if skip_running {
            self.delete_configs_skip_running(ids, window, cx);
        } else {
            self.delete_configs_blocking_running(ids, window, cx);
        }
        if clear_selection {
            self.selection.proxy_select_mode = false;
            self.selection.proxy_selected_ids.clear();
        }
    }
}
