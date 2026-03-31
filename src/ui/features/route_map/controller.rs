use gpui::{AppContext, Context, Window};
use gpui_component::input::InputState;

use crate::ui::state::WgApp;

pub(crate) fn ensure_route_map_search_input(
    app: &mut WgApp,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) {
    if app.ui.route_map_search_input.is_some() {
        return;
    }

    let input =
        cx.new(|cx| InputState::new(window, cx).placeholder("Search IP, CIDR, or endpoint host"));
    app.ui.route_map_search_input = Some(input);
}
