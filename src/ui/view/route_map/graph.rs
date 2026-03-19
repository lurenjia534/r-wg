use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::AnimationExt as _;
use gpui::{uniform_list, *};
use gpui_component::{
    group_box::{GroupBox, GroupBoxVariants},
    h_flex,
    scroll::Scrollbar,
    theme::ThemeMode,
    v_flex, ActiveTheme as _, Icon, StyledExt as _,
};

use crate::ui::state::{RouteMapMode, WgApp};

use super::data::{
    RouteMapData, RouteMapGraphStep, RouteMapGraphStepKind, RouteMapInventoryItem, RouteMapRouteRow,
};
use super::{empty_group, status_chip};

const ROUTE_LIST_SCROLL_STATE_ID: &str = "route-map-routes-scroll";
const ROUTE_ROW_HEIGHT: f32 = 48.0;

#[derive(Clone, Copy)]
struct FlowStepPalette {
    icon_foreground: Hsla,
    icon_background: Hsla,
    icon_border: Hsla,
    card_border: Hsla,
    connector_base: Hsla,
    connector_glow: Hsla,
}

pub(super) fn render_graph(
    model: &RouteMapData,
    mode: RouteMapMode,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) -> Div {
    match mode {
        RouteMapMode::Flow => render_flow(model, window, cx),
        RouteMapMode::Routes => render_routes(model, window, cx),
        RouteMapMode::Explain => super::explain::render_explain(model, cx),
        RouteMapMode::Events => super::events::render_events(model, window, cx),
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

    let steps = if window.viewport_size().width >= px(1500.0) {
        render_horizontal_flow_steps(&selected.graph_steps, cx).into_any_element()
    } else {
        render_vertical_flow_steps(&selected.graph_steps, cx).into_any_element()
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
                .flex_grow()
                .title("Decision Path")
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
                        .child(steps),
                ),
        )
}

fn render_vertical_flow_steps(steps: &[RouteMapGraphStep], cx: &mut Context<WgApp>) -> Div {
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
                            step,
                            cx,
                        )),
                )
            } else {
                list
            }
        })
        .w_full()
        .flex_grow()
}

fn render_horizontal_flow_steps(steps: &[RouteMapGraphStep], cx: &mut Context<WgApp>) -> Div {
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

fn animated_connector(
    animation_id: impl Into<ElementId>,
    vertical: bool,
    step: &RouteMapGraphStep,
    cx: &mut Context<WgApp>,
) -> Div {
    let palette = step_kind_palette(step.kind, cx);

    div()
        .relative()
        .w_full()
        .h_full()
        .bg(palette.connector_base)
        .child(
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
                        let pulse = 0.18 + (0.42 * (1.0 - ((delta as f32 * 2.0) - 1.0).abs()));
                        this.opacity(pulse)
                            .when(vertical, |this| this.rounded_full())
                            .when(!vertical, |this| this.rounded_full())
                    },
                ),
        )
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
        .text_xs()
        .child(route_value_cell(
            row.destination.clone(),
            Some(px(176.0)),
            true,
            cx,
        ))
        .child(route_value_cell(
            row.family.clone(),
            Some(px(64.0)),
            false,
            cx,
        ))
        .child(route_value_cell(
            row.kind.clone(),
            Some(px(112.0)),
            false,
            cx,
        ))
        .child(route_value_cell(
            row.peer.clone(),
            Some(px(84.0)),
            false,
            cx,
        ))
        .child(route_value_cell(
            row.endpoint.clone(),
            Some(px(168.0)),
            true,
            cx,
        ))
        .child(route_value_cell(
            row.table.clone(),
            Some(px(104.0)),
            true,
            cx,
        ))
        .child(route_value_cell(
            row.status.clone(),
            Some(px(72.0)),
            false,
            cx,
        ))
        .child(route_value_cell(row.note.clone(), None, false, cx))
}

fn route_value_cell(value: SharedString, width: Option<Pixels>, mono: bool, cx: &mut App) -> Div {
    let cell = div().text_xs().truncate().child(value);
    let cell = if mono {
        cell.font_family(cx.theme().mono_font_family.clone())
    } else {
        cell
    };
    if let Some(width) = width {
        cell.w(width)
    } else {
        cell.flex_1().min_w(px(0.0))
    }
}

