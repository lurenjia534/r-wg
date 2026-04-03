use gpui::{Context, Window};

use crate::ui::state::WgApp;

impl WgApp {
    pub(crate) fn command_toggle_tunnel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        crate::ui::features::session::controller::handle_start_stop(self, window, cx);
    }
}
