use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::{uniform_list, InteractiveElement as _, StatefulInteractiveElement as _, *};
use gpui_component::{
    button::{Button, ButtonVariants},
    description_list::DescriptionList,
    h_flex,
    input::{Input, InputState},
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
    resizable::{h_resizable, resizable_panel, ResizableState},
    scroll::{ScrollableElement, Scrollbar},
    tag::Tag,
    v_flex, ActiveTheme as _, Disableable as _, Icon, IconName, PixelsExt, Sizable as _,
    StyledExt as _,
};

use super::super::format::{format_addresses, format_allowed_ips, format_dns, format_route_table};
use super::super::state::{
    ConfigInspectorTab, ConfigSource, ConfigsLibraryRow, ConfigsWorkspace, DraftValidationState,
    EndpointFamily, WgApp,
};
use super::data::ConfigsViewData;
use super::widgets::status_badge;

const CONFIGS_COMPACT_BREAKPOINT: f32 = 1260.0;
const CONFIGS_LIBRARY_ROW_HEIGHT: f32 = 70.0;
const CONFIGS_LIBRARY_SCROLL_STATE_ID: &str = "configs-library-scroll";

struct ConfigsRuntimeView {
    selected_id: Option<u64>,
    latest_status: String,
    last_error: String,
    running_name: String,
}

impl WgApp {
    pub(crate) fn ensure_configs_workspace(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Entity<ConfigsWorkspace> {
        if let Some(workspace) = self.ui.configs_workspace.clone() {
            return workspace;
        }
        let app = cx.entity();
        let workspace = cx.new(|_| ConfigsWorkspace::new(app));
        self.ui.configs_workspace = Some(workspace.clone());
        workspace
    }
}

impl Render for ConfigsWorkspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let workspace_handle = cx.entity();
        self.ensure_inputs(window, cx);
        let app_handle = self.app.clone();
        let (data, runtime) = {
            let app = app_handle.read(cx);
            self.sync_from_app(&app);
            (
                ConfigsViewData::from_editor(
                    &app,
                    self.draft.clone(),
                    self.operation.clone(),
                    self.has_selection,
                ),
                ConfigsRuntimeView {
                    selected_id: app.selection.selected_id,
                    latest_status: app.ui.status.to_string(),
                    last_error: app
                        .ui
                        .last_error
                        .clone()
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "None".to_string()),
                    running_name: app
                        .runtime
                        .running_name
                        .clone()
                        .unwrap_or_else(|| "-".to_string()),
                },
            )
        };
        let name_input = self
            .name_input
            .clone()
            .expect("name input should be initialized");
        let library_search_input = self
            .library_search_input
            .clone()
            .expect("library search input should be initialized");
        let config_input = self
            .config_input
            .clone()
            .expect("config input should be initialized");

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .child(render_configs_page(
                &app_handle,
                &workspace_handle,
                &runtime,
                self.inspector_tab,
                &self.library_rows,
                &library_search_input,
                self.library_width,
                self.inspector_width,
                &data,
                &name_input,
                &config_input,
                window,
                cx,
            ))
    }
}

