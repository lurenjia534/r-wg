use gpui::prelude::FluentBuilder as _;
use gpui::{Context, *};
use gpui_component::{
    h_flex,
    input::{Input, InputState},
    tag::Tag,
    v_flex, ActiveTheme as _, Sizable as _, StyledExt as _,
};

use crate::ui::features::configs::state::ConfigsWorkspace;
use crate::ui::state::WgApp;

use super::inspector::{editor_action_bar, render_diagnostics_strip};
use super::{ConfigsLayoutMode, ConfigsViewData};

// Draft editor rendering and editor-side actions.

pub(super) fn render_editor_panel(
    app_handle: &Entity<WgApp>,
    workspace: &Entity<ConfigsWorkspace>,
    data: &ConfigsViewData,
    name_input: &Entity<InputState>,
    config_input: &Entity<InputState>,
    mode: ConfigsLayoutMode,
    cx: &mut Context<ConfigsWorkspace>,
) -> Div {
    let desktop = matches!(mode, ConfigsLayoutMode::Desktop);
    let compact = matches!(mode, ConfigsLayoutMode::Compact);
    let framed = compact;
    let document_badge = if data.has_saved_source {
        Tag::secondary()
            .xsmall()
            .rounded_full()
            .child("Saved source")
    } else {
        Tag::warning().xsmall().rounded_full().child("Draft")
    };
    let status_tags = h_flex()
        .items_center()
        .gap_2()
        .flex_wrap()
        .when(data.shared.draft_dirty, |this| {
            this.child(Tag::warning().xsmall().rounded_full().child("Dirty"))
        })
        .when(data.shared.needs_restart, |this| {
            this.child(
                Tag::warning()
                    .xsmall()
                    .rounded_full()
                    .child("Needs restart"),
            )
        })
        .when(data.is_running_draft, |this| {
            this.child(Tag::success().xsmall().rounded_full().child("Running"))
        });
    let title_block = if data.title_editing {
        div()
            .max_w_full()
            .px_1()
            .py_1()
            .border_b_1()
            .border_color(cx.theme().ring.alpha(0.55))
            .child(
                Input::new(name_input)
                    .appearance(false)
                    .focus_bordered(false)
                    .bordered(false),
            )
            .into_any_element()
    } else {
        let workspace = workspace.clone();
        let name_input = name_input.clone();
        div()
            .id("configs-editor-title-display")
            .max_w_full()
            .px_1()
            .py_1()
            .rounded_lg()
            .cursor_pointer()
            .hover(|this| this.bg(cx.theme().list_hover))
            .child(
                div()
                    .text_2xl()
                    .font_semibold()
                    .truncate()
                    .child(data.title.clone()),
            )
            .on_click(move |_, window, cx| {
                workspace.update(cx, |workspace, cx| {
                    let changed = workspace.set_title_editing(true);
                    name_input.update(cx, |input, cx| {
                        input.focus(window, cx);
                    });
                    if changed {
                        cx.notify();
                    }
                });
            })
            .into_any_element()
    };

    div()
        .flex()
        .flex_col()
        .flex_1()
        .w_full()
        .min_w(px(0.0))
        .h_full()
        .min_h(px(0.0))
        .bg(cx.theme().background.alpha(if compact { 0.0 } else { 0.9 }))
        .when(framed, |this| {
            this.rounded_xl()
                .border_1()
                .border_color(cx.theme().border.alpha(0.92))
                .bg(linear_gradient(
                    180.0,
                    linear_color_stop(cx.theme().background, 0.0),
                    linear_color_stop(cx.theme().group_box, 1.0),
                ))
                .shadow_sm()
        })
        .child(
            div()
                .px(px(if compact { 20.0 } else { 24.0 }))
                .py(px(if compact { 20.0 } else { 18.0 }))
                .border_b_1()
                .border_color(cx.theme().border.alpha(if compact { 0.62 } else { 0.88 }))
                .bg(cx
                    .theme()
                    .background
                    .alpha(if compact { 0.0 } else { 0.94 }))
                .child(
                    v_flex()
                        .gap(if compact { px(12.0) } else { px(12.0) })
                        .child(
                            h_flex()
                                .items_center()
                                .justify_between()
                                .flex_wrap()
                                .gap_4()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_semibold()
                                        .text_color(cx.theme().muted_foreground)
                                        .child("DOCUMENT"),
                                )
                                .child(editor_action_bar(data, app_handle, cx)),
                        )
                        .child(
                            h_flex()
                                .items_start()
                                .justify_between()
                                .flex_wrap()
                                .gap_4()
                                .child(
                                    v_flex()
                                        .gap(px(if compact { 8.0 } else { 8.0 }))
                                        .child(
                                            h_flex()
                                                .items_center()
                                                .gap_2()
                                                .flex_wrap()
                                                .child(title_block)
                                                .child(document_badge)
                                                .child(status_tags),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(data.source_summary.clone()),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .max_w(px(620.0))
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(data.runtime_note.clone()),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_3()
                .flex_1()
                .min_h(px(0.0))
                .overflow_hidden()
                .px(px(if compact { 16.0 } else { 24.0 }))
                .pb(px(if compact { 16.0 } else { 24.0 }))
                .pt(px(if compact { 16.0 } else { 20.0 }))
                .child(render_diagnostics_strip(data, cx))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .w_full()
                        .flex_1()
                        .min_h(px(0.0))
                        .when(desktop, |this| this.min_h(px(500.0)))
                        .rounded_xl()
                        .border_1()
                        .border_color(cx.theme().border.alpha(if compact { 0.9 } else { 0.88 }))
                        .bg(cx
                            .theme()
                            .background
                            .alpha(if compact { 1.0 } else { 0.98 }))
                        .shadow_sm()
                        .overflow_hidden()
                        .child(
                            div()
                                .px_4()
                                .py(px(if compact { 8.0 } else { 9.0 }))
                                .border_b_1()
                                .border_color(cx.theme().border.alpha(0.64))
                                .bg(cx
                                    .theme()
                                    .background
                                    .alpha(if compact { 0.92 } else { 0.9 }))
                                .child(
                                    div()
                                        .text_xs()
                                        .font_semibold()
                                        .text_color(cx.theme().muted_foreground)
                                        .child("WIREGUARD CONFIG"),
                                ),
                        )
                        .child(
                            div().flex_1().min_h(px(0.0)).px_3().py_2().child(
                                Input::new(config_input)
                                    .appearance(false)
                                    .focus_bordered(false)
                                    .bordered(false)
                                    .h_full(),
                            ),
                        ),
                ),
        )
}
