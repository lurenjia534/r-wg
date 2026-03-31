use crate::ui::features::configs::controller;

use super::*;

impl WgApp {
    pub(crate) fn handle_paste_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        controller::handle_paste_click(self, window, cx);
    }

    pub(crate) fn handle_copy_click(&mut self, cx: &mut Context<Self>) {
        controller::handle_copy_click(self, cx);
    }
}
