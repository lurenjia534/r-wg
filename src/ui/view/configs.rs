use gpui::prelude::FluentBuilder as _;
use gpui::{uniform_list, InteractiveElement as _, StatefulInteractiveElement as _, *};
use gpui_component::{
    button::{Button, ButtonVariants},
    description_list::DescriptionList,
    group_box::{GroupBox, GroupBoxVariants},
    h_flex,
    input::{Input, InputState},
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
    resizable::{h_resizable, resizable_panel, ResizableState},
    scroll::{ScrollableElement, Scrollbar},
    tag::Tag,
    v_flex, ActiveTheme as _, Disableable as _, Icon, IconName, PixelsExt, Selectable as _,
    Sizable as _, StyledExt as _,
};

use super::super::format::{
    format_addresses, format_allowed_ips, format_dns, format_route_table,
};
use super::super::state::{
    ConfigInspectorTab, ConfigSource, ConfigsLibraryRow, ConfigsWorkspace, DraftValidationState,
    EndpointFamily, WgApp,
};
use super::data::ConfigsViewData;
use super::widgets::status_badge;

const CONFIGS_COMPACT_BREAKPOINT: f32 = 1260.0;
const CONFIGS_LIBRARY_ROW_HEIGHT: f32 = 86.0;
const CONFIGS_LIBRARY_SCROLL_STATE_ID: &str = "configs-library-scroll";

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
        {
            let app = app_handle.read(cx);
            self.sync_from_app(&app);
        }

        self.app.update(cx, |app, cx| {
            let data = ConfigsViewData::from_editor(
                app,
                self.draft.clone(),
                self.operation.clone(),
                self.has_selection,
            );
            let name_input = self
                .name_input
                .clone()
                .expect("name input should be initialized");
            let config_input = self
                .config_input
                .clone()
                .expect("config input should be initialized");

            div()
                .flex()
                .flex_col()
                .gap_3()
                .flex_1()
                .min_h(px(0.0))
                .child(super::top_bar::render_top_bar(app, &data.shared, cx))
                .child(render_configs_page(
                    app,
                    &workspace_handle,
                    self.inspector_tab,
                    &self.library_rows,
                    self.library_width,
                    self.inspector_width,
                    &data,
                    &name_input,
                    &config_input,
                    window,
                    cx,
                ))
        })
    }
}

