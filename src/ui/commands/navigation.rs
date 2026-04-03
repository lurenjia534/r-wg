use gpui::{Context, Window};

use crate::ui::state::{SidebarItem, WgApp};

impl WgApp {
    pub(crate) fn command_open_sidebar_item(
        &mut self,
        item: SidebarItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        crate::ui::features::configs::dialogs::request_sidebar_active(self, item, window, cx);
    }
}
