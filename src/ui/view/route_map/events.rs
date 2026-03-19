use gpui::*;
use gpui_component::{
    group_box::{GroupBox, GroupBoxVariants},
    scroll::ScrollableElement as _,
    v_flex, ActiveTheme as _,
};

use crate::ui::state::WgApp;

use super::data::RouteMapData;
use super::empty_group;

pub(super) fn render_events(model: &RouteMapData, cx: &mut Context<WgApp>) -> Div {
    if model.net_events.is_empty() {
        return div().child(empty_group(
            "Events",
            "No `net` events captured yet for the current session.",
            cx,
        ));
    }

    let events = model
        .net_events
        .iter()
        .fold(v_flex().gap_2(), |list, event| {
            list.child(
                div()
                    .p_3()
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().group_box)
                    .text_xs()
                    .font_family(cx.theme().mono_font_family.clone())
                    .child(event.clone()),
            )
        });

    div().flex().flex_col().h_full().min_h(px(0.0)).child(
        GroupBox::new().fill().title("Events").child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .overflow_y_scrollbar()
                .child(events),
        ),
    )
}
