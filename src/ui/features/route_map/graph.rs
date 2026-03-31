use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::AnimationExt as _;
use gpui::{uniform_list, *};
use gpui_component::{
    group_box::{GroupBox, GroupBoxVariants},
    h_flex,
    scroll::ScrollableElement as _,
    scroll::Scrollbar,
    theme::ThemeMode,
    v_flex, ActiveTheme as _, Icon, StyledExt as _,
};

use crate::ui::state::{RouteMapMode, WgApp};
use crate::ui::view::route_map::data::{
    RouteMapData, RouteMapGraphStep, RouteMapGraphStepKind, RouteMapInventoryItem, RouteMapRouteRow,
};
use crate::ui::view::route_map::{empty_group, explain, status_chip};

use super::events;

const ROUTE_LIST_SCROLL_STATE_ID: &str = "route-map-routes-scroll";
const ROUTE_ROW_HEIGHT: f32 = 48.0;
const FLOW_HORIZONTAL_BREAKPOINT: f32 = 1500.0;
const FLOW_HORIZONTAL_MAX_WIDTH: f32 = 1120.0;
const FLOW_VERTICAL_MAX_WIDTH: f32 = 820.0;

#[derive(Clone, Copy)]
struct FlowStepPalette {
    icon_foreground: Hsla,
    icon_background: Hsla,
    icon_border: Hsla,
    card_border: Hsla,
    connector_base: Hsla,
    connector_glow: Hsla,
}

pub(crate) fn render_graph(
    model: &RouteMapData,
    mode: RouteMapMode,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) -> Div {
    match mode {
        RouteMapMode::Flow => render_flow(model, window, cx),
        RouteMapMode::Routes => render_routes(model, window, cx),
        RouteMapMode::Explain => explain::render_explain(model, cx),
        RouteMapMode::Events => events::render_events(model, window, cx),
    }
}

fn render_flow(model: &RouteMapData, window: &mut Window, cx: &mut Context<WgApp>) -> Div {
    let Some(selected) = model.selected_item.as_ref() else {
        return div().child(empty_group(
            "Decision Path",
            "Select an inventory item to inspect its decision path.",
            cx,
        ));
    };
    let content_style = StyleRefinement::default().flex_1().min_h_0();

    let animate_connectors = window.is_window_active();
    let horizontal_flow = window.viewport_size().width >= px(FLOW_HORIZONTAL_BREAKPOINT);
    let steps = if horizontal_flow {
        render_horizontal_flow_steps(&selected.graph_steps, animate_connectors, cx)
            .into_any_element()
    } else {
        render_vertical_flow_steps(&selected.graph_steps, animate_connectors, cx).into_any_element()
    };
    let flow_content = if horizontal_flow {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .w_full()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .max_w(px(FLOW_HORIZONTAL_MAX_WIDTH))
                    .flex_col()
                    .gap_4()
                    .child(steps)
                    .child(render_flow_band(selected, cx)),
            )
    } else {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .w_full()
            .overflow_hidden()
            .child(
                div()
                    .w_full()
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_y_scrollbar()
                    .child(
                        h_flex().w_full().justify_center().child(
                            div()
                                .w_full()
                                .max_w(px(FLOW_VERTICAL_MAX_WIDTH))
                                .flex_col()
                                .gap_4()
                                .pr_1()
                                .child(steps)
                                .child(render_flow_band(selected, cx)),
                        ),
                    ),
            )
    };

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
                .flex_1()
                .min_h_0()
                .content_style(content_style)
                .title("Decision Path")
                .child(
                    v_flex()
                        .gap_3()
                        .w_full()
                        .flex_1()
                        .min_h(px(0.0))
                        .overflow_hidden()
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
                        .when(
                            model.explain_match_id.as_ref() == Some(&selected.id),
                            |this| {
                                this.child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().info)
                                        .child("Explain hit highlighted in inventory."),
                                )
                            },
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(selected.subtitle.clone()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(selected_summary(selected)),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_h(px(0.0))
                                .w_full()
                                .child(flow_content),
                        ),
                ),
        )
}

