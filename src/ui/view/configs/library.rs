// Library search, list, and row rendering.

#[allow(clippy::too_many_arguments)]
fn render_library_panel(
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
        .h_full()
        .min_h(px(0.0))
        .rounded_lg()
        .border_1()
        .border_color(cx.theme().border)
        .bg(if compact {
            cx.theme().background
        } else {
            cx.theme().background.alpha(0.76)
        })
        .child(
            div()
                .px_4()
                .py_4()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(
                    v_flex()
                        .gap_2()
                        .child(
                            h_flex()
                                .items_center()
                                .justify_between()
                                .child(div().text_lg().font_semibold().child("Library"))
                                .child(Tag::secondary().small().child(if query.is_empty() {
                                    format!("{count} configs")
                                } else {
                                    format!("{count}/{total_count} configs")
                                })),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("Saved and imported tunnel profiles."),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .gap_2()
                                .px_2()
                                .py_1()
                                .rounded_md()
                                .bg(cx.theme().secondary.alpha(0.88))
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
                                .gap_2()
                                .flex_wrap()
                                .child(
                                    Button::new("cfg-new")
                                        .icon(Icon::new(IconName::Plus).size_3())
                                        .label("New")
                                        .outline()
                                        .small()
                                        .compact()
                                        .disabled(data.is_busy)
                                        .on_click({
                                            let app = app_handle.clone();
                                            let workspace = workspace.clone();
                                            move |_, window, cx| {
                                                app.update(cx, |this, cx| {
                                                    this.handle_new_draft_click(window, cx);
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
                                        .outline()
                                        .small()
                                        .compact()
                                        .disabled(data.is_busy)
                                        .on_click({
                                            let app = app_handle.clone();
                                            move |_, window, cx| {
                                                app.update(cx, |this, cx| {
                                                    this.handle_import_click(window, cx);
                                                });
                                            }
                                        }),
                                )
                                .child(
                                    Button::new("cfg-library-paste")
                                        .icon(Icon::new(IconName::Plus).size_3())
                                        .label("Paste")
                                        .outline()
                                        .small()
                                        .compact()
                                        .disabled(data.is_busy)
                                        .on_click({
                                            let app = app_handle.clone();
                                            let workspace = workspace.clone();
                                            move |_, window, cx| {
                                                app.update(cx, |this, cx| {
                                                    this.handle_paste_click(window, cx);
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
                                        .child("Working on an unsaved draft."),
                                )
                            },
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h(px(0.0))
                .rounded_md()
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
                        .px_2()
                        .py_1()
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
        cx.theme().list_active
    } else {
        cx.theme().background.alpha(0.0)
    };
    let border = if is_selected {
        cx.theme().list_active_border
    } else {
        cx.theme().background.alpha(0.0)
    };
    let accent = if is_selected {
        cx.theme().list_active_border
    } else {
        cx.theme().background.alpha(0.0)
    };
    let hover_bg = if is_selected {
        cx.theme().list_active
    } else {
        cx.theme().list_hover
    };

    let config_id = row.id;

    div()
        .id(("configs-library-row", config_id))
        .flex()
        .items_start()
        .gap_3()
        .px_3()
        .py_2()
        .h(px(CONFIGS_LIBRARY_ROW_HEIGHT))
        .rounded_lg()
        .border_1()
        .border_color(border)
        .bg(bg)
        .cursor_pointer()
        .hover(move |this| this.bg(hover_bg))
        .child(div().w(px(3.0)).h_full().rounded_full().bg(accent))
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
                                .truncate()
                                .child(row.name.clone()),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .gap_1()
                                .when(row.is_running, |this| {
                                    this.child(Tag::success().small().child("Running"))
                                })
                                .when(row.is_dirty, |this| {
                                    this.child(Tag::warning().small().child("Dirty"))
                                }),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .truncate()
                        .child(row.subtitle.clone()),
                )
                .when(is_selected, |this| {
                    this.child(
                        h_flex()
                            .items_center()
                            .gap_2()
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

