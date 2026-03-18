use gpui::prelude::FluentBuilder as _;
use gpui::{uniform_list, InteractiveElement as _, StatefulInteractiveElement as _, *};
use gpui_component::{
    button::{Button, ButtonVariants},
    description_list::DescriptionList,
    group_box::{GroupBox, GroupBoxVariants},
    h_flex,
    input::{Input, InputState},
    resizable::{h_resizable, resizable_panel},
    scroll::{ScrollableElement, Scrollbar},
    tag::Tag,
    v_flex, ActiveTheme as _, Disableable as _, Icon, IconName, Selectable as _, Sizable as _,
    StyledExt as _,
};

use super::super::format::{
    format_addresses, format_allowed_ips, format_dns, format_route_table,
};
use super::super::state::{
    ConfigInspectorTab, ConfigSource, ConfigsWorkspace, DraftValidationState, EndpointFamily,
    WgApp,
};
use super::data::ViewData;
use super::widgets::status_badge;

const CONFIGS_LIBRARY_WIDTH: f32 = 300.0;
const CONFIGS_INSPECTOR_WIDTH: f32 = 332.0;
const CONFIGS_COMPACT_BREAKPOINT: f32 = 1260.0;
const CONFIGS_LIBRARY_ROW_HEIGHT: f32 = 86.0;
const CONFIGS_LIBRARY_SCROLL_STATE_ID: &str = "configs-library-scroll";

#[derive(Clone)]
struct ConfigLibraryRowData {
    id: u64,
    name: String,
    subtitle: String,
    source: ConfigSource,
    endpoint_family: EndpointFamily,
    is_running: bool,
    is_dirty: bool,
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
        self.app.update(cx, |app, cx| {
            app.ensure_inputs(window, cx);
            let data = ViewData::new(app);
            let name_input = app
                .ui
                .name_input
                .clone()
                .expect("name input should be initialized");
            let config_input = app
                .ui
                .config_input
                .clone()
                .expect("config input should be initialized");

            div()
                .flex()
                .flex_col()
                .gap_3()
                .flex_1()
                .min_h(px(0.0))
                .child(super::top_bar::render_top_bar(app, &data, cx))
                .child(render_configs_page(
                    app,
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
    data: &ViewData,
    name_input: &Entity<InputState>,
    config_input: &Entity<InputState>,
    window: &mut Window,
    cx: &mut Context<WgApp>,
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
            .child(render_library_panel(app, window, cx))
            .child(render_editor_panel(app, data, name_input, config_input, cx))
            .child(render_inspector_panel(app, data, cx))
            .into_any_element()
    } else {
        div()
            .flex_1()
            .min_h(px(0.0))
            .child(
                h_resizable("configs-workspace")
                    .child(
                        resizable_panel()
                            .size(px(CONFIGS_LIBRARY_WIDTH))
                            .size_range(px(240.0)..px(420.0))
                            .child(div().h_full().p_3().child(render_library_panel(app, window, cx))),
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
                            .size(px(CONFIGS_INSPECTOR_WIDTH))
                            .size_range(px(280.0)..px(440.0))
                            .child(div().h_full().p_3().child(render_inspector_panel(app, data, cx))),
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

fn render_configs_shell_header(app: &WgApp, data: &ViewData, cx: &mut Context<WgApp>) -> Div {
    let selected_name = current_draft_title(app);

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
                        .child(status_badge(data.config_status.as_ref()))
                        .when(data.draft_dirty, |this| {
                            this.child(Tag::warning().small().rounded_full().child("Dirty"))
                        })
                        .when(data.needs_restart, |this| {
                            this.child(
                                Tag::warning()
                                    .small()
                                    .rounded_full()
                                    .child("Needs restart"),
                            )
                        })
                        .when(
                            app.runtime.running_id == app.editor.draft.source_id && app.runtime.running,
                            |this| this.child(Tag::success().small().rounded_full().child("Running")),
                        ),
                ),
        )
}

fn render_library_panel(app: &WgApp, window: &mut Window, cx: &mut Context<WgApp>) -> Div {
    let count = app.configs.len();
    let rows = build_library_rows(app);
    let rows_for_list = rows.clone();
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
                                        .disabled(app.editor.is_busy())
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
                                        .disabled(app.editor.is_busy())
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
                                        .disabled(app.editor.is_busy())
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.handle_paste_click(window, cx);
                                        })),
                                ),
                        )
                        .when(app.editor.draft.source_id.is_none() && !app.editor.draft.name.is_empty(), |this| {
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

fn build_library_rows(app: &WgApp) -> Vec<ConfigLibraryRowData> {
    app.configs
        .iter()
        .map(|config| ConfigLibraryRowData {
            id: config.id,
            name: config.name.clone(),
            subtitle: config_subtitle(config),
            source: config.source.clone(),
            endpoint_family: config.endpoint_family,
            is_running: app.runtime.running_id == Some(config.id)
                || app.runtime.running_name.as_deref() == Some(config.name.as_str()),
            is_dirty: app.editor.draft.source_id == Some(config.id) && app.editor.draft.is_dirty(),
        })
        .collect()
}

fn render_library_row(
    app: &WgApp,
    row: &ConfigLibraryRowData,
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
            if this.editor.is_busy() {
                return;
            }
            this.select_tunnel(config_id, window, cx);
        }))
}

