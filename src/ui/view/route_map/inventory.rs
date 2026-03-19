use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    group_box::{GroupBox, GroupBoxVariants},
    h_flex,
    scroll::ScrollableElement as _,
    v_flex, ActiveTheme as _, StyledExt as _,
};

use crate::ui::state::WgApp;

use super::data::{RouteMapData, RouteMapInventoryGroup, RouteMapInventoryItem};
use super::{status_chip, summary_chip};

pub(super) fn render_inventory(model: &RouteMapData, cx: &mut Context<WgApp>) -> Div {
    let content = if !model.has_plan {
        v_flex()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(model.plan_status.clone()),
            )
            .when_some(model.parse_error.as_ref(), |this, parse_error| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().danger)
                        .child(parse_error.clone()),
                )
            })
    } else {
        model
            .inventory_groups
            .iter()
            .fold(v_flex().gap_3(), |content, group| {
                content.child(render_group(group, model.selected_item_id.as_ref(), cx))
            })
    };

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
                .title("Inventory / Groups")
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_grow()
                        .w_full()
                        .min_h(px(0.0))
                        .gap_3()
                        .overflow_y_scrollbar()
                        .child(content),
                ),
        )
}

fn render_group(
    group: &RouteMapInventoryGroup,
    selected_id: Option<&SharedString>,
    cx: &mut Context<WgApp>,
) -> Stateful<Div> {
    let header = h_flex().items_center().justify_between().gap_2().child(
        v_flex()
            .gap_1()
            .child(div().text_sm().font_semibold().child(group.label.clone()))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(group.summary.clone()),
            ),
    );

    let body = if group.items.is_empty() {
        div()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(group.empty_note.clone())
    } else {
        group.items.iter().fold(v_flex().gap_2(), |list, item| {
            list.child(render_item(item, selected_id, cx))
        })
    };

    div()
        .flex()
        .flex_col()
        .w_full()
        .gap_2()
        .id(group.id.clone())
        .child(header)
        .child(body)
}

fn render_item(
    item: &RouteMapInventoryItem,
    selected_id: Option<&SharedString>,
    cx: &mut Context<WgApp>,
) -> Stateful<Div> {
    let selected = selected_id == Some(&item.id);
    let border = if selected {
        cx.theme().accent
    } else {
        cx.theme().border
    };
    let background = if selected {
        cx.theme().accent.alpha(0.08)
    } else {
        cx.theme().group_box
    };
    let accent = if selected {
        cx.theme().accent
    } else {
        cx.theme().border.alpha(0.0)
    };
    let item_id = item.id.clone();

    div()
        .id(item.id.clone())
        .flex()
        .flex_col()
        .w_full()
        .gap_2()
        .p_3()
        .rounded_lg()
        .border_1()
        .border_color(border)
        .bg(background)
        .cursor_pointer()
        .relative()
        .child(
            div()
                .absolute()
                .top(px(10.0))
                .bottom(px(10.0))
                .left(px(0.0))
                .w(px(3.0))
                .rounded_md()
                .bg(accent),
        )
        .child(
            h_flex()
                .items_start()
                .justify_between()
                .gap_3()
                .child(
                    v_flex()
                        .gap_1()
                        .child(div().text_sm().font_semibold().child(item.title.clone()))
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(item.subtitle.clone()),
                        ),
                )
                .child(status_chip(item.status)),
        )
        .child(
            h_flex()
                .items_center()
                .gap_1()
                .flex_wrap()
                .children(item.chips.iter().map(summary_chip)),
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            this.set_route_map_selected_item(Some(item_id.clone()), cx);
        }))
}
