use gpui::prelude::FluentBuilder as _;
use gpui::{Context, *};
use gpui_component::{
    input::{Input, InputState},
    h_flex, v_flex, ActiveTheme as _, StyledExt as _,
};

use crate::ui::state::{ConfigsWorkspace, WgApp};
use crate::ui::view::configs::ConfigsViewData;

use super::inspector::{editor_action_bar, render_diagnostics_strip};
use super::ConfigsLayoutMode;

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
            .w_full()
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
        .h_full()
        .min_h(px(0.0))
        .rounded_xl()
        .border_1()
        .border_color(cx.theme().border.alpha(0.92))
        .bg(if compact {
            linear_gradient(
                180.0,
                linear_color_stop(cx.theme().background, 0.0),
                linear_color_stop(cx.theme().group_box, 1.0),
            )
        } else {
            linear_gradient(
                180.0,
                linear_color_stop(cx.theme().background.alpha(0.98), 0.0),
                linear_color_stop(cx.theme().group_box.alpha(0.88), 1.0),
            )
        })
        .shadow_sm()
        .child(
            div()
                .px_6()
                .py_5()
                .border_b_1()
                .border_color(cx.theme().border.alpha(0.62))
                .child(
                    v_flex().gap_3().child(
                        h_flex()
                            .items_start()
                            .justify_between()
                            .flex_wrap()
                            .gap_4()
                            .child(
                                v_flex()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_semibold()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("DOCUMENT"),
                                    )
                                    .child(title_block)
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .gap_3()
                                            .flex_wrap()
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(data.source_summary.clone()),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(data.runtime_note.clone()),
                                            ),
                                    ),
                            )
                            .child(editor_action_bar(data, app_handle, cx)),
                    ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_4()
                .flex_1()
                .min_h(px(0.0))
                .overflow_hidden()
                .px_4()
                .pb_4()
                .pt_4()
                .child(render_diagnostics_strip(data, cx))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .w_full()
                        .flex_1()
                        .min_h(px(0.0))
                        .when(desktop, |this| this.min_h(px(500.0)))
                        .rounded_2xl()
                        .border_1()
                        .border_color(cx.theme().border.alpha(0.9))
                        .bg(cx.theme().group_box)
                        .shadow_sm()
                        .overflow_hidden()
                        .child(
                            div()
                                .px_4()
                                .py_2()
                                .border_b_1()
                                .border_color(cx.theme().border.alpha(0.52))
                                .bg(linear_gradient(
                                    180.0,
                                    linear_color_stop(cx.theme().group_box.alpha(0.98), 0.0),
                                    linear_color_stop(cx.theme().background.alpha(0.92), 1.0),
                                ))
                                .child(
                                    div()
                                        .text_xs()
                                        .font_semibold()
                                        .text_color(cx.theme().muted_foreground)
                                        .child("WIREGUARD CONFIG"),
                                ),
                        )
                        .child(
                            div().flex_1().min_h(px(0.0)).p_2().child(
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
