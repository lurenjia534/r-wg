use gpui::*;
use gpui_component::{
    group_box::{GroupBox, GroupBoxVariants},
    scroll::ScrollableElement as _,
    v_flex, ActiveTheme as _, StyledExt as _,
};

use crate::ui::state::WgApp;

use super::data::{RouteMapData, RouteMapGraphStepKind, RouteMapItemStatus};
use super::{empty_group, status_chip, summary_chip};

pub(super) fn render_inspector(model: &RouteMapData, cx: &mut Context<WgApp>) -> Div {
    let Some(selected) = model.selected_item.as_ref() else {
        return div().child(empty_group(
            "Inspector",
            "Select a route, guardrail, or policy item to inspect why it exists.",
            cx,
        ));
    };
    let content_style = StyleRefinement::default().flex_grow().min_h(px(0.0));

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
                .flex_grow()
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
                            v_flex()
                                .gap_3()
                                .w_full()
                                .min_h(px(0.0))
                                .overflow_y_scrollbar()
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
                                                    div()
                                                        .text_lg()
                                                        .font_semibold()
                                                        .child(selected.inspector.title.clone()),
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
                                                .children(selected.chips.iter().map(summary_chip)),
                                        ),
                                )
                                .child(
                                    if selected.status == RouteMapItemStatus::Warning
                                        || selected.status == RouteMapItemStatus::Failed
                                    {
                                        v_flex()
                                            .gap_3()
                                            .child(render_card_section(
                                                "Risk Assessment",
                                                &selected.inspector.risk_assessment,
                                                true,
                                                cx,
                                            ))
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
                                            .child(render_glossary_section(
                                                &selected.graph_steps,
                                                cx,
                                            ))
                                            .child(render_card_section(
                                                "Runtime Evidence",
                                                &selected.inspector.runtime_evidence,
                                                false,
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
                                            .child(render_glossary_section(
                                                &selected.graph_steps,
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
                                    },
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
    steps: &[super::data::RouteMapGraphStep],
    cx: &mut Context<WgApp>,
) -> Div {
    let mut kinds = Vec::new();
    for step in steps {
        if !kinds.contains(&step.kind) {
            kinds.push(step.kind);
        }
    }

    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(section_title("Glossary", cx))
        .child(kinds.into_iter().fold(v_flex().gap_2(), |list, kind| {
            let (label, body) = glossary_entry(kind);
            list.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border.alpha(0.7))
                    .bg(cx.theme().group_box.alpha(0.62))
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
        }))
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
