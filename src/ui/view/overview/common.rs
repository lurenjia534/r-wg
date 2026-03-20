use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariants as _},
    group_box::{GroupBox, GroupBoxVariants},
    h_flex,
    tag::Tag,
    v_flex, ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, StyledExt as _,
};

use crate::ui::state::WgApp;

#[derive(Clone, Copy)]
pub(super) enum OverviewSectionTone {
    Primary,
    Secondary,
}

pub(super) fn overview_section<T>(
    tone: OverviewSectionTone,
    title: impl IntoElement,
    body: impl IntoElement,
    _cx: &mut Context<T>,
) -> GroupBox {
    match tone {
        OverviewSectionTone::Primary => GroupBox::new().fill().title(title).child(body),
        OverviewSectionTone::Secondary => GroupBox::new().outline().title(title).child(body),
    }
}

pub(super) fn section_title<T>(
    icon: IconName,
    label: &str,
    caption: Option<impl Into<SharedString>>,
    tone: OverviewSectionTone,
    cx: &mut Context<T>,
) -> Div {
    let icon_tint = cx.theme().muted_foreground;
    let icon_bg = match tone {
        OverviewSectionTone::Primary => {
            cx.theme()
                .secondary
                .alpha(if cx.theme().is_dark() { 0.32 } else { 0.58 })
        }
        OverviewSectionTone::Secondary => {
            cx.theme()
                .background
                .alpha(if cx.theme().is_dark() { 0.64 } else { 0.88 })
        }
    };
    let caption = caption.map(Into::into);
    h_flex().items_start().justify_between().gap_2().child(
        h_flex()
            .items_start()
            .gap_2()
            .child(
                div()
                    .size(px(28.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .bg(icon_bg)
                    .child(Icon::new(icon).size_4().text_color(icon_tint)),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .font_semibold()
                            .text_color(cx.theme().foreground)
                            .child(label.to_string()),
                    )
                    .when_some(caption, |this, caption| {
                        this.child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(cx.theme().muted_foreground)
                                .child(caption),
                        )
                    }),
            ),
    )
}

pub(super) fn tile_border<T>(cx: &mut Context<T>) -> Hsla {
    cx.theme()
        .border
        .alpha(if cx.theme().is_dark() { 0.68 } else { 0.52 })
}

pub(super) fn tile_surface<T>(cx: &mut Context<T>) -> Hsla {
    cx.theme()
        .background
        .alpha(if cx.theme().is_dark() { 0.62 } else { 0.8 })
}

pub(super) fn tile_shell<T>(cx: &mut Context<T>) -> Div {
    v_flex()
        .gap_1()
        .flex_grow()
        .min_w(px(0.0))
        .p_3()
        .rounded_lg()
        .border_1()
        .border_color(tile_border(cx))
        .bg(tile_surface(cx))
}

