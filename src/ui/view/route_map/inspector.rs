use gpui::*;
use gpui_component::{
    button::{Button, ButtonCustomVariant, ButtonVariants as _},
    collapsible::Collapsible,
    group_box::{GroupBox, GroupBoxVariants},
    h_flex,
    scroll::ScrollableElement as _,
    v_flex, ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _,
};

use crate::ui::state::WgApp;

use super::data::{RouteMapData, RouteMapGraphStepKind, RouteMapItemStatus};
use super::{empty_group, status_chip, summary_chip};

pub(super) fn render_inspector(app: &WgApp, model: &RouteMapData, cx: &mut Context<WgApp>) -> Div {
    let Some(selected) = model.selected_item.as_ref() else {
        return div().child(empty_group(
            "Inspector",
            "Select a route, guardrail, or policy item to inspect why it exists.",
            cx,
        ));
    };
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
                .title("Inspector")
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
                                    v_flex()
                                        .gap_3()
                                        .w_full()
                                        .child(
                                            v_flex()
                                                .gap_2()
                                                .child(
                                                    div()
                                                        .flex()
                                                        .flex_row()
                                                        .items_center()
                                                        .justify_between()
                                                        .gap_3()
                                                        .child(
                                                            div().text_lg().font_semibold().child(
                                                                selected.inspector.title.clone(),
                                                            ),
                                                        )
                                                        .child(status_chip(selected.status)),
                                                )
                                                .child(
                                                    div()
                                                        .text_sm()
                                                        .text_color(cx.theme().muted_foreground)
                                                        .child(selected.inspector.subtitle.clone()),
                                                )
                                                .child(
                                                    div()
                                                        .flex()
                                                        .flex_row()
                                                        .gap_1()
                                                        .flex_wrap()
                                                        .children(
                                                            selected.chips.iter().map(summary_chip),
                                                        ),
                                                ),
                                        )
                                        .child(
                                            if selected.status == RouteMapItemStatus::Warning
                                                || selected.status == RouteMapItemStatus::Failed
                                            {
                                                v_flex()
                                                    .gap_3()
                                                    .child(render_plain_section(
                                                        "Why It Matches",
                                                        &selected.inspector.why_match,
                                                        cx,
                                                    ))
                                                    .child(render_plain_section(
                                                        "Platform Details",
                                                        &selected.inspector.platform_details,
                                                        cx,
                                                    ))
                                                    .child(render_card_section(
                                                        "Runtime Evidence",
                                                        &selected.inspector.runtime_evidence,
                                                        false,
                                                        cx,
                                                    ))
                                                    .child(render_card_section(
                                                        "Risk Assessment",
                                                        &selected.inspector.risk_assessment,
                                                        true,
                                                        cx,
                                                    ))
                                                    .child(render_glossary_section(
                                                        app.ui_session.route_map_glossary_open,
                                                        &selected.graph_steps,
                                                        cx,
                                                    ))
                                            } else {
                                                v_flex()
                                                    .gap_3()
                                                    .child(render_plain_section(
                                                        "Why It Matches",
                                                        &selected.inspector.why_match,
                                                        cx,
                                                    ))
                                                    .child(render_plain_section(
                                                        "Platform Details",
                                                        &selected.inspector.platform_details,
                                                        cx,
                                                    ))
                                                    .child(render_card_section(
                                                        "Runtime Evidence",
                                                        &selected.inspector.runtime_evidence,
                                                        false,
                                                        cx,
                                                    ))
                                                    .child(render_card_section(
                                                        "Risk Assessment",
                                                        &selected.inspector.risk_assessment,
                                                        false,
                                                        cx,
                                                    ))
                                                    .child(render_glossary_section(
                                                        app.ui_session.route_map_glossary_open,
                                                        &selected.graph_steps,
                                                        cx,
                                                    ))
                                            },
                                        ),
                                ),
                        ),
                ),
        )
}

fn render_plain_section(title: &str, entries: &[SharedString], cx: &mut Context<WgApp>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(section_title(title, cx))
        .child(entries.iter().fold(v_flex().gap_2(), |list, entry| {
            list.child(
                div()
                    .px_1()
                    .text_sm()
                    .text_color(cx.theme().foreground)
                    .child(entry.clone()),
            )
        }))
}