fn render_editor_panel(
    app: &WgApp,
    data: &ViewData,
    name_input: &Entity<InputState>,
    config_input: &Entity<InputState>,
    cx: &mut Context<WgApp>,
) -> Div {
    let selected_source = app
        .editor
        .draft
        .source_id
        .and_then(|id| app.configs.get_by_id(id));

    let summary = selected_source
        .map(config_origin_summary)
        .unwrap_or_else(|| "Unsaved draft".to_string());
    let note = if app.runtime.running_id == app.editor.draft.source_id && app.editor.draft.is_dirty() {
        "Editing a running tunnel. Saved changes take effect after restart."
    } else if app.runtime.running_id == app.editor.draft.source_id {
        "This tunnel is currently running."
    } else {
        "Changes affect the saved config after you save them."
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
                                                .child(current_draft_title(app)),
                                        )
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(summary),
                                        ),
                                )
                                .child(editor_action_bar(app, cx)),
                        )
                        .child(
                            div()
                                .px_3()
                                .py_2()
                                .rounded_lg()
                                .border_1()
                                .border_color(cx.theme().border.alpha(0.45))
                                .bg(cx.theme().secondary.alpha(0.45))
                                .child(
                                    v_flex()
                                        .gap_1()
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_semibold()
                                                .text_color(cx.theme().muted_foreground)
                                                .child("Workspace Summary"),
                                        )
                                        .child(
                                            DescriptionList::new()
                                                .columns(2)
                                                .item(
                                                    "Draft State",
                                                    if data.draft_dirty { "Dirty" } else { "Saved" },
                                                    1,
                                                )
                                                .item(
                                                    "Source",
                                                    if app.editor.draft.source_id.is_some() {
                                                        "Saved config".to_string()
                                                    } else {
                                                        "Unsaved draft".to_string()
                                                    },
                                                    1,
                                                )
                                                .item("Status", data.parse_error.clone().unwrap_or_else(|| "Ready".to_string()), 2),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(note),
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
                .child(render_diagnostics_strip(app, data, cx))
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

fn render_inspector_panel(app: &WgApp, data: &ViewData, cx: &mut Context<WgApp>) -> Div {
    let selected_source = app
        .editor
        .draft
        .source_id
        .and_then(|id| app.configs.get_by_id(id));

    let preview_card = {
        let addresses = data
            .parsed_config
            .as_ref()
            .map(|cfg| format_addresses(&cfg.interface))
            .unwrap_or_else(|| "-".to_string());
        let dns = data
            .parsed_config
            .as_ref()
            .map(|cfg| format_dns(&cfg.interface))
            .unwrap_or_else(|| "-".to_string());
        let route_table = data
            .parsed_config
            .as_ref()
            .map(|cfg| format_route_table(cfg.interface.table))
            .unwrap_or_else(|| "-".to_string());
        let routes = data
            .parsed_config
            .as_ref()
            .map(|cfg| format_allowed_ips(&cfg.peers))
            .unwrap_or_else(|| "-".to_string());
        let peers = data
            .parsed_config
            .as_ref()
            .map(|cfg| cfg.peers.len().to_string())
            .unwrap_or_else(|| "0".to_string());
        let source = selected_source
            .map(config_origin_summary)
            .unwrap_or_else(|| "Unsaved draft".to_string());

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
        let (state, line, message) = match &app.editor.draft.validation {
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
                    if data.draft_dirty { "Unsaved changes" } else { "Saved" },
                    1,
                )
                .item(
                    "Runtime",
                    if data.needs_restart {
                        "Restart required after save".to_string()
                    } else if app.runtime.running_id == app.editor.draft.source_id && app.runtime.running {
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
            .item("Handshake", data.last_handshake.clone(), 1),
    );

    let inspector_tabs = inspector_tab_row(app, cx);
    let inspector_body = match app.ui_session.inspector_tab {
        ConfigInspectorTab::Preview => preview_card.into_any_element(),
        ConfigInspectorTab::Diagnostics => diagnostics_card.into_any_element(),
        ConfigInspectorTab::Activity => activity_card.into_any_element(),
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
                .child(inspector_tabs)
                .child(inspector_body),
        )
}

fn inspector_tab_row(app: &WgApp, cx: &mut Context<WgApp>) -> Div {
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
                .selected(app.ui_session.inspector_tab == ConfigInspectorTab::Preview)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.set_session_inspector_tab(ConfigInspectorTab::Preview, cx);
                })),
        )
        .child(
            Button::new("inspector-diagnostics")
                .label("Diagnostics")
                .outline()
                .small()
                .compact()
                .selected(app.ui_session.inspector_tab == ConfigInspectorTab::Diagnostics)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.set_session_inspector_tab(ConfigInspectorTab::Diagnostics, cx);
                })),
        )
        .child(
            Button::new("inspector-activity")
                .label("Activity")
                .outline()
                .small()
                .compact()
                .selected(app.ui_session.inspector_tab == ConfigInspectorTab::Activity)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.set_session_inspector_tab(ConfigInspectorTab::Activity, cx);
                })),
        )
}

