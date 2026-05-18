use gpui::prelude::FluentBuilder as _;
use gpui::{uniform_list, *};
use gpui_component::{
    group_box::{GroupBox, GroupBoxVariants},
    h_flex,
    scroll::ScrollableElement as _,
    scroll::Scrollbar,
    tag::Tag,
    v_flex, ActiveTheme as _, Sizable as _, StyledExt as _,
};

use crate::ui::state::WgApp;

use super::data::{RouteMapData, RouteMapRouteRow};
use super::empty_group;

const ROUTE_LIST_SCROLL_STATE_ID: &str = "route-map-routes-scroll";
const ROUTE_ROW_HEIGHT: f32 = 48.0;
const ROUTE_CARD_LAYOUT_BREAKPOINT: f32 = 1500.0;

pub(super) fn render_routes(
    model: &RouteMapData,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) -> Div {
    if model.route_rows.is_empty() {
        return div().child(empty_group(
            "Routes",
            "No visible planned routes for the current filters.",
            cx,
        ));
    }

    if window.viewport_size().width < px(ROUTE_CARD_LAYOUT_BREAKPOINT) {
        return render_route_cards(model, cx);
    }

    let content_style = StyleRefinement::default().flex_1().min_h_0();
    let rows = model.route_rows.clone();
    let scroll_handle = window
        .use_keyed_state(ROUTE_LIST_SCROLL_STATE_ID, cx, |_, _| {
            UniformListScrollHandle::new()
        })
        .read(cx)
        .clone();
    let header = route_row_header(cx);
    let list = uniform_list(
        "route-map-routes-list",
        rows.len(),
        move |visible_range, _window, cx| {
            visible_range
                .map(|ix| render_route_row(&rows[ix], cx))
                .collect::<Vec<_>>()
        },
    )
    .track_scroll(&scroll_handle)
    .w_full()
    .flex_1();

    div()
        .flex()
        .flex_col()
        .flex_1()
        .w_full()
        .h_full()
        .min_h(px(0.0))
        .child(
            GroupBox::new()
                .fill()
                .flex_1()
                .min_h_0()
                .content_style(content_style)
                .title("Routes")
                .child(
                    v_flex()
                        .gap_2()
                        .w_full()
                        .flex_1()
                        .min_h(px(0.0))
                        .child(header)
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .h_full()
                                .w_full()
                                .min_h(px(0.0))
                                .overflow_hidden()
                                .relative()
                                .child(list)
                                .child(Scrollbar::vertical(&scroll_handle)),
                        ),
                ),
        )
}

fn render_route_cards(model: &RouteMapData, cx: &mut Context<WgApp>) -> Div {
    let content_style = StyleRefinement::default().flex_1().min_h_0();

    div()
        .flex()
        .flex_col()
        .flex_1()
        .w_full()
        .h_full()
        .min_h(px(0.0))
        .child(
            GroupBox::new()
                .fill()
                .flex_1()
                .min_h_0()
                .content_style(content_style)
                .title("Routes")
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .w_full()
                        .min_h(px(0.0))
                        .overflow_hidden()
                        .child(
                            div()
                                .w_full()
                                .flex_1()
                                .min_h_0()
                                .overflow_y_scrollbar()
                                .child(
                                    v_flex().w_full().gap_2().children(
                                        model
                                            .route_rows
                                            .iter()
                                            .map(|row| render_route_card(row, cx)),
                                    ),
                                ),
                        ),
                ),
        )
}

fn route_row_header(cx: &mut Context<WgApp>) -> Div {
    div()
        .flex()
        .items_center()
        .gap_3()
        .px_3()
        .pb_1()
        .border_b_1()
        .border_color(cx.theme().border.alpha(0.6))
        .text_xs()
        .font_semibold()
        .text_color(cx.theme().muted_foreground)
        .child(route_header_cell("Destination", Some(px(176.0))))
        .child(route_header_cell("Family", Some(px(64.0))))
        .child(route_header_cell("Kind", Some(px(112.0))))
        .child(route_header_cell("Peer", Some(px(84.0))))
        .child(route_header_cell("Endpoint", Some(px(168.0))))
        .child(route_header_cell("Table", Some(px(104.0))))
        .child(route_header_cell("Status", Some(px(72.0))))
        .child(route_header_cell("Note", None))
}

