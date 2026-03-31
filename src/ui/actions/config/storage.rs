use crate::ui::features::configs::controller;

use super::naming::next_available_name;
use super::*;

impl WgApp {
    pub(crate) fn handle_save_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        controller::handle_save_click(self, window, cx);
    }

    pub(crate) fn handle_save_and_restart_click(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        controller::handle_save_and_restart_click(self, window, cx);
    }

    pub(crate) fn handle_save_as_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        controller::handle_save_as_click(self, window, cx);
    }

    pub(crate) fn handle_rename_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        controller::handle_rename_click(self, window, cx);
    }

    pub(crate) fn handle_delete_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        controller::handle_delete_click(self, window, cx);
    }

    pub(crate) fn delete_configs_blocking_running(
        &mut self,
        ids: &[u64],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        controller::delete_configs_blocking_running(self, ids, window, cx);
    }

    pub(crate) fn delete_configs_skip_running(
        &mut self,
        ids: &[u64],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        controller::delete_configs_skip_running(self, ids, window, cx);
    }

    pub(crate) fn next_config_name(&self, base: &str) -> String {
        next_available_name(self.configs.iter().map(|cfg| cfg.name.as_str()), base)
    }
}
