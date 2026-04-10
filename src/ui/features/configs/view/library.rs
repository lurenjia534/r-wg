use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::{App, Stateful, Window, *};
use gpui_component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputState},
    scroll::Scrollbar,
    tag::Tag,
    v_flex, ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, StyledExt as _,
};

use crate::ui::features::configs::state::{ConfigsLibraryRow, ConfigsWorkspace};
use crate::ui::state::{ConfigsPrimaryPane, EndpointFamily, WgApp};

use super::inspector::{endpoint_family_tag, source_tag};
use super::{
    ConfigsLayoutMode, ConfigsViewData, CONFIGS_LIBRARY_ROW_HEIGHT, CONFIGS_LIBRARY_SCROLL_STATE_ID,
};

// Library search, list, and row rendering.

#[allow(clippy::too_many_arguments)]
pub(super) fn render_library_panel(
    app_handle: &Entity<WgApp>,
    selected_id: Option<u64>,
    data: &ConfigsViewData,
    workspace: &Entity<ConfigsWorkspace>,
    rows: &Arc<Vec<ConfigsLibraryRow>>,
    search_input: &Entity<InputState>,
    mode: ConfigsLayoutMode,
    window: &mut Window,
    cx: &mut Context<ConfigsWorkspace>,
) -> Div {
    let compact = matches!(mode, ConfigsLayoutMode::Compact);
    let framed = compact;
    let query = search_input.read(cx).value().trim().to_lowercase();
    let total_count = rows.len();
    let filtered_indices = Arc::new(
        rows.iter()
            .enumerate()
            .filter_map(|(ix, row)| {
                let matches_query = query.is_empty()
                    || row.name_lower.contains(&query)
                    || row.subtitle_lower.contains(&query)
                    || row.family_label.contains(&query)
                    || row.source_label.contains(&query);
                matches_query.then_some(ix)
            })
            .collect::<Vec<_>>(),
    );
    let count = filtered_indices.len();
    let rows_for_list = rows.clone();
    let visible_indices = filtered_indices.clone();
    let app_for_list = app_handle.clone();
    let workspace_for_list = workspace.clone();
    let scroll_handle = window
        .use_keyed_state(CONFIGS_LIBRARY_SCROLL_STATE_ID, cx, |_, _| {
            UniformListScrollHandle::new()
        })
        .read(cx)
        .clone();
    let list = uniform_list(
        "configs-library-list",
        visible_indices.len(),
        move |visible_range, _window, cx| {
            visible_range
                .map(|ix| {
                    let row_ix = visible_indices[ix];
                    render_library_row(
                        &app_for_list,
                        &workspace_for_list,
                        selected_id,
                        &rows_for_list[row_ix],
                        compact,
                        cx,
                    )
                })
                .collect::<Vec<_>>()
        },
    )
    .track_scroll(scroll_handle.clone())
    .w_full()
    .flex_1();

    div()
        .flex()
        .flex_col()
        .flex_1()
        .w_full()
        .min_w(px(0.0))
        .h_full()
        .min_h(px(0.0))
        .when(framed, |this| {
            this.rounded_lg()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().background)
        })
        .child(
            div()
                .px(px(if compact { 16.0 } else { 20.0 }))
                .py(px(if compact { 16.0 } else { 18.0 }))
                .border_b_1()
                .border_color(cx.theme().border.alpha(if compact { 1.0 } else { 0.88 }))
                .child(
                    v_flex()
                        .gap(px(if compact { 10.0 } else { 10.0 }))
                        .child(
                            h_flex()
                                .items_start()
                                .justify_between()
                                .child(
                                    v_flex()
                                        .gap_0p5()
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_semibold()
                                                .text_color(cx.theme().muted_foreground)
                                                .child("LIBRARY"),
                                        )
                                        .child(
                                            div()
                                                .text_base()
                                                .font_semibold()
                                                .child("Tunnel configs"),
                                        ),
                                )
                                .child(Tag::secondary().small().child(if query.is_empty() {
                                    format!("{count} configs")
                                } else {
                                    format!("{count}/{total_count} configs")
                                })),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .gap_2()
                                .px(px(10.0))
                                .py(px(7.0))
                                .rounded_md()
                                .border_1()
                                .border_color(cx.theme().border.alpha(if compact {
                                    0.52
                                } else {
                                    0.42
                                }))
                                .bg(cx
                                    .theme()
                                    .background
                                    .alpha(if compact { 0.92 } else { 0.86 }))
                                .child(Icon::new(IconName::Search).size_4())
                                .child(
                                    Input::new(search_input)
                                        .appearance(false)
                                        .bordered(false)
                                        .cleanable(true),
                                ),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .gap_1()
                                .w_full()
                                .child(
                                    Button::new("cfg-new")
                                        .icon(Icon::new(IconName::Plus).size_3())
                                        .label("New")
                                        .primary()
                                        .small()
                                        .compact()
                                        .disabled(data.is_busy)
                                        .on_click({
                                            let app = app_handle.clone();
                                            let workspace = workspace.clone();
                                            move |_, window, cx| {
                                                app.update(cx, |this, cx| {
                                                    this.command_new_draft(window, cx);
                                                });
                                                if compact {
                                                    workspace.update(cx, |workspace, cx| {
                                                        if workspace.set_primary_pane(
                                                            ConfigsPrimaryPane::Editor,
                                                        ) {
                                                            cx.notify();
                                                        }
                                                    });
                                                }
                                            }
                                        }),
                                )
                                .child(
                                    Button::new("cfg-library-import")
                                        .icon(Icon::new(IconName::FolderOpen).size_3())
                                        .label("Import")
                                        .ghost()
                                        .small()
                                        .compact()
                                        .disabled(data.is_busy)
                                        .on_click({
                                            let app = app_handle.clone();
                                            move |_, window, cx| {
                                                app.update(cx, |this, cx| {
                                                    this.command_import_config(window, cx);
                                                });
                                            }
                                        }),
                                )
                                .child(
                                    Button::new("cfg-library-paste")
                                        .icon(Icon::new(IconName::Plus).size_3())
                                        .label("Paste")
                                        .ghost()
                                        .small()
                                        .compact()
                                        .disabled(data.is_busy)
                                        .on_click({
                                            let app = app_handle.clone();
                                            let workspace = workspace.clone();
                                            move |_, window, cx| {
                                                app.update(cx, |this, cx| {
                                                    this.command_paste_config(window, cx);
                                                });
                                                if compact {
                                                    workspace.update(cx, |workspace, cx| {
                                                        if workspace.set_primary_pane(
                                                            ConfigsPrimaryPane::Editor,
                                                        ) {
                                                            cx.notify();
                                                        }
                                                    });
                                                }
                                            }
                                        }),
                                ),
                        )
                        .when(
                            !data.has_saved_source && !data.draft.name.is_empty(),
                            |this| {
                                this.child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child("Unsaved draft in editor."),
                                )
                            },
                        )
                        .when(query.is_empty() && !rows.is_empty(), |this| {
                            this.child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("All profiles"),
                            )
                        }),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h(px(0.0))
                .overflow_hidden()
                .child(if rows.is_empty() {
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .px_3()
                        .py_6()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("No configs yet. Import a file or start a new draft.")
                        .into_any_element()
                } else {
                    div()
                        .relative()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_h(px(0.0))
                        .px(px(if compact { 8.0 } else { 10.0 }))
                        .py(px(if compact { 6.0 } else { 10.0 }))
                        .child(list)
                        .child(Scrollbar::vertical(&scroll_handle))
                        .into_any_element()
                }),
        )
}

