use gpui::prelude::FluentBuilder as _;
use gpui::{uniform_list, *};
use gpui_component::{
    group_box::{GroupBox, GroupBoxVariants},
    scroll::Scrollbar,
    ActiveTheme as _, StyledExt as _,
};

use super::data::RouteMapData;
use super::{empty_group, summary_chip};
use crate::ui::state::WgApp;

const EVENTS_LIST_SCROLL_STATE_ID: &str = "route-map-events-scroll";
const EVENT_ROW_HEIGHT: f32 = 42.0;

pub(crate) fn render_events(
    model: &RouteMapData,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) -> Div {
    if model.net_events.is_empty() {
        return div().child(empty_group(
            "Events",
            "No `net` events captured yet for the current session.",
            cx,
        ));
    }

    let rows = model.net_events.clone();
    let scroll_handle = window
        .use_keyed_state(EVENTS_LIST_SCROLL_STATE_ID, cx, |_, _| {
            UniformListScrollHandle::new()
        })
        .read(cx)
        .clone();
    let list = uniform_list(
        "route-map-events-list",
        rows.len(),
        move |visible_range, _window, cx| {
            visible_range
                .map(|ix| render_event_row(rows[ix].clone(), cx))
                .collect::<Vec<_>>()
        },
    )
    .track_scroll(scroll_handle.clone())
    .with_sizing_behavior(ListSizingBehavior::Auto)
    .w_full()
    .flex_1()
    .size_full();
    let content_style = StyleRefinement::default().flex_1().min_h_0();

    div().flex().flex_col().h_full().min_h(px(0.0)).child(
        GroupBox::new()
            .fill()
            .flex_1()
            .min_h_0()
            .content_style(content_style)
            .title("Events")
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .h_full()
                    .min_h(px(0.0))
                    .overflow_hidden()
                    .relative()
                    .child(list)
                    .child(Scrollbar::vertical(&scroll_handle)),
            ),
    )
}

pub(crate) fn render_events_workspace(
    model: &RouteMapData,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) -> Div {
    let evidence_chip = model
        .summary_chips
        .iter()
        .find(|chip| chip.label.as_ref().starts_with("Evidence "));
    let routes_chip = model
        .summary_chips
        .iter()
        .find(|chip| chip.label.as_ref().starts_with("Routes "));

    div()
        .flex()
        .flex_col()
        .flex_1()
        .w_full()
        .h_full()
        .min_h(px(0.0))
        .gap_3()
        .child(
            GroupBox::new().fill().title("Events Overview").child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .w_full()
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                "Recent `net` evidence stays full-width here so runtime drift is easier to scan against the current plan.",
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .flex_wrap()
                            .when_some(evidence_chip, |this, chip| this.child(summary_chip(chip)))
                            .when_some(routes_chip, |this, chip| this.child(summary_chip(chip))),
                    )
                    .when_some(model.selected_item.as_ref(), |this, selected| {
                        this.child(
                            div()
                                .rounded_lg()
                                .border_1()
                                .border_color(cx.theme().border)
                                .bg(cx.theme().group_box)
                                .p_3()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_semibold()
                                        .text_color(cx.theme().muted_foreground)
                                        .child("CURRENT SELECTION"),
                                )
                                .child(
                                    div()
                                        .pt_1()
                                        .text_sm()
                                        .font_semibold()
                                        .child(selected.title.clone()),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(selected.subtitle.clone()),
                                ),
                        )
                    }),
            ),
        )
        .child(render_events(model, window, cx))
}

fn render_event_row(event: SharedString, cx: &mut App) -> Div {
    div()
        .h(px(EVENT_ROW_HEIGHT))
        .px_3()
        .flex()
        .items_center()
        .border_b_1()
        .border_color(cx.theme().border.alpha(0.42))
        .text_xs()
        .font_family(cx.theme().mono_font_family.clone())
        .truncate()
        .child(event)
}