pub(crate) fn render_configs_page(
    app: &mut WgApp,
    workspace: &Entity<ConfigsWorkspace>,
    inspector_tab: ConfigInspectorTab,
    library_rows: &[ConfigsLibraryRow],
    library_width: f32,
    inspector_width: f32,
    data: &ConfigsViewData,
    name_input: &Entity<InputState>,
    config_input: &Entity<InputState>,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) -> Div {
    let compact = window.viewport_size().width < px(CONFIGS_COMPACT_BREAKPOINT);
    let app_handle = cx.entity();
    let workspace = if compact {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .flex_1()
            .min_h(px(0.0))
            .p_3()
            .child(render_library_panel(app, data, workspace, library_rows, window, cx))
            .child(render_editor_panel(app, data, name_input, config_input, cx))
            .child(render_inspector_panel(app, workspace, inspector_tab, compact, data, cx))
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
                                        if workspace.set_panel_widths(
                                            library_width,
                                            inspector_width,
                                        ) {
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
                            .child(
                                div()
                                    .h_full()
                                    .p_3()
                                    .child(render_library_panel(
                                        app,
                                        data,
                                        workspace,
                                        library_rows,
                                        window,
                                        cx,
                                    )),
                            ),
                    )
                    .child(
                        resizable_panel()
                            .size_range(px(420.0)..Pixels::MAX)
                            .child(
                                div()
                                    .h_full()
                                    .p_3()
                                    .child(render_editor_panel(app, data, name_input, config_input, cx)),
                            ),
                    )
                    .child(
                        resizable_panel()
                            .size(px(inspector_width))
                            .size_range(px(280.0)..px(440.0))
                            .child(
                                div()
                                    .h_full()
                                    .p_3()
                                    .child(render_inspector_panel(
                                        app,
                                        workspace,
                                        inspector_tab,
                                        compact,
                                        data,
                                        cx,
                                    )),
                            ),
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
        .child(render_configs_shell_header(app, data, cx))
        .child(workspace)
}

fn render_configs_shell_header(
    _app: &WgApp,
    data: &ConfigsViewData,
    cx: &mut Context<WgApp>,
) -> Div {
    let selected_name = data.title.clone();

    div()
        .px_5()
        .py_4()
        .border_b_1()
        .border_color(cx.theme().border)
        .bg(linear_gradient(
            135.0,
            linear_color_stop(cx.theme().background, 0.0),
            linear_color_stop(cx.theme().muted.alpha(0.9), 1.0),
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
                        .items_center()
                        .flex_wrap()
                        .gap_2()
                        .child(Tag::secondary().small().rounded_full().child(selected_name))
                        .child(status_badge(data.shared.config_status.as_ref()))
                        .when(data.shared.draft_dirty, |this| {
                            this.child(Tag::warning().small().rounded_full().child("Dirty"))
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
                            this.child(Tag::success().small().rounded_full().child("Running"))
                        }),
                ),
        )
}

fn render_library_panel(
    _app: &WgApp,
    data: &ConfigsViewData,
    _workspace: &Entity<ConfigsWorkspace>,
    rows: &[ConfigsLibraryRow],
    window: &mut Window,
    cx: &mut Context<WgApp>,
) -> Div {
    let count = rows.len();
    let rows_for_list = rows.to_vec();
    let app_entity = cx.entity();
    let scroll_handle = window
        .use_keyed_state(CONFIGS_LIBRARY_SCROLL_STATE_ID, cx, |_, _| {
            UniformListScrollHandle::new()
        })
        .read(cx)
        .clone();
    let list = uniform_list(
        "configs-library-list",
        rows_for_list.len(),
        move |visible_range, _window, cx| {
            app_entity.update(cx, |this, cx| {
                visible_range
                    .map(|ix| render_library_row(this, &rows_for_list[ix], cx))
                    .collect::<Vec<_>>()
            })
        },
    )
    .track_scroll(scroll_handle.clone())
    .w_full()
    .flex_1();

    div()
        .flex()
        .flex_col()
        .gap_3()
        .h_full()
        .min_h(px(0.0))
        .rounded_lg()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().background.alpha(0.72))
        .child(
            div()
                .px_4()
                .py_3()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(
                    v_flex()
                        .gap_3()
                        .child(
                            h_flex()
                                .items_center()
                                .justify_between()
                                .child(div().text_lg().font_semibold().child("Library"))
                                .child(Tag::secondary().small().child(format!("{count} configs"))),
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
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.handle_new_draft_click(window, cx);
                                        })),
                                )
                                .child(
                                    Button::new("cfg-library-import")
                                        .icon(Icon::new(IconName::FolderOpen).size_3())
                                        .label("Import")
                                        .outline()
                                        .small()
                                        .compact()
                                        .disabled(data.is_busy)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.handle_import_click(window, cx);
                                        })),
                                )
                                .child(
                                    Button::new("cfg-library-paste")
                                        .icon(Icon::new(IconName::Plus).size_3())
                                        .label("Paste")
                                        .outline()
                                        .small()
                                        .compact()
                                        .disabled(data.is_busy)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.handle_paste_click(window, cx);
                                        })),
                                ),
                        )
                        .when(!data.has_saved_source && !data.draft.name.is_empty(), |this| {
                            this.child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Working on an unsaved draft."),
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
                        .p_2()
                        .child(list)
                        .child(Scrollbar::vertical(&scroll_handle))
                        .into_any_element()
                }),
        )
}

