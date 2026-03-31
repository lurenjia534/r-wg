use gpui::{Context, Window};

use crate::ui::features::configs::controller;
use crate::ui::state::WgApp;

impl WgApp {
    pub(crate) fn handle_import_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        controller::handle_import_click(self, window, cx);
    }

    pub(crate) fn handle_export_click(&mut self, cx: &mut Context<Self>) {
        controller::handle_export_click(self, cx);
    }
}
