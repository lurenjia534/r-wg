use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::{Context, Window, *};
use gpui_component::{
    h_flex,
    input::InputState,
    resizable::{h_resizable, resizable_panel, ResizableState},
    tag::Tag,
    v_flex, ActiveTheme as _, PixelsExt, Sizable as _, StyledExt as _,
};

use crate::ui::features::configs::state::{ConfigsLibraryRow, ConfigsWorkspace};
use crate::ui::state::{ConfigInspectorTab, ConfigsPrimaryPane, WgApp};

use super::editor::render_editor_panel;
use super::inspector::{render_configs_primary_pane_tabs, render_inspector_panel};
use super::library::render_library_panel;
use super::{
    ConfigsLayoutMode, ConfigsRuntimeView, ConfigsViewData, CONFIGS_MEDIUM_INSPECTOR_HEIGHT,
};

// Desktop, medium, and compact layouts plus the shared shell header.

#[allow(clippy::too_many_arguments)]
pub(super) fn render_configs_desktop_layout(
    app_handle: &Entity<WgApp>,
    workspace: &Entity<ConfigsWorkspace>,
    runtime: &ConfigsRuntimeView,
    inspector_tab: ConfigInspectorTab,
    library_rows: &Arc<Vec<ConfigsLibraryRow>>,
    library_search_input: &Entity<InputState>,
    library_width: f32,
    inspector_width: f32,
    data: &ConfigsViewData,
    name_input: &Entity<InputState>,
    config_input: &Entity<InputState>,
    window: &mut Window,
    cx: &mut Context<ConfigsWorkspace>,
) -> AnyElement {
    div()
        .flex_1()
        .w_full()
        .min_w(px(0.0))
        .min_h(px(0.0))
        .child(
            h_resizable("configs-workspace")
                .on_resize({
                    let app = app_handle.clone();
                    move |state: &Entity<ResizableState>, _window, cx| {
                        let sizes = state.read(cx).sizes().clone();
                        if sizes.len() < 3 {
                            return;
                        }
                        let library_width = sizes[0].as_f32();
                        let inspector_width = sizes[2].as_f32();
                        app.update(cx, |app, cx| {
                            let changed = app.persist_configs_panel_widths(
                                library_width,
                                inspector_width,
                                cx,
                            );
                            if let Some(workspace) = app.ui.configs_workspace.clone() {
                                workspace.update(cx, |workspace, cx| {
                                    if workspace.set_panel_widths(library_width, inspector_width) {
                                        cx.notify();
                                    }
                                });
                            } else if changed {
                                cx.notify();
                            }
                        });
                    }
                })
                .child(
                    resizable_panel()
                        .size(px(library_width))
                        .size_range(px(272.0)..px(360.0))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .w_full()
                                .min_w(px(0.0))
                                .h_full()
                                .min_h(px(0.0))
                                .bg(cx.theme().background.alpha(0.74))
                                .border_r_1()
                                .border_color(cx.theme().border.alpha(0.72))
                                .child(configs_sidebar_shell(
                                    render_library_panel(
                                        app_handle,
                                        runtime.selected_id,
                                        data,
                                        workspace,
                                        library_rows,
                                        library_search_input,
                                        ConfigsLayoutMode::Desktop,
                                        window,
                                        cx,
                                    )
                                    .into_any_element(),
                                )),
                        ),
                )
                .child(
                    resizable_panel().size_range(px(560.0)..Pixels::MAX).child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .w_full()
                            .min_w(px(0.0))
                            .h_full()
                            .min_h(px(0.0))
                            .bg(cx.theme().tiles)
                            .child(configs_editor_shell(
                                render_editor_panel(
                                    app_handle,
                                    workspace,
                                    data,
                                    name_input,
                                    config_input,
                                    ConfigsLayoutMode::Desktop,
                                    cx,
                                )
                                .into_any_element(),
                            )),
                    ),
                )
                .child(
                    resizable_panel()
                        .size(px(inspector_width))
                        .size_range(px(288.0)..px(360.0))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .w_full()
                                .min_w(px(0.0))
                                .h_full()
                                .min_h(px(0.0))
                                .bg(cx.theme().background.alpha(0.78))
                                .border_l_1()
                                .border_color(cx.theme().border.alpha(0.72))
                                .child(configs_sidebar_shell(
                                    render_inspector_panel(
                                        runtime,
                                        workspace,
                                        inspector_tab,
                                        ConfigsLayoutMode::Desktop,
                                        data,
                                        cx,
                                    )
                                    .into_any_element(),
                                )),
                        ),
                ),
        )
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn render_configs_medium_layout(
    app_handle: &Entity<WgApp>,
    workspace: &Entity<ConfigsWorkspace>,
    runtime: &ConfigsRuntimeView,
    inspector_tab: ConfigInspectorTab,
    library_rows: &Arc<Vec<ConfigsLibraryRow>>,
    library_search_input: &Entity<InputState>,
    library_width: f32,
    inspector_width: f32,
    data: &ConfigsViewData,
    name_input: &Entity<InputState>,
    config_input: &Entity<InputState>,
    window: &mut Window,
    cx: &mut Context<ConfigsWorkspace>,
) -> AnyElement {
    div()
        .flex_1()
        .w_full()
        .min_w(px(0.0))
        .min_h(px(0.0))
        .child(
            h_resizable("configs-workspace-medium")
                .on_resize({
                    let app = app_handle.clone();
                    move |state: &Entity<ResizableState>, _window, cx| {
                        let sizes = state.read(cx).sizes().clone();
                        let Some(library_width) = sizes.first() else {
                            return;
                        };
                        app.update(cx, |app, cx| {
                            let changed = app.persist_configs_panel_widths(
                                library_width.as_f32(),
                                inspector_width,
                                cx,
                            );
                            if let Some(workspace) = app.ui.configs_workspace.clone() {
                                workspace.update(cx, |workspace, cx| {
                                    if workspace
                                        .set_panel_widths(library_width.as_f32(), inspector_width)
                                    {
                                        cx.notify();
                                    }
                                });
                            } else if changed {
                                cx.notify();
                            }
                        });
                    }
                })
                .child(
                    resizable_panel()
                        .size(px(library_width))
                        .size_range(px(264.0)..px(344.0))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .w_full()
                                .min_w(px(0.0))
                                .h_full()
                                .min_h(px(0.0))
                                .bg(cx.theme().background.alpha(0.74))
                                .border_r_1()
                                .border_color(cx.theme().border.alpha(0.72))
                                .child(configs_sidebar_shell(
                                    render_library_panel(
                                        app_handle,
                                        runtime.selected_id,
                                        data,
                                        workspace,
                                        library_rows,
                                        library_search_input,
                                        ConfigsLayoutMode::Medium,
                                        window,
                                        cx,
                                    )
                                    .into_any_element(),
                                )),
                        ),
                )
                .child(
                    resizable_panel().size_range(px(700.0)..Pixels::MAX).child(
                        configs_editor_shell(
                            div()
                                .flex()
                                .flex_col()
                                .h_full()
                                .min_h(px(0.0))
                                .child(div().flex_1().min_h(px(0.0)).overflow_hidden().child(
                                    render_editor_panel(
                                        app_handle,
                                        workspace,
                                        data,
                                        name_input,
                                        config_input,
                                        ConfigsLayoutMode::Medium,
                                        cx,
                                    ),
                                ))
                                .child(
                                    div()
                                        .h(px(CONFIGS_MEDIUM_INSPECTOR_HEIGHT))
                                        .min_h(px(CONFIGS_MEDIUM_INSPECTOR_HEIGHT))
                                        .border_t_1()
                                        .border_color(cx.theme().border.alpha(0.82))
                                        .bg(cx.theme().background.alpha(0.78))
                                        .child(render_inspector_panel(
                                            runtime,
                                            workspace,
                                            inspector_tab,
                                            ConfigsLayoutMode::Medium,
                                            data,
                                            cx,
                                        )),
                                )
                                .into_any_element(),
                        ),
                    ),
                ),
        )
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn render_configs_compact_layout(
    app_handle: &Entity<WgApp>,
    workspace: &Entity<ConfigsWorkspace>,
    runtime: &ConfigsRuntimeView,
    inspector_tab: ConfigInspectorTab,
    primary_pane: ConfigsPrimaryPane,
    library_rows: &Arc<Vec<ConfigsLibraryRow>>,
    library_search_input: &Entity<InputState>,
    data: &ConfigsViewData,
    name_input: &Entity<InputState>,
    config_input: &Entity<InputState>,
    window: &mut Window,
    cx: &mut Context<ConfigsWorkspace>,
) -> AnyElement {
    let active_panel = match primary_pane {
        ConfigsPrimaryPane::Library => render_library_panel(
            app_handle,
            runtime.selected_id,
            data,
            workspace,
            library_rows,
            library_search_input,
            ConfigsLayoutMode::Compact,
            window,
            cx,
        )
        .into_any_element(),
        ConfigsPrimaryPane::Editor => render_editor_panel(
            app_handle,
            workspace,
            data,
            name_input,
            config_input,
            ConfigsLayoutMode::Compact,
            cx,
        )
        .into_any_element(),
        ConfigsPrimaryPane::Inspector => render_inspector_panel(
            runtime,
            workspace,
            inspector_tab,
            ConfigsLayoutMode::Compact,
            data,
            cx,
        )
        .into_any_element(),
    };

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.0))
        .child(div().px_3().pt_3().child(render_configs_primary_pane_tabs(
            workspace,
            primary_pane,
            data,
            cx,
        )))
        .child(
            div()
                .flex_1()
                .min_h(px(0.0))
                .p_3()
                .pt_0()
                .child(active_panel),
        )
        .into_any_element()
}