fn render_library_row(
    app: &WgApp,
    row: &ConfigsLibraryRow,
    cx: &mut Context<WgApp>,
) -> Stateful<Div> {
    let is_selected = app.selection.selected_id == Some(row.id);

    let bg = if is_selected {
        cx.theme().accent.alpha(0.14)
    } else {
        cx.theme().background
    };
    let border = if is_selected {
        cx.theme().accent.alpha(0.5)
    } else {
        cx.theme().border.alpha(0.35)
    };

    let config_id = row.id;

    div()
        .id(("configs-library-row", config_id))
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .h(px(CONFIGS_LIBRARY_ROW_HEIGHT))
        .rounded_lg()
        .border_1()
        .border_color(border)
        .bg(bg)
        .cursor_pointer()
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
                .when(row.is_running, |this| {
                    this.child(Tag::success().small().child("Running"))
                }),
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
                .child(endpoint_family_tag(row.endpoint_family))
                .when(row.is_dirty, |this| {
                    this.child(Tag::warning().small().child("Dirty"))
                }),
        )
        .on_click(cx.listener(move |this, _, window, cx| {
            if this.configs_is_busy(cx) {
                return;
            }
            this.select_tunnel(config_id, window, cx);
        }))
}

fn render_editor_panel(
    _app: &WgApp,
    data: &ConfigsViewData,
    name_input: &Entity<InputState>,
    config_input: &Entity<InputState>,
    cx: &mut Context<WgApp>,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .h_full()
        .min_h(px(0.0))
        .rounded_lg()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().background.alpha(0.72))
        .child(
            div()
                .px_4()
                .py_3()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(
                    v_flex()
                        .gap_3()
                        .child(
                            h_flex()
                                .items_start()
                                .justify_between()
                                .flex_wrap()
                                .gap_3()
                                .child(
                                    v_flex()
                                        .gap_1()
                                        .child(
                                            div()
                                                .text_lg()
                                                .font_semibold()
                                                .child(data.title.clone()),
                                        )
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(data.source_summary.clone()),
                                        ),
                                )
                                .child(editor_action_bar(data, cx)),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .gap_2()
                                .flex_wrap()
                                .child(
                                    Tag::secondary().small().rounded_full().child(if data.has_saved_source {
                                        "Saved config"
                                    } else {
                                        "Unsaved draft"
                                    }),
                                )
                                .child(
                                    if data.shared.draft_dirty {
                                        Tag::warning().small().rounded_full().child("Dirty")
                                    } else {
                                        Tag::secondary().small().rounded_full().child("Saved")
                                    },
                                )
                                .child(
                                    match &data.draft.validation {
                                        DraftValidationState::Valid { .. } => {
                                            Tag::success().small().rounded_full().child("Valid")
                                        }
                                        DraftValidationState::Invalid { .. } => {
                                            Tag::danger().small().rounded_full().child("Invalid")
                                        }
                                        DraftValidationState::Idle => {
                                            Tag::secondary().small().rounded_full().child("Draft")
                                        }
                                    },
                                )
                                .when(data.shared.needs_restart, |this| {
                                    this.child(
                                        Tag::warning()
                                            .small()
                                            .rounded_full()
                                            .child("Restart required"),
                                    )
                                }),
                        )
                        .child(
                            div()
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
                .p_3()
                .child(render_diagnostics_strip(data, cx))
                .child(
                    GroupBox::new().fill().title("Tunnel Name").child(
                        div()
                            .w_full()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(cx.theme().secondary)
                            .child(Input::new(name_input).appearance(false).bordered(false)),
                    ),
                )
                .child(
                    GroupBox::new()
                        .fill()
                        .title("Config")
                        .child(
                            div()
                                .w_full()
                                .flex_grow()
                                .min_h(px(420.0))
                                .p_2()
                                .rounded_md()
                                .bg(cx.theme().secondary)
                                .child(
                                    Input::new(config_input)
                                        .appearance(false)
                                        .bordered(false)
                                        .h_full(),
                                ),
                        )
                        .flex_grow(),
                ),
        )
}