fn render_vertical_flow_steps(
    steps: &[RouteMapGraphStep],
    animate_connectors: bool,
    cx: &mut Context<WgApp>,
) -> Div {
    steps
        .iter()
        .enumerate()
        .fold(v_flex().gap_2(), |list, (index, step)| {
            let list = list.child(render_flow_step(step, cx));
            if index + 1 < steps.len() {
                list.child(
                    div()
                        .ml_6()
                        .w(px(1.0))
                        .h(px(18.0))
                        .child(animated_connector(
                            ("route-map-flow-v", index),
                            true,
                            animate_connectors,
                            step,
                            cx,
                        )),
                )
            } else {
                list
            }
        })
        .w_full()
}

fn render_horizontal_flow_steps(
    steps: &[RouteMapGraphStep],
    animate_connectors: bool,
    cx: &mut Context<WgApp>,
) -> Div {
    steps
        .iter()
        .enumerate()
        .fold(
            h_flex().items_start().gap_0().w_full(),
            |row, (index, step)| {
                let row = row.child(render_flow_card(step, cx));
                if index + 1 < steps.len() {
                    row.child(
                        div().flex_1().h_full().min_w(px(28.0)).px_2().child(
                            div()
                                .mt(px(38.0))
                                .h(px(1.0))
                                .w_full()
                                .child(animated_connector(
                                    ("route-map-flow-h", index),
                                    false,
                                    animate_connectors,
                                    step,
                                    cx,
                                )),
                        ),
                    )
                } else {
                    row
                }
            },
        )
        .min_h(px(0.0))
}

fn render_flow_band(selected: &RouteMapInventoryItem, cx: &mut Context<WgApp>) -> Div {
    let facts = flow_band_text(
        "Facts",
        selected_summary(selected).to_string(),
        cx.theme().accent,
        cx.theme().accent.alpha(0.18),
        cx.theme().accent.alpha(0.36),
        cx,
    );
    let evidence = flow_band_text(
        "Evidence",
        if selected.inspector.runtime_evidence.is_empty() {
            "No runtime evidence captured yet.".to_string()
        } else {
            selected
                .inspector
                .runtime_evidence
                .iter()
                .take(2)
                .map(|item| item.as_ref())
                .collect::<Vec<_>>()
                .join(" · ")
        },
        cx.theme().info,
        cx.theme().info.alpha(0.16),
        cx.theme().info.alpha(0.34),
        cx,
    );
    let risk = flow_band_text(
        "Risk",
        if selected.inspector.risk_assessment.is_empty() {
            "No additional risk notes.".to_string()
        } else {
            selected
                .inspector
                .risk_assessment
                .iter()
                .take(2)
                .map(|item| item.as_ref())
                .collect::<Vec<_>>()
                .join(" · ")
        },
        cx.theme().warning,
        cx.theme().warning.alpha(0.12),
        cx.theme().warning.alpha(0.34),
        cx,
    );

    div()
        .flex()
        .flex_wrap()
        .gap_2()
        .w_full()
        .child(facts)
        .child(evidence)
        .child(risk)
}

fn flow_band_text(
    label: &str,
    body: String,
    surface: Hsla,
    fill: Hsla,
    border: Hsla,
    cx: &mut Context<WgApp>,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .flex_1()
        .min_w(px(220.0))
        .rounded_lg()
        .border_1()
        .border_color(border)
        .bg(fill)
        .px_3()
        .py_2()
        .child(
            h_flex()
                .items_center()
                .gap_2()
                .child(div().size(px(7.0)).rounded_full().bg(surface.alpha(0.88)))
                .child(
                    div()
                        .text_xs()
                        .font_semibold()
                        .text_color(surface)
                        .child(label.to_string()),
                ),
        )
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().foreground)
                .child(body),
        )
}

