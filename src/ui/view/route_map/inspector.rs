use gpui::*;
use gpui_component::{
    group_box::{GroupBox, GroupBoxVariants},
    scroll::ScrollableElement as _,
    v_flex, ActiveTheme as _, StyledExt as _,
};

use crate::ui::state::WgApp;

use super::data::{RouteMapData, RouteMapInspector};
use super::{empty_group, status_chip, summary_chip};

pub(super) fn render_inspector(model: &RouteMapData, cx: &mut Context<WgApp>) -> Div {
    let Some(selected) = model.selected_item.as_ref() else {
        return div().child(empty_group(
            "Inspector",
            "Select a route, guardrail, or policy item to inspect why it exists.",
            cx,
        ));
    };

    div()
        .flex()
        .flex_col()
        .flex_1()
        .w_full()
        .h_full()
        .min_h(px(0.0))
        .child(
            GroupBox::new().fill().flex_grow().title("Inspector").child(
                v_flex()
                    .gap_3()
                    .flex_grow()
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
                    .child(render_section(
                        "Why It Matches",
                        &selected.inspector,
                        Section::Why,
                        cx,
                    ))
                    .child(render_section(
                        "Platform Details",
                        &selected.inspector,
                        Section::Platform,
                        cx,
                    ))
                    .child(render_section(
                        "Runtime Evidence",
                        &selected.inspector,
                        Section::Runtime,
                        cx,
                    ))
                    .child(render_section(
                        "Risk Assessment",
                        &selected.inspector,
                        Section::Risk,
                        cx,
                    )),
            ),
        )
}

enum Section {
    Why,
    Platform,
    Runtime,
    Risk,
}

fn render_section(
    title: &str,
    inspector: &RouteMapInspector,
    section: Section,
    cx: &mut Context<WgApp>,
) -> Div {
    let entries = match section {
        Section::Why => &inspector.why_match,
        Section::Platform => &inspector.platform_details,
        Section::Runtime => &inspector.runtime_evidence,
        Section::Risk => &inspector.risk_assessment,
    };

    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .text_xs()
                .font_semibold()
                .text_color(cx.theme().muted_foreground)
                .child(title.to_string()),
        )
        .child(entries.iter().fold(v_flex().gap_1(), |list, entry| {
            list.child(
                div()
                    .p_3()
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().group_box)
                    .text_sm()
                    .child(entry.clone()),
            )
        }))
}