fn render_diagnostics_strip(app: &WgApp, data: &ViewData, cx: &mut Context<WgApp>) -> Div {
    let (tone_bg, tone_border, title, detail) = match &app.editor.draft.validation {
        DraftValidationState::Idle => (
            cx.theme().secondary.alpha(0.45),
            cx.theme().border.alpha(0.45),
            "Draft idle".to_string(),
            "Start editing to validate this config.".to_string(),
        ),
        DraftValidationState::Valid { .. } => (
            cx.theme().accent.alpha(0.12),
            cx.theme().accent.alpha(0.3),
            if data.draft_dirty {
                "Valid draft".to_string()
            } else {
                "Saved and valid".to_string()
            },
            if data.needs_restart {
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
                .when(data.draft_dirty, |this| {
                    this.child(Tag::warning().small().child("Unsaved"))
                })
                .when(data.needs_restart, |this| {
                    this.child(Tag::warning().small().child("Restart required"))
                })
                .when(
                    matches!(app.editor.draft.validation, DraftValidationState::Valid { .. }),
                    |this| this.child(Tag::success().small().child("Ready")),
                ),
        )
}

fn editor_action_bar(app: &WgApp, cx: &mut Context<WgApp>) -> Div {
    let busy = app.editor.is_busy();
    let has_selection = app.selection.selected_id.is_some();
    let has_saved_source = app.editor.draft.source_id.is_some();
    let can_save = !busy
        && !matches!(app.editor.draft.validation, DraftValidationState::Idle)
        && (app.editor.draft.is_dirty() || !has_saved_source);

    v_flex()
        .gap_2()
        .child(
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
                        .disabled(!can_save)
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
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.handle_save_as_click(window, cx);
                        })),
                ),
        )
        .child(
            h_flex()
                .items_center()
                .gap_2()
                .flex_wrap()
                .child(
                    Button::new("cfg-rename")
                        .icon(Icon::new(IconName::Replace).size_3())
                        .label("Rename")
                        .outline()
                        .small()
                        .compact()
                        .disabled(busy || !has_saved_source || !has_selection)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.handle_rename_click(window, cx);
                        })),
                )
                .child(
                    Button::new("cfg-delete")
                        .icon(Icon::new(IconName::Delete).size_3())
                        .label("Delete")
                        .danger()
                        .small()
                        .compact()
                        .disabled(busy || !has_selection)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.handle_delete_click(window, cx);
                        })),
                )
                .child(
                    Button::new("cfg-export")
                        .icon(Icon::new(IconName::ExternalLink).size_3())
                        .label("Export")
                        .outline()
                        .small()
                        .compact()
                        .disabled(busy || !has_selection)
                        .on_click(cx.listener(|this, _, _window, cx| {
                            this.handle_export_click(cx);
                        })),
                )
                .child(
                    Button::new("cfg-copy")
                        .icon(Icon::new(IconName::Copy).size_3())
                        .label("Copy")
                        .outline()
                        .small()
                        .compact()
                        .disabled(busy || !has_selection)
                        .on_click(cx.listener(|this, _, _window, cx| {
                            this.handle_copy_click(cx);
                        })),
                ),
        )
}

fn current_draft_title(app: &WgApp) -> String {
    let name = app.editor.draft.name.as_ref().trim();
    if !name.is_empty() {
        return name.to_string();
    }
    if app.editor.draft.source_id.is_none() {
        return "New Draft".to_string();
    }
    "Untitled Config".to_string()
}

fn config_origin_summary(config: &super::super::state::TunnelConfig) -> String {
    match &config.source {
        ConfigSource::File { origin_path } => origin_path
            .as_ref()
            .map(|path| format!("Imported from {}", path.display()))
            .unwrap_or_else(|| "Imported config".to_string()),
        ConfigSource::Paste => "Created in app storage".to_string(),
    }
}

fn config_subtitle(config: &super::super::state::TunnelConfig) -> String {
    match &config.source {
        ConfigSource::File { origin_path } => origin_path
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .map(|name| format!("Imported • {name}"))
            .unwrap_or_else(|| "Imported config".to_string()),
        ConfigSource::Paste => "Saved in app storage".to_string(),
    }
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