fn animated_connector(
    animation_id: impl Into<ElementId>,
    vertical: bool,
    animate: bool,
    step: &RouteMapGraphStep,
    cx: &mut Context<WgApp>,
) -> Div {
    let palette = step_kind_palette(step.kind, cx);
    div()
        .relative()
        .w_full()
        .h_full()
        .bg(palette.connector_base)
        .child(if animate {
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .bg(palette.connector_glow)
                .with_animation(
                    animation_id,
                    Animation::new(Duration::from_secs(2)).repeat(),
                    move |this, delta| {
                        let pulse = 0.18 + (0.42 * (1.0 - ((delta * 2.0) - 1.0).abs()));
                        this.opacity(pulse)
                            .when(vertical, |this| this.rounded_full())
                            .when(!vertical, |this| this.rounded_full())
                    },
                )
                .into_any_element()
        } else {
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .bg(palette.connector_glow)
                .opacity(0.18)
                .when(vertical, |this| this.rounded_full())
                .when(!vertical, |this| this.rounded_full())
                .into_any_element()
        })
}

fn render_flow_step(step: &RouteMapGraphStep, cx: &mut Context<WgApp>) -> Div {
    let palette = step_kind_palette(step.kind, cx);

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
                .border_1()
                .border_color(palette.icon_border)
                .bg(palette.icon_background)
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Icon::new(step.icon.clone())
                        .size_4()
                        .text_color(palette.icon_foreground),
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
                .border_color(palette.card_border)
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

fn render_flow_card(step: &RouteMapGraphStep, cx: &mut Context<WgApp>) -> Div {
    let palette = step_kind_palette(step.kind, cx);

    div()
        .flex()
        .flex_col()
        .gap_2()
        .w(px(164.0))
        .min_w(px(164.0))
        .child(
            div()
                .size(px(32.0))
                .rounded_full()
                .border_1()
                .border_color(palette.icon_border)
                .bg(palette.icon_background)
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Icon::new(step.icon.clone())
                        .size_4()
                        .text_color(palette.icon_foreground),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .p_3()
                .rounded_lg()
                .border_1()
                .border_color(palette.card_border)
                .bg(cx.theme().group_box)
                .child(
                    div()
                        .text_xs()
                        .font_semibold()
                        .text_color(cx.theme().muted_foreground)
                        .child(step.label.clone()),
                )
                .child(
                    div()
                        .text_sm()
                        .font_semibold()
                        .truncate()
                        .child(step.value.clone()),
                )
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

fn render_routes(model: &RouteMapData, window: &mut Window, cx: &mut Context<WgApp>) -> Div {
    if model.route_rows.is_empty() {
        return div().child(empty_group(
            "Routes",
            "No visible planned routes for the current filters.",
            cx,
        ));
    }

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
    .track_scroll(scroll_handle.clone())
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
                            .relative()
                            .child(list)
                            .child(Scrollbar::vertical(&scroll_handle)),
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

fn selected_summary(selected: &RouteMapInventoryItem) -> SharedString {
    if let Some(route_row) = selected.route_row.as_ref() {
        format!(
            "{} via {} ({})",
            route_row.destination, route_row.peer, route_row.status
        )
        .into()
    } else {
        selected.inspector.why_match.join(" · ").into()
    }
}

fn step_kind_palette(kind: RouteMapGraphStepKind, cx: &mut Context<WgApp>) -> FlowStepPalette {
    let mode = cx.theme().mode;
    let accent = match kind {
        RouteMapGraphStepKind::Interface => cx.theme().accent,
        RouteMapGraphStepKind::Dns => cx.theme().info,
        RouteMapGraphStepKind::Policy => cx.theme().warning,
        RouteMapGraphStepKind::Peer => cx.theme().success,
        RouteMapGraphStepKind::Endpoint => cx.theme().warning,
        RouteMapGraphStepKind::Guardrail => cx.theme().danger,
        RouteMapGraphStepKind::Destination => cx.theme().accent,
    };
    let neutral = cx.theme().border;

    if mode == ThemeMode::Dark {
        FlowStepPalette {
            icon_foreground: accent.alpha(0.96),
            icon_background: accent.alpha(0.16),
            icon_border: accent.alpha(0.38),
            card_border: neutral.alpha(0.72),
            connector_base: neutral.alpha(0.42),
            connector_glow: accent.alpha(0.62),
        }
    } else {
        FlowStepPalette {
            icon_foreground: accent.alpha(0.92),
            icon_background: accent.alpha(0.12),
            icon_border: accent.alpha(0.28),
            card_border: neutral.alpha(0.92),
            connector_base: neutral.alpha(0.34),
            connector_glow: accent.alpha(0.48),
        }
    }
}