pub(super) fn tile_icon<T>(icon: IconName, color: Hsla, cx: &mut Context<T>) -> Div {
    div()
        .size(px(24.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .bg(color.alpha(if cx.theme().is_dark() { 0.14 } else { 0.1 }))
        .child(Icon::new(icon).size_3().text_color(color))
}

pub(super) fn tile_header<T>(
    icon: IconName,
    label: &str,
    color: Hsla,
    trailing: Option<AnyElement>,
    cx: &mut Context<T>,
) -> Div {
    h_flex()
        .items_center()
        .justify_between()
        .gap_2()
        .child(
            h_flex()
                .items_center()
                .gap_2()
                .child(tile_icon(icon, color, cx))
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(cx.theme().muted_foreground)
                        .child(label.to_string()),
                ),
        )
        .when_some(trailing, |this, trailing| this.child(trailing))
}

pub(super) fn metric_cell<T>(
    icon: IconName,
    label: &str,
    value: &str,
    color: impl Into<Hsla>,
    monospace: bool,
    cx: &mut Context<T>,
) -> Div {
    let color: Hsla = color.into();
    tile_shell(cx)
        .child(tile_header(icon, label, color, None, cx))
        .child(
            div()
                .text_lg()
                .font_semibold()
                .text_color(cx.theme().foreground)
                .when(monospace, |this| {
                    this.font_family(cx.theme().mono_font_family.clone())
                })
                .child(value.to_string()),
        )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn copyable_metric_cell<T>(
    app_handle: &Entity<WgApp>,
    id: &'static str,
    icon: IconName,
    label: &str,
    value: &str,
    color: impl Into<Hsla>,
    monospace: bool,
    cx: &mut Context<T>,
) -> Div {
    let color: Hsla = color.into();
    let can_copy = !value.trim().is_empty() && value != "-" && value != "\u{2014}";
    let copy_value = value.to_string();
    let copy_label = label.to_string();
    let copy_button = Button::new(id)
        .ghost()
        .small()
        .icon(Icon::new(IconName::Copy).size_3())
        .disabled(!can_copy)
        .tooltip(if can_copy {
            "Copy value"
        } else {
            "No value to copy"
        })
        .on_click({
            let app_handle = app_handle.clone();
            move |_, window, cx| {
                if !can_copy {
                    return;
                }
                let copy_value = copy_value.clone();
                let copy_label = copy_label.clone();
                app_handle.update(cx, |app, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(copy_value));
                    app.push_success_toast(format!("{copy_label} copied"), window, cx);
                    app.set_status(format!("{copy_label} copied"));
                });
            }
        });

    tile_shell(cx)
        .child(tile_header(
            icon,
            label,
            color,
            Some(copy_button.into_any_element()),
            cx,
        ))
        .child(
            div()
                .text_lg()
                .font_semibold()
                .text_color(cx.theme().foreground)
                .when(monospace, |this| {
                    this.font_family(cx.theme().mono_font_family.clone())
                })
                .child(value.to_string()),
        )
}

pub(super) fn status_item<T>(
    icon: IconName,
    label: &str,
    value: &str,
    color: impl Into<Hsla>,
    monospace: bool,
    cx: &mut Context<T>,
) -> Div {
    let color: Hsla = color.into();
    tile_shell(cx)
        .child(tile_header(icon, label, color, None, cx))
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .text_color(cx.theme().foreground)
                .when(monospace, |this| {
                    this.font_family(cx.theme().mono_font_family.clone())
                })
                .child(value.to_string()),
        )
}

pub(super) fn status_state_item<T>(is_running: bool, cx: &mut Context<T>) -> Div {
    let (state_text, state_icon, tag) = if is_running {
        ("On", IconName::CircleCheck, Tag::success())
    } else {
        ("Off", IconName::CircleX, Tag::secondary().outline())
    };

    let color = if is_running {
        cx.theme().success
    } else {
        cx.theme().muted_foreground
    };

    tile_shell(cx)
        .child(tile_header(state_icon.clone(), "Status", color, None, cx))
        .child(
            tag.xsmall()
                .gap_1()
                .child(Icon::new(state_icon).size_3())
                .child(state_text),
        )
}

pub(super) fn two_row_grid<T>(top: [Div; 3], bottom: [Div; 3], _cx: &mut Context<T>) -> Div {
    let [top_left, top_mid, top_right] = top;
    let [bottom_left, bottom_mid, bottom_right] = bottom;
    div()
        .grid()
        .grid_cols(3)
        .gap_2()
        .child(top_left)
        .child(top_mid)
        .child(top_right)
        .child(bottom_left)
        .child(bottom_mid)
        .child(bottom_right)
}

pub(super) fn vertical_rule<T>(cx: &mut Context<T>) -> Div {
    div()
        .w(px(1.0))
        .h(px(56.0))
        .bg(cx
            .theme()
            .border
            .alpha(if cx.theme().is_dark() { 0.55 } else { 0.45 }))
}