fn route_header_cell(label: &str, width: Option<Pixels>) -> Div {
    let cell = div()
        .text_xs()
        .font_semibold()
        .truncate()
        .child(label.to_string());
    if let Some(width) = width {
        cell.w(width)
    } else {
        cell.flex_1().min_w(px(0.0))
    }
}

fn render_route_row(row: &RouteMapRouteRow, cx: &mut App) -> Div {
    div()
        .h(px(ROUTE_ROW_HEIGHT))
        .flex()
        .items_center()
        .w_full()
        .gap_3()
        .px_3()
        .border_b_1()
        .border_color(cx.theme().border.alpha(0.42))
        .child(route_cell(
            row.destination.clone(),
            Some(px(176.0)),
            Some(cx.theme().mono_font_family.clone()),
            cx,
        ))
        .child(route_cell(row.family.clone(), Some(px(64.0)), None, cx))
        .child(route_cell(row.kind.clone(), Some(px(112.0)), None, cx))
        .child(route_cell(row.peer.clone(), Some(px(84.0)), None, cx))
        .child(route_cell(
            row.endpoint.clone(),
            Some(px(168.0)),
            Some(cx.theme().mono_font_family.clone()),
            cx,
        ))
        .child(route_cell(row.table.clone(), Some(px(104.0)), None, cx))
        .child(route_cell(row.status.clone(), Some(px(72.0)), None, cx))
        .child(route_cell(row.note.clone(), None, None, cx))
}

fn route_cell(
    value: SharedString,
    width: Option<Pixels>,
    font_family: Option<SharedString>,
    cx: &mut App,
) -> Div {
    let base = div()
        .text_sm()
        .truncate()
        .text_color(cx.theme().foreground)
        .when_some(font_family, |this, family| this.font_family(family));
    let base = if let Some(width) = width {
        base.w(width)
    } else {
        base.flex_1().min_w(px(0.0))
    };
    base.child(value)
}

fn render_route_card(row: &RouteMapRouteRow, cx: &mut Context<WgApp>) -> Div {
    div()
        .w_full()
        .rounded_lg()
        .border_1()
        .border_color(cx.theme().border.alpha(0.72))
        .bg(cx.theme().group_box)
        .p_3()
        .child(
            v_flex()
                .gap_2()
                .child(
                    h_flex()
                        .items_start()
                        .justify_between()
                        .gap_3()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .min_w(px(0.0))
                                .child(
                                    div()
                                        .text_xs()
                                        .font_semibold()
                                        .text_color(cx.theme().muted_foreground)
                                        .child("Destination"),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .font_semibold()
                                        .font_family(cx.theme().mono_font_family.clone())
                                        .child(row.destination.clone()),
                                ),
                        )
                        .child(
                            Tag::secondary()
                                .small()
                                .rounded_full()
                                .child(row.status.clone()),
                        ),
                )
                .child(
                    h_flex()
                        .items_center()
                        .gap_1()
                        .flex_wrap()
                        .child(
                            Tag::secondary()
                                .small()
                                .rounded_full()
                                .child(row.family.clone()),
                        )
                        .child(
                            Tag::secondary()
                                .small()
                                .rounded_full()
                                .child(row.kind.clone()),
                        )
                        .child(
                            Tag::secondary()
                                .small()
                                .rounded_full()
                                .child(row.peer.clone()),
                        )
                        .child(
                            Tag::secondary()
                                .small()
                                .rounded_full()
                                .child(row.table.clone()),
                        ),
                )
                .child(route_card_line("Endpoint", row.endpoint.clone(), true, cx))
                .child(route_card_line("Note", row.note.clone(), false, cx)),
        )
}

fn route_card_line(
    label: &str,
    value: SharedString,
    monospace: bool,
    cx: &mut Context<WgApp>,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_xs()
                .font_semibold()
                .text_color(cx.theme().muted_foreground)
                .child(label.to_string()),
        )
        .child(
            div()
                .text_sm()
                .when(monospace, |this| {
                    this.font_family(cx.theme().mono_font_family.clone())
                })
                .child(value),
        )
}