fn render_inspector_panel(
    app: &WgApp,
    workspace: &Entity<ConfigsWorkspace>,
    inspector_tab: ConfigInspectorTab,
    compact: bool,
    data: &ConfigsViewData,
    cx: &mut Context<WgApp>,
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

        GroupBox::new().fill().title("Preview").child(
            DescriptionList::new()
                .columns(1)
                .item("Source", source, 1)
                .item("Local Address", addresses, 1)
                .item("DNS", dns, 1)
                .item("Route Table", route_table, 1)
                .item("Allowed IPs", routes, 1)
                .item("Peers", peers, 1),
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
                line.map(|line| line.to_string()).unwrap_or_else(|| "-".to_string()),
                message.to_string(),
            ),
        };

        GroupBox::new().fill().title("Diagnostics").child(
            DescriptionList::new()
                .columns(1)
                .item("Validation", state, 1)
                .item("Line", line, 1)
                .item("Message", message, 1)
                .item(
                    "Save State",
                    if data.shared.draft_dirty {
                        "Unsaved changes"
                    } else {
                        "Saved"
                    },
                    1,
                )
                .item(
                    "Runtime",
                    if data.shared.needs_restart {
                        "Restart required after save".to_string()
                    } else if data.is_running_draft {
                        "Running".to_string()
                    } else {
                        "Idle".to_string()
                    },
                    1,
                ),
        )
    };

    let activity_card = GroupBox::new().fill().title("Activity").child(
        DescriptionList::new()
            .columns(1)
            .item("Latest Status", app.ui.status.to_string(), 1)
            .item(
                "Last Error",
                app.ui
                    .last_error
                    .clone()
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "None".to_string()),
                1,
            )
            .item(
                "Running Tunnel",
                app.runtime
                    .running_name
                    .clone()
                    .unwrap_or_else(|| "-".to_string()),
                1,
            )
            .item("Handshake", data.shared.last_handshake.clone(), 1),
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
        .bg(cx.theme().background.alpha(0.72))
        .child(
            div()
                .px_4()
                .py_3()
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
    _cx: &mut Context<WgApp>,
) -> Div {
    h_flex()
        .items_center()
        .gap_2()
        .flex_wrap()
        .child(
            Button::new("inspector-preview")
                .label("Preview")
                .outline()
                .small()
                .compact()
                .selected(inspector_tab == ConfigInspectorTab::Preview)
                .on_click({
                    let workspace = workspace.clone();
                    move |_, _, cx| {
                        let _ = workspace.update(cx, |workspace, cx| {
                            let changed =
                                workspace.set_inspector_tab(ConfigInspectorTab::Preview);
                            let _ = workspace.app.update(cx, |app, cx| {
                                app.persist_preferred_inspector_tab(
                                    ConfigInspectorTab::Preview,
                                    cx,
                                );
                            });
                            if changed {
                                cx.notify();
                            }
                        });
                    }
                }),
        )
        .child(
            Button::new("inspector-diagnostics")
                .label("Diagnostics")
                .outline()
                .small()
                .compact()
                .selected(inspector_tab == ConfigInspectorTab::Diagnostics)
                .on_click({
                    let workspace = workspace.clone();
                    move |_, _, cx| {
                        let _ = workspace.update(cx, |workspace, cx| {
                            let changed =
                                workspace.set_inspector_tab(ConfigInspectorTab::Diagnostics);
                            let _ = workspace.app.update(cx, |app, cx| {
                                app.persist_preferred_inspector_tab(
                                    ConfigInspectorTab::Diagnostics,
                                    cx,
                                );
                            });
                            if changed {
                                cx.notify();
                            }
                        });
                    }
                }),
        )
        .child(
            Button::new("inspector-activity")
                .label("Activity")
                .outline()
                .small()
                .compact()
                .selected(inspector_tab == ConfigInspectorTab::Activity)
                .on_click({
                    let workspace = workspace.clone();
                    move |_, _, cx| {
                        let _ = workspace.update(cx, |workspace, cx| {
                            let changed =
                                workspace.set_inspector_tab(ConfigInspectorTab::Activity);
                            let _ = workspace.app.update(cx, |app, cx| {
                                app.persist_preferred_inspector_tab(
                                    ConfigInspectorTab::Activity,
                                    cx,
                                );
                            });
                            if changed {
                                cx.notify();
                            }
                        });
                    }
                }),
        )
}

