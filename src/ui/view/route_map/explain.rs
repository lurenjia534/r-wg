use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    group_box::{GroupBox, GroupBoxVariants},
    scroll::ScrollableElement as _,
    v_flex, ActiveTheme as _, StyledExt as _,
};

use crate::ui::state::WgApp;

use super::data::RouteMapData;
use super::{empty_group, summary_chip};

pub(super) fn render_explain(model: &RouteMapData, cx: &mut Context<WgApp>) -> Div {
    let Some(explain) = model.explain.as_ref() else {
        return div().child(empty_group(
            "Explain",
            "Search for an IP, CIDR, or domain to explain the decision chain.",
            cx,
        ));
    };
    let content_style = StyleRefinement::default().flex_1().min_h_0();

    div()
        .flex()
        .flex_col()
        .flex_1()
        .h_full()
        .min_h(px(0.0))
        .child(
            GroupBox::new()
                .fill()
                .flex_1()
                .min_h_0()
                .content_style(content_style)
                .title("Explain")
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
                                            div()
                                                .text_xs()
                                                .font_semibold()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(if explain.query.is_empty() {
                                                    SharedString::from("QUERY")
                                                } else {
                                                    explain.query.clone()
                                                }),
                                        )
                                        .child(
                                            div()
                                                .text_lg()
                                                .font_semibold()
                                                .child(explain.headline.clone()),
                                        )
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(explain.summary.clone()),
                                        )
                                        .when_some(
                                            explain.matched_item_id.as_ref(),
                                            |this, matched| {
                                                this.child(summary_chip(
                                                    &super::data::RouteMapChip {
                                                        label: format!("Linked to {}", matched)
                                                            .into(),
                                                        tone: super::data::RouteMapTone::Info,
                                                    },
                                                ))
                                            },
                                        )
                                        .child(explain.steps.iter().fold(
                                            v_flex().gap_2(),
                                            |list, step| {
                                                list.child(
                                                    div()
                                                        .p_3()
                                                        .rounded_lg()
                                                        .border_1()
                                                        .border_color(cx.theme().border)
                                                        .bg(cx.theme().group_box)
                                                        .text_sm()
                                                        .child(step.clone()),
                                                )
                                            },
                                        ))
                                        .when(!explain.risk.is_empty(), |this| {
                                            this.child(
                                                v_flex()
                                                    .gap_2()
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .font_semibold()
                                                            .text_color(cx.theme().warning)
                                                            .child("RISK"),
                                                    )
                                                    .child(explain.risk.iter().fold(
                                                        v_flex().gap_1(),
                                                        |list, item| {
                                                            list.child(
                                                                div()
                                                                    .text_sm()
                                                                    .text_color(
                                                                        cx.theme().muted_foreground,
                                                                    )
                                                                    .child(item.clone()),
                                                            )
                                                        },
                                                    )),
                                            )
                                        }),
                                ),
                        ),
                ),
        )
}