fn render_library_row(
    app_handle: &Entity<WgApp>,
    workspace: &Entity<ConfigsWorkspace>,
    selected_id: Option<u64>,
    row: &ConfigsLibraryRow,
    compact: bool,
    cx: &mut App,
) -> Stateful<Div> {
    let is_selected = selected_id == Some(row.id);

    let bg = if is_selected {
        cx.theme().sidebar_accent.alpha(0.96)
    } else {
        cx.theme().background.alpha(0.0)
    };
    let border = if is_selected {
        cx.theme().sidebar_primary.alpha(0.36)
    } else {
        cx.theme().background.alpha(0.0)
    };
    let accent = if is_selected {
        cx.theme().sidebar_primary
    } else {
        cx.theme().background.alpha(0.0)
    };
    let hover_bg = if is_selected {
        cx.theme().sidebar_accent.opacity(0.9)
    } else {
        cx.theme().list_hover
    };
    let title_color = if is_selected {
        cx.theme().sidebar_accent_foreground
    } else {
        cx.theme().foreground
    };
    let subtitle_color = if is_selected {
        cx.theme().sidebar_accent_foreground.opacity(0.82)
    } else {
        cx.theme().muted_foreground
    };

    let config_id = row.id;

    div()
        .id(("configs-library-row", config_id))
        .flex()
        .items_start()
        .gap_2()
        .px(px(10.0))
        .py(px(7.0))
        .h(px(CONFIGS_LIBRARY_ROW_HEIGHT))
        .rounded(px(11.0))
        .border_1()
        .border_color(border)
        .bg(bg)
        .cursor_pointer()
        .hover(move |this| this.bg(hover_bg))
        .child(
            div()
                .w(px(2.0))
                .h_4()
                .mt(px(10.0))
                .rounded_full()
                .bg(accent),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .justify_center()
                .gap_1()
                .child(
                    h_flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .font_semibold()
                                .text_color(title_color)
                                .truncate()
                                .child(row.name.clone()),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .gap_1()
                                .when(row.is_running, |this| {
                                    this.child(Tag::success().xsmall().child("Running"))
                                })
                                .when(row.is_dirty, |this| {
                                    this.child(Tag::warning().xsmall().child("Dirty"))
                                }),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(subtitle_color)
                        .truncate()
                        .child(row.subtitle.clone()),
                )
                .when(is_selected || compact, |this| {
                    this.child(
                        h_flex()
                            .items_center()
                            .gap_1()
                            .flex_wrap()
                            .child(source_tag(&row.source))
                            .when(row.endpoint_family != EndpointFamily::Unknown, |this| {
                                this.child(endpoint_family_tag(row.endpoint_family))
                            }),
                    )
                }),
        )
        .on_click({
            let app = app_handle.clone();
            let workspace = workspace.clone();
            move |_, window, cx| {
                app.update(cx, |this, cx| {
                    if this.configs_is_busy(cx) {
                        return;
                    }
                    this.select_tunnel(config_id, window, cx);
                });
                if compact {
                    workspace.update(cx, |workspace, cx| {
                        if workspace.set_primary_pane(ConfigsPrimaryPane::Editor) {
                            cx.notify();
                        }
                    });
                }
            }
        })
}
