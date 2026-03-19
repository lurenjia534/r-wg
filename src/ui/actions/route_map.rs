use gpui::{AppContext, Context, Window};
use gpui_component::input::InputState;

use super::super::state::WgApp;

impl WgApp {
    pub(crate) fn ensure_route_map_search_input(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.ui.route_map_search_input.is_some() {
            return;
        }

        let input = cx
            .new(|cx| InputState::new(window, cx).placeholder("Search IP, CIDR, or endpoint host"));
        self.ui.route_map_search_input = Some(input);
    }
}