fn render_configs_page(
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
) -> Div {
    let compact = window.viewport_size().width < px(CONFIGS_COMPACT_BREAKPOINT);
    let workspace = if compact {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .flex_1()
            .min_h(px(0.0))
            .p_3()
            .child(render_library_panel(
                app_handle,
                runtime.selected_id,
                data,
                workspace,
                library_rows,
                library_search_input,
                window,
                cx,
            ))
            .child(render_editor_panel(app_handle, data, name_input, config_input, cx))
            .child(render_inspector_panel(
                runtime,
                workspace,
                inspector_tab,
                compact,
                data,
                cx,
            ))
            .into_any_element()
    } else {
        div()
            .flex_1()
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
                            let _ = app.update(cx, |app, cx| {
                                let changed = app.persist_configs_panel_widths(
                                    library_width,
                                    inspector_width,
                                    cx,
                                );
                                if let Some(workspace) = app.ui.configs_workspace.clone() {
                                    let _ = workspace.update(cx, |workspace, cx| {
                                        if workspace
                                            .set_panel_widths(library_width, inspector_width)
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
                            .size_range(px(240.0)..px(420.0))
                            .child(div().h_full().p_3().child(render_library_panel(
                                app_handle,
                                runtime.selected_id,
                                data,
                                workspace,
                                library_rows,
                                library_search_input,
                                window,
                                cx,
                            ))),
                    )
                    .child(resizable_panel().size_range(px(420.0)..Pixels::MAX).child(
                        div().h_full().p_3().child(render_editor_panel(
                            app_handle,
                            data,
                            name_input,
                            config_input,
                            cx,
                        )),
                    ))
                    .child(
                        resizable_panel()
                            .size(px(inspector_width))
                            .size_range(px(280.0)..px(440.0))
                            .child(div().h_full().p_3().child(render_inspector_panel(
                                runtime,
                                workspace,
                                inspector_tab,
                                compact,
                                data,
                                cx,
                            ))),
                    ),
            )
            .into_any_element()
    };

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.0))
        .rounded_lg()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().tiles)
        .overflow_hidden()
        .child(render_configs_shell_header(data, cx))
        .child(workspace)
}