fn selected_summary(selected: &RouteMapInventoryItem) -> SharedString {
    let mut parts = selected
        .chips
        .iter()
        .take(3)
        .map(|chip| chip.label.to_string())
        .collect::<Vec<_>>();

    if let Some(route_row) = selected.route_row.as_ref() {
        parts.push(route_row.table.to_string());
    }

    let has_evidence = selected
        .inspector
        .runtime_evidence
        .iter()
        .all(|entry| entry.as_ref() != "No matching net event captured yet.");
    parts.push(if has_evidence {
        format!("evidence {}", selected.inspector.runtime_evidence.len())
    } else {
        "evidence missing".to_string()
    });

    parts.join(" · ").into()
}

fn step_kind_palette(kind: RouteMapGraphStepKind, cx: &mut Context<WgApp>) -> FlowStepPalette {
    let is_light = cx.theme().mode == ThemeMode::Light;

    let connector_strength = if is_light { 0.5 } else { 0.34 };
    let connector_glow = if is_light { 0.82 } else { 0.62 };
    let neutral_border = if is_light {
        cx.theme().list_active_border
    } else {
        cx.theme().border.alpha(0.92)
    };

    match kind {
        RouteMapGraphStepKind::Interface => FlowStepPalette {
            icon_foreground: cx.theme().accent_foreground,
            icon_background: cx.theme().accent,
            icon_border: cx.theme().accent.alpha(if is_light { 0.92 } else { 0.72 }),
            card_border: cx.theme().accent.alpha(if is_light { 0.56 } else { 0.38 }),
            connector_base: cx.theme().accent.alpha(connector_strength),
            connector_glow: cx.theme().accent.alpha(connector_glow),
        },
        RouteMapGraphStepKind::Dns => FlowStepPalette {
            icon_foreground: cx.theme().info_foreground,
            icon_background: cx.theme().info,
            icon_border: cx.theme().info.alpha(if is_light { 0.94 } else { 0.74 }),
            card_border: cx.theme().info.alpha(if is_light { 0.6 } else { 0.42 }),
            connector_base: cx.theme().info.alpha(connector_strength),
            connector_glow: cx.theme().info.alpha(connector_glow),
        },
        RouteMapGraphStepKind::Policy => FlowStepPalette {
            icon_foreground: cx.theme().primary_foreground,
            icon_background: cx.theme().blue,
            icon_border: cx.theme().blue.alpha(if is_light { 0.9 } else { 0.7 }),
            card_border: cx.theme().blue.alpha(if is_light { 0.58 } else { 0.4 }),
            connector_base: cx.theme().blue.alpha(connector_strength),
            connector_glow: cx.theme().blue.alpha(connector_glow),
        },
        RouteMapGraphStepKind::Peer | RouteMapGraphStepKind::Endpoint => FlowStepPalette {
            icon_foreground: cx.theme().secondary_foreground,
            icon_background: cx.theme().secondary,
            icon_border: neutral_border,
            card_border: neutral_border,
            connector_base: cx.theme().secondary_foreground.alpha(if is_light {
                0.34
            } else {
                0.24
            }),
            connector_glow: cx.theme().secondary_foreground.alpha(if is_light {
                0.6
            } else {
                0.44
            }),
        },
        RouteMapGraphStepKind::Guardrail => FlowStepPalette {
            icon_foreground: cx.theme().warning_foreground,
            icon_background: cx.theme().warning,
            icon_border: cx.theme().warning.alpha(if is_light { 0.96 } else { 0.78 }),
            card_border: cx.theme().warning.alpha(if is_light { 0.62 } else { 0.44 }),
            connector_base: cx.theme().warning.alpha(connector_strength),
            connector_glow: cx.theme().warning.alpha(connector_glow),
        },
        RouteMapGraphStepKind::Destination => FlowStepPalette {
            icon_foreground: cx.theme().secondary_foreground,
            icon_background: cx.theme().muted,
            icon_border: neutral_border,
            card_border: neutral_border,
            connector_base: cx.theme().secondary_foreground.alpha(if is_light {
                0.3
            } else {
                0.22
            }),
            connector_glow: cx.theme().secondary_foreground.alpha(if is_light {
                0.52
            } else {
                0.38
            }),
        },
    }
}