pub(super) fn render_configs_shell_header(
    data: &ConfigsViewData,
    cx: &mut Context<ConfigsWorkspace>,
) -> Div {
    let selected_name = data.title.clone();

    div()
        .px_5()
        .py_3()
        .border_b_1()
        .border_color(cx.theme().border.alpha(0.78))
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .flex_wrap()
                .gap_4()
                .child(
                    v_flex()
                        .gap_0p5()
                        .child(div().text_lg().font_semibold().child("Configs"))
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("Edit, validate, and manage tunnel profiles."),
                        ),
                )
                .child(
                    h_flex()
                        .items_center()
                        .flex_wrap()
                        .gap_2()
                        .child(Tag::secondary().small().rounded_full().child(selected_name))
                        .when(data.shared.draft_dirty, |this| {
                            this.child(Tag::warning().small().rounded_full().child("Dirty"))
                        })
                        .when(data.shared.needs_restart, |this| {
                            this.child(Tag::warning().small().rounded_full().child("Needs restart"))
                        })
                        .when(data.is_running_draft, |this| {
                            this.child(Tag::success().small().rounded_full().child("Running"))
                        }),
                ),
        )
}

fn configs_sidebar_shell(child: AnyElement) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .w_full()
        .min_w(px(0.0))
        .h_full()
        .min_h(px(0.0))
        .p_2()
        .child(child)
}

fn configs_editor_shell(child: AnyElement) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .w_full()
        .min_w(px(0.0))
        .h_full()
        .min_h(px(0.0))
        .px_4()
        .py_3()
        .child(child)
}