fn render_configs_shell_header(
    data: &ConfigsViewData,
    cx: &mut Context<ConfigsWorkspace>,
) -> Div {
    let selected_name = data.title.clone();

    div()
        .px_6()
        .py_5()
        .min_h(px(84.0))
        .border_b_1()
        .border_color(cx.theme().border)
        .bg(linear_gradient(
            135.0,
            linear_color_stop(cx.theme().background.alpha(0.98), 0.0),
            linear_color_stop(cx.theme().muted.alpha(0.72), 1.0),
        ))
        .child(
            h_flex()
                .items_start()
                .justify_between()
                .flex_wrap()
                .gap_4()
                .child(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_xs()
                                .font_semibold()
                                .text_color(cx.theme().muted_foreground)
                                .child("CONFIGURATION"),
                        )
                        .child(div().text_xl().font_semibold().child("Configs"))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(
                                "Edit, validate, and manage tunnel profiles from one workspace.",
                            ),
                        ),
                )
                .child(
                    h_flex()
                        .items_start()
                        .flex_wrap()
                        .gap_2()
                        .child(Tag::secondary().small().rounded_full().child(selected_name))
                        .child(status_badge(data.shared.config_status.as_ref()))
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

fn render_library_panel(
    app_handle: &Entity<WgApp>,
    selected_id: Option<u64>,
    data: &ConfigsViewData,
    _workspace: &Entity<ConfigsWorkspace>,
    rows: &Arc<Vec<ConfigsLibraryRow>>,
    search_input: &Entity<InputState>,
    window: &mut Window,
    cx: &mut Context<ConfigsWorkspace>,
) -> Div {
    let query = search_input.read(cx).value().trim().to_lowercase();
    let total_count = rows.len();
    let filtered_indices = Arc::new(
        rows.iter()
            .enumerate()
            .filter_map(|(ix, row)| {
                let family_label = match row.endpoint_family {
                    EndpointFamily::V4 => "ipv4",
                    EndpointFamily::V6 => "ipv6",
                    EndpointFamily::Dual => "dual",
                    EndpointFamily::Unknown => "unknown",
                };
                let source_label = match row.source {
                    ConfigSource::File { .. } => "imported",
                    ConfigSource::Paste => "saved",
                };
                let matches_query = query.is_empty()
                    || row.name.to_lowercase().contains(&query)
                    || row.subtitle.to_lowercase().contains(&query)
                    || family_label.contains(&query)
                    || source_label.contains(&query);
                matches_query.then_some(ix)
            })
            .collect::<Vec<_>>(),
    );
    let count = filtered_indices.len();
    let rows_for_list = rows.clone();
    let visible_indices = filtered_indices.clone();
    let app_for_list = app_handle.clone();
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
                    render_library_row(&app_for_list, selected_id, &rows_for_list[row_ix], cx)
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
        .bg(cx.theme().background.alpha(0.76))
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
                                .child(
                                    Tag::secondary().small().child(if query.is_empty() {
                                        format!("{count} configs")
                                    } else {
                                        format!("{count}/{total_count} configs")
                                    }),
                                ),
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
                                            move |_, window, cx| {
                                                let _ = app.update(cx, |this, cx| {
                                                    this.handle_new_draft_click(window, cx);
                                                });
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
                                                let _ = app.update(cx, |this, cx| {
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
                                            move |_, window, cx| {
                                                let _ = app.update(cx, |this, cx| {
                                                    this.handle_paste_click(window, cx);
                                                });
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
    selected_id: Option<u64>,
    row: &ConfigsLibraryRow,
    cx: &mut App,
) -> Stateful<Div> {
    let is_selected = selected_id == Some(row.id);

    let bg = if is_selected {
        cx.theme().accent.alpha(0.10)
    } else {
        cx.theme().background.alpha(0.0)
    };
    let accent = if is_selected {
        cx.theme().accent
    } else {
        cx.theme().background.alpha(0.0)
    };

    let config_id = row.id;

    div()
        .id(("configs-library-row", config_id))
        .flex()
        .items_start()
        .gap_3()
        .px_2()
        .py_1()
        .h(px(CONFIGS_LIBRARY_ROW_HEIGHT))
        .rounded_md()
        .bg(bg)
        .cursor_pointer()
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
                .child(
                    h_flex()
                        .items_center()
                        .gap_1()
                        .flex_wrap()
                        .child(source_tag(&row.source))
                        .when(row.endpoint_family != EndpointFamily::Unknown, |this| {
                            this.child(endpoint_family_tag(row.endpoint_family))
                        }),
                ),
        )
        .on_click({
            let app = app_handle.clone();
            move |_, window, cx| {
                let _ = app.update(cx, |this, cx| {
                    if this.configs_is_busy(cx) {
                        return;
                    }
                    this.select_tunnel(config_id, window, cx);
                });
            }
        })
}

fn render_editor_panel(
    app_handle: &Entity<WgApp>,
    data: &ConfigsViewData,
    name_input: &Entity<InputState>,
    config_input: &Entity<InputState>,
    cx: &mut Context<ConfigsWorkspace>,
) -> Div {
    div()
        .flex()
        .flex_col()
        .h_full()
        .min_h(px(0.0))
        .rounded_lg()
        .border_1()
        .border_color(cx.theme().border.alpha(0.9))
        .bg(cx.theme().background.alpha(0.82))
        .child(
            div()
                .px_6()
                .py_5()
                .border_b_1()
                .border_color(cx.theme().border.alpha(0.7))
                .child(
                    v_flex()
                        .gap_4()
                        .child(
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
                                                .child("CONFIG FILE"),
                                        )
                                        .child(
                                            div()
                                                .max_w_full()
                                                .px_4()
                                                .py_3()
                                                .rounded_xl()
                                                .border_1()
                                                .border_color(cx.theme().border.alpha(0.45))
                                                .bg(cx.theme().background.alpha(0.72))
                                                .child(
                                                    Input::new(name_input)
                                                        .appearance(false)
                                                        .bordered(false),
                                                ),
                                        )
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
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .gap_1()
                                .flex_wrap()
                                .when(!data.has_saved_source, |this| {
                                    this.child(
                                        Tag::secondary().small().rounded_full().child("Unsaved draft"),
                                    )
                                })
                                .child(if data.shared.draft_dirty {
                                    Tag::warning().small().rounded_full().child("Dirty")
                                } else {
                                    Tag::secondary().small().rounded_full().child("Saved")
                                })
                                .child(match &data.draft.validation {
                                    DraftValidationState::Valid { .. } => {
                                        Tag::success().small().rounded_full().child("Valid")
                                    }
                                    DraftValidationState::Invalid { .. } => {
                                        Tag::danger().small().rounded_full().child("Invalid")
                                    }
                                    DraftValidationState::Idle => Tag::secondary()
                                        .small()
                                        .rounded_full()
                                        .child("Draft"),
                                })
                                .when(data.shared.needs_restart, |this| {
                                    this.child(
                                        Tag::warning()
                                            .small()
                                            .rounded_full()
                                            .child("Needs restart"),
                                    )
                                })
                                .when(data.is_running_draft, |this| {
                                    this.child(
                                        Tag::success().small().rounded_full().child("Running"),
                                    )
                                }),
                        )
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_3()
                .flex_1()
                .min_h(px(0.0))
                .p_5()
                .child(render_diagnostics_strip(data, cx))
                .child(
                    div()
                        .px_1()
                        .text_xs()
                        .font_semibold()
                        .text_color(cx.theme().muted_foreground)
                        .child("CONFIG"),
                )
                .child(
                    div()
                        .w_full()
                        .flex_grow()
                        .min_h(px(460.0))
                        .rounded_2xl()
                        .border_1()
                        .border_color(cx.theme().border.alpha(0.85))
                        .bg(cx.theme().background.alpha(0.94))
                        .p_3()
                        .shadow_sm()
                        .child(
                            Input::new(config_input)
                                .appearance(false)
                                .bordered(false)
                                .h_full(),
                        ),
                ),
        )
}

fn render_inspector_panel(
    runtime: &ConfigsRuntimeView,
    workspace: &Entity<ConfigsWorkspace>,
    inspector_tab: ConfigInspectorTab,
    compact: bool,
    data: &ConfigsViewData,
    cx: &mut Context<ConfigsWorkspace>,
) -> Div {
    let preview_card = {
        let addresses = data
            .shared
            .parsed_config
            .as_ref()
            .map(|cfg| format_addresses(&cfg.interface))
            .unwrap_or_else(|| "-".to_string());
        let dns = data
            .shared
            .parsed_config
            .as_ref()
            .map(|cfg| format_dns(&cfg.interface))
            .unwrap_or_else(|| "-".to_string());
        let route_table = data
            .shared
            .parsed_config
            .as_ref()
            .map(|cfg| format_route_table(cfg.interface.table))
            .unwrap_or_else(|| "-".to_string());
        let routes = data
            .shared
            .parsed_config
            .as_ref()
            .map(|cfg| format_allowed_ips(&cfg.peers))
            .unwrap_or_else(|| "-".to_string());
        let peers = data
            .shared
            .parsed_config
            .as_ref()
            .map(|cfg| cfg.peers.len().to_string())
            .unwrap_or_else(|| "0".to_string());
        let source = data.source_summary.clone();

        inspector_card(
            "Preview",
            "Parsed config summary",
            DescriptionList::new()
                .columns(1)
                .item("Source", source, 1)
                .item("Local Address", addresses, 1)
                .item("DNS", dns, 1)
                .item("Route Table", route_table, 1)
                .item("Allowed IPs", routes, 1)
                .item("Peers", peers, 1),
            cx,
        )
    };

    let diagnostics_card = {
        let (state, line, message) = match &data.draft.validation {
            DraftValidationState::Idle => (
                "Idle".to_string(),
                "-".to_string(),
                "Start editing to validate this config.".to_string(),
            ),
            DraftValidationState::Valid { .. } => (
                "Valid".to_string(),
                "-".to_string(),
                "Config parses successfully.".to_string(),
            ),
            DraftValidationState::Invalid { line, message, .. } => (
                "Invalid".to_string(),
                line.map(|line| line.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                message.to_string(),
            ),
        };

        inspector_card(
            "Diagnostics",
            "Validation and save state",
            v_flex()
                .gap_3()
                .child(
                    h_flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(div().text_sm().font_semibold().child(state))
                        .child(
                            h_flex()
                                .items_center()
                                .gap_1()
                                .when(data.shared.draft_dirty, |this| {
                                    this.child(Tag::warning().small().child("Unsaved"))
                                })
                                .when(data.shared.needs_restart, |this| {
                                    this.child(Tag::warning().small().child("Restart"))
                                })
                                .when(data.is_running_draft, |this| {
                                    this.child(Tag::success().small().child("Running"))
                                }),
                        ),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(message),
                )
                .child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .flex_wrap()
                        .child(Tag::secondary().small().child(format!("Line {line}")))
                        .child(
                            Tag::secondary().small().child(if data.shared.draft_dirty {
                                "Unsaved changes"
                            } else {
                                "Saved"
                            }),
                        ),
                ),
            cx,
        )
    };

    let activity_card = inspector_card(
        "Activity",
        "Recent runtime notes",
        v_flex()
            .gap_3()
            .child(inspector_activity_row("Latest Status", runtime.latest_status.clone(), cx))
            .child(inspector_activity_row("Last Error", runtime.last_error.clone(), cx))
            .child(inspector_activity_row(
                "Running Tunnel",
                runtime.running_name.clone(),
                cx,
            ))
            .child(inspector_activity_row(
                "Handshake",
                data.shared.last_handshake.clone(),
                cx,
            )),
        cx,
    );

    let inspector_tabs = inspector_tab_row(workspace, inspector_tab, cx);
    let inspector_body = if compact {
        match inspector_tab {
            ConfigInspectorTab::Preview => preview_card.into_any_element(),
            ConfigInspectorTab::Diagnostics => diagnostics_card.into_any_element(),
            ConfigInspectorTab::Activity => activity_card.into_any_element(),
        }
    } else {
        v_flex()
            .gap_3()
            .child(preview_card)
            .child(diagnostics_card)
            .child(activity_card)
            .into_any_element()
    };

    div()
        .flex()
        .flex_col()
        .gap_3()
        .h_full()
        .min_h(px(0.0))
        .rounded_lg()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().background.alpha(0.76))
        .child(
            div()
                .px_4()
                .py_4()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(
                    v_flex()
                        .gap_1()
                        .child(div().text_lg().font_semibold().child("Inspector"))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child("Preview parsed config, validation, and runtime notes."),
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
                .overflow_y_scrollbar()
                .p_3()
                .when(compact, |this| this.child(inspector_tabs))
                .child(inspector_body),
        )
}

fn inspector_tab_row(
    workspace: &Entity<ConfigsWorkspace>,
    inspector_tab: ConfigInspectorTab,
    cx: &mut Context<ConfigsWorkspace>,
) -> Div {
    h_flex()
        .items_end()
        .gap_4()
        .w_full()
        .border_b_1()
        .border_color(cx.theme().border.alpha(0.6))
        .child(inspector_tab_button(
            "inspector-preview",
            "Preview",
            ConfigInspectorTab::Preview,
            inspector_tab,
            workspace,
            cx,
        ))
        .child(inspector_tab_button(
            "inspector-diagnostics",
            "Diagnostics",
            ConfigInspectorTab::Diagnostics,
            inspector_tab,
            workspace,
            cx,
        ))
        .child(inspector_tab_button(
            "inspector-activity",
            "Activity",
            ConfigInspectorTab::Activity,
            inspector_tab,
            workspace,
            cx,
        ))
}

fn inspector_tab_button(
    id: &'static str,
    label: &'static str,
    value: ConfigInspectorTab,
    current: ConfigInspectorTab,
    workspace: &Entity<ConfigsWorkspace>,
    cx: &mut Context<ConfigsWorkspace>,
) -> Stateful<Div> {
    let selected = current == value;
    let text_color = if selected {
        cx.theme().foreground
    } else {
        cx.theme().muted_foreground
    };
    let underline = if selected {
        cx.theme().accent
    } else {
        cx.theme().background.alpha(0.0)
    };

    div()
        .id(id)
        .flex()
        .flex_col()
        .items_center()
        .gap_2()
        .pt_1()
        .pb_0p5()
        .cursor_pointer()
        .child(
            div()
                .text_sm()
                .font_weight(if selected {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::MEDIUM
                })
                .text_color(text_color)
                .child(label),
        )
        .child(div().w_full().h(px(2.0)).rounded_full().bg(underline))
        .on_click({
            let workspace = workspace.clone();
            move |_, _, cx| {
                let _ = workspace.update(cx, |workspace, cx| {
                    let changed = workspace.set_inspector_tab(value);
                    let _ = workspace.app.update(cx, |app, cx| {
                        app.persist_preferred_inspector_tab(value, cx);
                    });
                    if changed {
                        cx.notify();
                    }
                });
            }
        })
}

fn render_diagnostics_strip(data: &ConfigsViewData, cx: &mut Context<ConfigsWorkspace>) -> Div {
    let (tone_bg, tone_border, tone_bar, title, detail, icon) = match &data.draft.validation {
        DraftValidationState::Idle => (
            cx.theme().secondary.alpha(0.45),
            cx.theme().border.alpha(0.45),
            cx.theme().muted_foreground.alpha(0.5),
            "Draft idle".to_string(),
            "Start editing to validate this config.".to_string(),
            IconName::Info,
        ),
        DraftValidationState::Valid { .. } => (
            cx.theme().accent.alpha(0.12),
            cx.theme().accent.alpha(0.3),
            cx.theme().accent,
            if data.shared.draft_dirty {
                "Valid draft".to_string()
            } else {
                "Saved and valid".to_string()
            },
            if data.shared.needs_restart {
                "Saved changes require a tunnel restart to take effect.".to_string()
            } else if data.shared.draft_dirty {
                "No syntax issues. Save this draft to apply the changes.".to_string()
            } else {
                "WireGuard config parsed successfully.".to_string()
            },
            IconName::CircleCheck,
        ),
        DraftValidationState::Invalid { line, message, .. } => (
            cx.theme().danger.alpha(0.08),
            cx.theme().danger.alpha(0.3),
            cx.theme().danger,
            "Validation error".to_string(),
            match line {
                Some(line) => format!("Line {line}: {message}"),
                None => message.to_string(),
            },
            IconName::CircleX,
        ),
    };

    div()
        .flex()
        .items_start()
        .gap_3()
        .px_0()
        .py_0()
        .rounded_lg()
        .border_1()
        .border_color(tone_border)
        .bg(tone_bg)
        .child(div().w(px(3.0)).h_full().rounded_lg().bg(tone_bar))
        .child(
            h_flex()
                .items_start()
                .justify_between()
                .gap_3()
                .flex_1()
                .px_3()
                .py_3()
                .child(
                    h_flex()
                        .items_start()
                        .gap_3()
                        .child(
                            div()
                                .mt(px(1.0))
                                .child(Icon::new(icon).size_4().text_color(tone_bar)),
                        )
                        .child(
                            v_flex()
                                .gap_1()
                                .child(div().text_xs().font_semibold().child(title))
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(detail),
                                ),
                        ),
                )
                .child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .flex_wrap()
                        .when(data.shared.draft_dirty, |this| {
                            this.child(Tag::warning().small().child("Unsaved"))
                        })
                        .when(data.shared.needs_restart, |this| {
                            this.child(Tag::warning().small().child("Restart"))
                        })
                        .when(
                            matches!(data.draft.validation, DraftValidationState::Valid { .. }),
                            |this| this.child(Tag::success().small().child("Ready")),
                        )
                ),
        )
}

fn editor_action_bar(
    data: &ConfigsViewData,
    app_handle: &Entity<WgApp>,
    _cx: &mut Context<ConfigsWorkspace>,
) -> Div {
    let manage_button = if data.is_busy || !data.has_saved_source || !data.has_selection {
        Button::new("cfg-manage")
            .icon(Icon::new(IconName::Menu).size_3())
            .label("Manage")
            .ghost()
            .small()
            .compact()
            .disabled(true)
            .into_any_element()
    } else {
        let menu_handle = app_handle.clone();
        Button::new("cfg-manage")
            .icon(Icon::new(IconName::Menu).size_3())
            .label("Manage")
            .ghost()
            .small()
            .compact()
            .dropdown_caret(true)
            .dropdown_menu_with_anchor(Corner::TopRight, move |menu: PopupMenu, _, _| {
                let rename_handle = menu_handle.clone();
                let export_handle = menu_handle.clone();
                let copy_handle = menu_handle.clone();
                let delete_handle = menu_handle.clone();
                menu.item(PopupMenuItem::new("Rename").on_click({
                    move |_, window, cx| {
                        let _ = rename_handle.update(cx, |this, cx| {
                            this.handle_rename_click(window, cx);
                        });
                    }
                }))
                .item(PopupMenuItem::new("Export").on_click({
                    move |_, _, cx| {
                        let _ = export_handle.update(cx, |this, cx| {
                            this.handle_export_click(cx);
                        });
                    }
                }))
                .item(PopupMenuItem::new("Copy").on_click({
                    move |_, _, cx| {
                        let _ = copy_handle.update(cx, |this, cx| {
                            this.handle_copy_click(cx);
                        });
                    }
                }))
                .item(PopupMenuItem::separator())
                .item(PopupMenuItem::new("Delete").on_click({
                    move |_, window, cx| {
                        let _ = delete_handle.update(cx, |this, cx| {
                            this.handle_delete_click(window, cx);
                        });
                    }
                }))
            })
            .into_any_element()
    };

    h_flex()
        .items_center()
        .gap_2()
        .flex_wrap()
        .child(
            Button::new("cfg-save")
                .icon(Icon::new(IconName::Check).size_3())
                .label("Save")
                .success()
                .small()
                .compact()
                .disabled(!data.can_save)
                .on_click({
                    let app = app_handle.clone();
                    move |_, window, cx| {
                        let _ = app.update(cx, |this, cx| {
                            this.handle_save_click(window, cx);
                        });
                    }
                }),
        )
        .child(
            Button::new("cfg-save-as")
                .icon(Icon::new(IconName::Copy).size_3())
                .label("Save as new")
                .ghost()
                .small()
                .compact()
                .disabled(data.is_busy)
                .on_click({
                    let app = app_handle.clone();
                    move |_, window, cx| {
                        let _ = app.update(cx, |this, cx| {
                            this.handle_save_as_click(window, cx);
                        });
                    }
                }),
        )
        .when(data.can_restart, |this| {
            this.child(
                Button::new("cfg-save-restart")
                    .icon(Icon::new(IconName::Redo2).size_3())
                    .label("Save & Restart")
                    .ghost()
                    .small()
                    .compact()
                    .on_click({
                        let app = app_handle.clone();
                        move |_, window, cx| {
                            let _ = app.update(cx, |this, cx| {
                                this.handle_save_and_restart_click(window, cx);
                            });
                        }
                    }),
            )
        })
        .child(manage_button)
}

fn inspector_card<T: IntoElement>(
    title: &'static str,
    subtitle: &'static str,
    body: T,
    cx: &mut Context<ConfigsWorkspace>,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .rounded_lg()
        .border_1()
        .border_color(cx.theme().border.alpha(0.5))
        .bg(cx.theme().background.alpha(0.52))
        .p_3()
        .child(
            v_flex()
                .gap_1()
                .child(div().text_sm().font_semibold().child(title))
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(subtitle),
                ),
        )
        .child(body)
}

fn inspector_activity_row(
    label: &'static str,
    value: String,
    cx: &mut Context<ConfigsWorkspace>,
) -> Div {
    div()
        .flex()
        .items_start()
        .justify_between()
        .gap_3()
        .pb_3()
        .border_b_1()
        .border_color(cx.theme().border.alpha(0.5))
        .child(
            div()
                .text_xs()
                .font_semibold()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(
            div()
                .text_sm()
                .text_right()
                .max_w(px(180.0))
                .child(value),
        )
}

fn source_tag(source: &ConfigSource) -> Tag {
    match source {
        ConfigSource::File { .. } => Tag::secondary().small().child("Imported"),
        ConfigSource::Paste => Tag::secondary().small().child("Saved"),
    }
}

fn endpoint_family_tag(family: EndpointFamily) -> Tag {
    match family {
        EndpointFamily::V4 => Tag::secondary().small().child("IPv4"),
        EndpointFamily::V6 => Tag::secondary().small().child("IPv6"),
        EndpointFamily::Dual => Tag::secondary().small().child("Dual"),
        EndpointFamily::Unknown => Tag::secondary().small().child("Unknown"),
    }
}
