use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    h_flex, tag::Tag, v_flex, ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _,
};

use crate::ui::state::WgApp;

pub(super) fn card_title(
    icon: IconName,
    label: &str,
    trailing_icon: Option<IconName>,
    cx: &mut Context<WgApp>,
) -> Div {
    h_flex()
        .items_center()
        .justify_between()
        .child(
            h_flex()
                .items_center()
                .gap_2()
                .child(
                    Icon::new(icon)
                        .size_4()
                        .text_color(cx.theme().accent_foreground),
                )
                .child(div().text_base().font_semibold().child(label.to_string())),
        )
        .when_some(trailing_icon, |this, icon| {
            this.child(
                Icon::new(icon)
                    .size_4()
                    .text_color(cx.theme().muted_foreground),
            )
        })
}

pub(super) fn metric_cell(
    icon: IconName,
    label: &str,
    value: &str,
    color: impl Into<Hsla>,
    cx: &mut Context<WgApp>,
) -> Div {
    let color: Hsla = color.into();
    v_flex()
        .gap_1()
        .flex_grow()
        .min_w(px(0.0))
        .px_4()
        .py_2()
        .child(
            h_flex()
                .items_center()
                .gap_2()
                .text_sm()
                .font_semibold()
                .text_color(cx.theme().foreground)
                .child(Icon::new(icon).size_4().text_color(color))
                .child(label.to_string()),
        )
        .child(
            div()
                .text_2xl()
                .font_semibold()
                .text_color(color)
                .child(value.to_string()),
        )
}

pub(super) fn status_item(
    icon: IconName,
    label: &str,
    value: &str,
    color: impl Into<Hsla>,
    cx: &mut Context<WgApp>,
) -> Div {
    let color: Hsla = color.into();
    v_flex()
        .gap_1()
        .flex_grow()
        .min_w(px(0.0))
        .px_4()
        .py_2()
        .child(
            h_flex()
                .items_center()
                .gap_2()
                .text_sm()
                .font_semibold()
                .text_color(cx.theme().foreground)
                .child(Icon::new(icon).size_3().text_color(color))
                .child(label.to_string()),
        )
        .child(
            div()
                .text_base()
                .font_semibold()
                .text_color(cx.theme().foreground)
                .child(value.to_string()),
        )
}

pub(super) fn status_state_item(is_running: bool, cx: &mut Context<WgApp>) -> Div {
    let (state_text, state_icon, tag) = if is_running {
        ("On", IconName::CircleCheck, Tag::success())
    } else {
        ("Off", IconName::CircleX, Tag::secondary().outline())
    };

    v_flex()
        .gap_1()
        .flex_grow()
        .min_w(px(0.0))
        .px_4()
        .py_2()
        .child(
            div()
                .text_sm()
                .font_semibold()
                .text_color(cx.theme().foreground)
                .child("Status"),
        )
        .child(
            tag.small()
                .gap_1()
                .child(Icon::new(state_icon).size_3())
                .child(state_text),
        )
}

pub(super) fn two_row_grid(top: [Div; 3], bottom: [Div; 3], cx: &mut Context<WgApp>) -> Div {
    let [top_left, top_mid, top_right] = top;
    let [bottom_left, bottom_mid, bottom_right] = bottom;
    let border = cx.theme().border;
    div()
        .grid()
        .grid_cols(3)
        .gap_0()
        .child(top_left.border_r_1().border_color(border))
        .child(top_mid.border_r_1().border_color(border))
        .child(top_right)
        .child(bottom_left.border_r_1().border_t_1().border_color(border))
        .child(bottom_mid.border_r_1().border_t_1().border_color(border))
        .child(bottom_right.border_t_1().border_color(border))
}

pub(super) fn vertical_rule(cx: &mut Context<WgApp>) -> Div {
    div().w(px(1.0)).h(px(64.0)).bg(cx.theme().border)
}