fn render_diagnostics_strip(data: &ConfigsViewData, cx: &mut Context<WgApp>) -> Div {
    let (tone_bg, tone_border, title, detail) = match &data.draft.validation {
        DraftValidationState::Idle => (
            cx.theme().secondary.alpha(0.45),
            cx.theme().border.alpha(0.45),
            "Draft idle".to_string(),
            "Start editing to validate this config.".to_string(),
        ),
        DraftValidationState::Valid { .. } => (
            cx.theme().accent.alpha(0.12),
            cx.theme().accent.alpha(0.3),
            if data.shared.draft_dirty {
                "Valid draft".to_string()
            } else {
                "Saved and valid".to_string()
            },
            if data.shared.needs_restart {
                "Saved changes require a tunnel restart to take effect.".to_string()
            } else {
                "WireGuard config parsed successfully.".to_string()
            },
        ),
        DraftValidationState::Invalid { line, message, .. } => (
            cx.theme().danger.alpha(0.08),
            cx.theme().danger.alpha(0.3),
            "Validation error".to_string(),
            match line {
                Some(line) => format!("Line {line}: {message}"),
                None => message.to_string(),
            },
        ),
    };

    div()
        .flex()
        .items_start()
        .justify_between()
        .gap_3()
        .px_3()
        .py_2()
        .rounded_lg()
        .border_1()
        .border_color(tone_border)
        .bg(tone_bg)
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
        )
        .child(
            h_flex()
                .items_center()
                .gap_2()
                .when(data.shared.draft_dirty, |this| {
                    this.child(Tag::warning().small().child("Unsaved"))
                })
                .when(data.shared.needs_restart, |this| {
                    this.child(Tag::warning().small().child("Restart required"))
                })
                .when(matches!(data.draft.validation, DraftValidationState::Valid { .. }), |this| {
                    this.child(Tag::success().small().child("Ready"))
                }),
        )
}

fn editor_action_bar(data: &ConfigsViewData, cx: &mut Context<WgApp>) -> Div {
    let app_handle = cx.entity();
    let manage_button = if data.is_busy || !data.has_saved_source || !data.has_selection {
        Button::new("cfg-manage")
            .icon(Icon::new(IconName::Menu).size_3())
            .label("Manage")
            .outline()
            .small()
            .compact()
            .disabled(true)
            .into_any_element()
    } else {
        Button::new("cfg-manage")
            .icon(Icon::new(IconName::Menu).size_3())
            .label("Manage")
            .outline()
            .small()
            .compact()
            .dropdown_caret(true)
            .dropdown_menu_with_anchor(Corner::TopRight, move |menu: PopupMenu, _, _| {
                let rename_handle = app_handle.clone();
                let export_handle = app_handle.clone();
                let copy_handle = app_handle.clone();
                let delete_handle = app_handle.clone();
                menu
                    .item(PopupMenuItem::new("Rename").on_click({
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
                .on_click(cx.listener(|this, _, window, cx| {
                    this.handle_save_click(window, cx);
                })),
        )
        .child(
            Button::new("cfg-save-as")
                .icon(Icon::new(IconName::Copy).size_3())
                .label("Save As New")
                .outline()
                .small()
                .compact()
                .disabled(data.is_busy)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.handle_save_as_click(window, cx);
                })),
        )
        .when(data.can_restart, |this| {
            this.child(
                Button::new("cfg-save-restart")
                    .icon(Icon::new(IconName::Redo2).size_3())
                    .label("Save & Restart")
                    .outline()
                    .small()
                    .compact()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.handle_save_and_restart_click(window, cx);
                    })),
            )
        })
        .child(manage_button)
}

fn source_tag(source: &ConfigSource) -> Tag {
    match source {
        ConfigSource::File { .. } => Tag::info().small().child("Imported"),
        ConfigSource::Paste => Tag::secondary().small().child("Saved"),
    }
}

fn endpoint_family_tag(family: EndpointFamily) -> Tag {
    match family {
        EndpointFamily::V4 => Tag::secondary().small().child("IPv4"),
        EndpointFamily::V6 => Tag::info().small().child("IPv6"),
        EndpointFamily::Dual => Tag::warning().small().child("Dual"),
        EndpointFamily::Unknown => Tag::secondary().small().child("Unknown"),
    }
}