fn render_card_section(
    title: &str,
    entries: &[SharedString],
    emphasize: bool,
    cx: &mut Context<WgApp>,
) -> Div {
    let border = if emphasize {
        cx.theme().warning.alpha(0.32)
    } else {
        cx.theme().border
    };
    let background = if emphasize {
        cx.theme().warning.alpha(0.08)
    } else {
        cx.theme().group_box
    };

    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(section_title(title, cx))
        .child(entries.iter().fold(v_flex().gap_1(), |list, entry| {
            list.child(
                div()
                    .p_3()
                    .rounded_lg()
                    .border_1()
                    .border_color(border)
                    .bg(background)
                    .text_sm()
                    .child(entry.clone()),
            )
        }))
}

fn render_glossary_section(
    open: bool,
    steps: &[super::data::RouteMapGraphStep],
    cx: &mut Context<WgApp>,
) -> Div {
    let mut kinds = Vec::new();
    for step in steps {
        if !kinds.contains(&step.kind) {
            kinds.push(step.kind);
        }
    }

    if kinds.is_empty() {
        return div();
    }

    let header = Button::new("route-map-glossary-toggle")
        .custom(
            ButtonCustomVariant::new(cx)
                .color(cx.theme().group_box.alpha(0.58))
                .foreground(cx.theme().foreground)
                .border(cx.theme().border.alpha(0.6))
                .hover(cx.theme().group_box.alpha(0.82))
                .active(cx.theme().group_box.alpha(0.92)),
        )
        .compact()
        .w_full()
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .gap_3()
                .w_full()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(section_title("Glossary", cx))
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("{} terms", kinds.len())),
                        ),
                )
                .child(
                    Icon::new(if open {
                        IconName::ChevronUp
                    } else {
                        IconName::ChevronDown
                    })
                    .small()
                    .text_color(cx.theme().muted_foreground),
                ),
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            let next_open = !this.ui_session.route_map_glossary_open;
            this.set_route_map_glossary_open(next_open, cx);
        }));

    let content = kinds
        .into_iter()
        .fold(v_flex().gap_2().pt_2(), |list, kind| {
            let (label, body) = glossary_entry(kind);
            list.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border.alpha(0.6))
                    .bg(cx.theme().group_box.alpha(0.5))
                    .p_3()
                    .child(
                        div()
                            .text_xs()
                            .font_semibold()
                            .text_color(cx.theme().muted_foreground)
                            .child(label),
                    )
                    .child(div().text_sm().child(body)),
            )
        });

    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(Collapsible::new().open(open).child(header).content(content))
}

fn glossary_entry(kind: RouteMapGraphStepKind) -> (&'static str, &'static str) {
    match kind {
        RouteMapGraphStepKind::Interface => (
            "Interface",
            "The local WireGuard interface and address family state that anchor the plan.",
        ),
        RouteMapGraphStepKind::Dns => (
            "DNS / Guard",
            "Resolver routes and family guardrails that affect how traffic reaches trusted DNS.",
        ),
        RouteMapGraphStepKind::Policy => (
            "Policy / Table",
            "Routing table, policy rule, metric, or fwmark handling used to steer traffic.",
        ),
        RouteMapGraphStepKind::Peer => (
            "Peer",
            "The peer definition that advertises a prefix or receives routed traffic.",
        ),
        RouteMapGraphStepKind::Endpoint => (
            "Endpoint",
            "The remote peer endpoint host or IP used for tunnel transport.",
        ),
        RouteMapGraphStepKind::Guardrail => (
            "Guardrail",
            "A protective rule that prevents loops, leaks, or unsafe default-route behavior.",
        ),
        RouteMapGraphStepKind::Destination => (
            "Destination",
            "The matched prefix, bypass host route, or concrete target that closes the decision path.",
        ),
    }
}

fn section_title(title: &str, cx: &mut Context<WgApp>) -> Div {
    div()
        .text_xs()
        .font_semibold()
        .text_color(cx.theme().muted_foreground)
        .child(title.to_string())
}
