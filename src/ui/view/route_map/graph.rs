use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    group_box::{GroupBox, GroupBoxVariants},
    h_flex,
    scroll::ScrollableElement as _,
    v_flex, ActiveTheme as _, Icon, StyledExt as _,
};

use crate::ui::state::{RouteMapMode, WgApp};

use super::data::{RouteMapData, RouteMapGraphStep, RouteMapRouteRow};
use super::{empty_group, status_chip};

pub(super) fn render_graph(
    model: &RouteMapData,
    mode: RouteMapMode,
    cx: &mut Context<WgApp>,
) -> Div {
    match mode {
        RouteMapMode::Flow => render_flow(model, cx),
        RouteMapMode::Routes => render_routes(model, cx),
        RouteMapMode::Explain => super::explain::render_explain(model, cx),
        RouteMapMode::Events => super::events::render_events(model, cx),
    }
}

fn render_flow(model: &RouteMapData, cx: &mut Context<WgApp>) -> Div {
    let Some(selected) = model.selected_item.as_ref() else {
        return div().child(empty_group(
            "Decision Graph",
            "Select an inventory item to inspect its decision path.",
            cx,
        ));
    };

    let steps =
        selected
            .graph_steps
            .iter()
            .enumerate()
            .fold(v_flex().gap_2(), |list, (index, step)| {
                let list = list.child(render_flow_step(step, cx));
                if index + 1 < selected.graph_steps.len() {
                    list.child(div().ml_6().w(px(1.0)).h(px(18.0)).bg(cx.theme().border))
                } else {
                    list
                }
            });

    div()
        .flex()
        .flex_col()
        .flex_1()
        .w_full()
        .h_full()
        .min_h(px(0.0))
        .gap_3()
        .child(
            GroupBox::new()
                .fill()
                .flex_grow()
                .title("Decision Graph")
                .child(
                    v_flex()
                        .gap_3()
                        .w_full()
                        .flex_grow()
                        .min_h(px(0.0))
                        .child(
                            h_flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_semibold()
                                        .child(selected.title.clone()),
                                )
                                .child(status_chip(selected.status)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(selected.subtitle.clone()),
                        )
                        .child(steps.w_full().flex_grow()),
                ),
        )
}

fn render_flow_step(step: &RouteMapGraphStep, cx: &mut Context<WgApp>) -> Div {
    div()
        .flex()
        .flex_row()
        .items_start()
        .w_full()
        .gap_3()
        .child(
            div()
                .size(px(28.0))
                .rounded_full()
                .bg(cx.theme().accent.alpha(0.12))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Icon::new(step.icon.clone())
                        .size_4()
                        .text_color(cx.theme().accent),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .p_3()
                .w_full()
                .flex_1()
                .rounded_lg()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().group_box)
                .child(
                    div()
                        .text_xs()
                        .font_semibold()
                        .text_color(cx.theme().muted_foreground)
                        .child(step.label.clone()),
                )
                .child(div().text_sm().font_semibold().child(step.value.clone()))
                .when_some(step.note.as_ref(), |this, note| {
                    this.child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(note.clone()),
                    )
                }),
        )
}

fn render_routes(model: &RouteMapData, cx: &mut Context<WgApp>) -> Div {
    if model.route_rows.is_empty() {
        return div().child(empty_group(
            "Routes",
            "No visible planned routes for the current filters.",
            cx,
        ));
    }

    let header = route_row_header(cx);
    let rows = model.route_rows.iter().fold(v_flex().gap_1(), |list, row| {
        list.child(render_route_row(row, cx))
    });

    div()
        .flex()
        .flex_col()
        .flex_1()
        .w_full()
        .h_full()
        .min_h(px(0.0))
        .child(
            GroupBox::new().fill().flex_grow().title("Routes").child(
                v_flex()
                    .gap_2()
                    .flex_grow()
                    .min_h(px(0.0))
                    .child(header)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .w_full()
                            .min_h(px(0.0))
                            .gap_1()
                            .overflow_y_scrollbar()
                            .child(rows),
                    ),
            ),
        )
}

fn route_row_header(cx: &mut Context<WgApp>) -> Div {
    div()
        .grid()
        .grid_cols(8)
        .gap_2()
        .px_3()
        .pb_1()
        .border_b_1()
        .border_color(cx.theme().border.alpha(0.6))
        .text_xs()
        .font_semibold()
        .text_color(cx.theme().muted_foreground)
        .child("Destination")
        .child("Family")
        .child("Kind")
        .child("Peer")
        .child("Endpoint")
        .child("Table")
        .child("Status")
        .child("Note")
}

fn render_route_row(row: &RouteMapRouteRow, cx: &mut Context<WgApp>) -> Div {
    div()
        .grid()
        .grid_cols(8)
        .w_full()
        .gap_2()
        .p_3()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().group_box)
        .text_xs()
        .child(row.destination.clone())
        .child(row.family.clone())
        .child(row.kind.clone())
        .child(row.peer.clone())
        .child(row.endpoint.clone())
        .child(row.table.clone())
        .child(row.status.clone())
        .child(row.note.clone())
}
