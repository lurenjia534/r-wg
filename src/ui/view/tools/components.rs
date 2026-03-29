use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    group_box::{GroupBox, GroupBoxVariants},
    h_flex,
    scroll::ScrollableElement as _,
    tag::Tag,
    v_flex, ActiveTheme as _, Sizable as _, StyledExt as _,
};

pub(super) fn empty_result_state<T>(message: &str, cx: &mut Context<T>) -> Div {
    div()
        .rounded_lg()
        .border_1()
        .border_color(cx.theme().border.alpha(0.55))
        .bg(cx.theme().group_box)
        .px_4()
        .py_4()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(message.to_string())
}

pub(super) fn error_banner<T>(message: impl Into<SharedString>, cx: &mut Context<T>) -> Div {
    div()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().danger.alpha(0.45))
        .bg(cx.theme().danger.alpha(0.08))
        .px_3()
        .py_2()
        .text_sm()
        .text_color(cx.theme().danger)
        .child(message.into())
}

pub(super) fn readonly_text_block<T>(
    title: &str,
    content: &str,
    monospace: bool,
    cx: &mut Context<T>,
) -> GroupBox {
    GroupBox::new().fill().title(title.to_string()).child(
        div()
            .max_h(px(220.0))
            .overflow_y_scrollbar()
            .rounded_lg()
            .border_1()
            .border_color(cx.theme().border.alpha(0.55))
            .bg(cx.theme().group_box)
            .px_3()
            .py_2()
            .when(monospace, |this| {
                this.font_family(cx.theme().mono_font_family.clone())
            })
            .text_sm()
            .child(if content.is_empty() {
                "-".to_string()
            } else {
                content.to_string()
            }),
    )
}

pub(super) fn summary_block<T>(
    title: &str,
    rows: &[(SharedString, SharedString)],
    cx: &mut Context<T>,
) -> GroupBox {
    GroupBox::new()
        .fill()
        .title(title.to_string())
        .child(v_flex().gap_2().children(rows.iter().map(|(label, value)| {
            h_flex()
                .items_center()
                .justify_between()
                .gap_3()
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(label.clone()),
                )
                .child(div().text_sm().font_semibold().child(value.clone()))
        })))
}

pub(super) fn active_config_source_tag(source: crate::ui::state::ActiveConfigSource) -> Tag {
    match source {
        crate::ui::state::ActiveConfigSource::Draft => {
            Tag::info().small().rounded_full().child("Draft")
        }
        crate::ui::state::ActiveConfigSource::SavedSelection => Tag::secondary()
            .small()
            .rounded_full()
            .child("Saved Config"),
        crate::ui::state::ActiveConfigSource::None => {
            Tag::secondary().small().rounded_full().child("No Config")
        }
    }
}
