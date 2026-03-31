use super::*;
use crate::ui::features::configs::dialogs;

impl WgApp {
    pub(crate) fn request_sidebar_active(
        &mut self,
        item: SidebarItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        dialogs::request_sidebar_active(self, item, window, cx);
    }

    pub(crate) fn handle_new_draft_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        dialogs::handle_new_draft_click(self, window, cx);
    }
}
