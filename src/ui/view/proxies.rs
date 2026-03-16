use std::collections::BTreeSet;

use super::super::state::{
    ConfigSource, EndpointFamily, ProxiesViewMode, ProxyRunningFilter, TunnelConfig, WgApp,
};
use super::proxies_grid::{proxy_grid, ProxyGridMetrics};
use gpui::prelude::FluentBuilder as _;
use gpui::{uniform_list, InteractiveElement as _, StatefulInteractiveElement as _, *};
use gpui_component::{
    button::{Button, ButtonGroup, ButtonVariant, ButtonVariants},
    description_list::DescriptionList,
    dialog::DialogButtonProps,
    group_box::{GroupBox, GroupBoxVariants},
    h_flex,
    input::Input,
    menu::{DropdownMenu, PopupMenuItem},
    scroll::Scrollbar,
    tag::Tag,
    ActiveTheme as _, Disableable as _, Icon, IconName, Selectable, Sizable as _, StyledExt as _,
    WindowExt,
};

const PROXIES_CARD_WIDTH: f32 = 240.0;
const PROXIES_GALLERY_CARD_HEIGHT: f32 = 104.0;
const PROXIES_CARD_GAP: f32 = 8.0;
const PROXIES_DETAIL_WIDTH: f32 = 320.0;
const PROXIES_LIST_ROW_HEIGHT: f32 = 50.0;
const PROXIES_GALLERY_SCROLL_STATE_ID: &str = "proxies-gallery-scroll";
const PROXIES_LIST_SCROLL_STATE_ID: &str = "proxies-list-scroll";
const PROXIES_SPLIT_BREAKPOINT: f32 = 1180.0;

#[derive(Clone)]
struct ProxyNameParts {
    country: Option<String>,
    city: Option<String>,
    protocol: Option<String>,
    sequence: Option<String>,
}

#[derive(Clone)]
struct ProxyRowData {
    id: u64,
    name: String,
    name_lower: String,
    country: Option<String>,
    city: Option<String>,
    protocol: Option<String>,
    sequence: Option<String>,
    endpoint_family: EndpointFamily,
    is_running: bool,
    source_kind: &'static str,
}

impl ProxyRowData {
    fn country_label(&self) -> &str {
        self.country.as_deref().unwrap_or("—")
    }

    fn city_label(&self) -> &str {
        self.city.as_deref().unwrap_or("—")
    }

    fn protocol_label(&self) -> &str {
        self.protocol.as_deref().unwrap_or("—")
    }

    fn sequence_label(&self) -> &str {
        self.sequence.as_deref().unwrap_or("—")
    }

    fn location_label(&self) -> String {
        match (self.country.as_deref(), self.city.as_deref()) {
            (Some(country), Some(city)) => format!("{country} / {city}"),
            (Some(country), None) => country.to_string(),
            (None, Some(city)) => city.to_string(),
            (None, None) => "—".to_string(),
        }
    }
}

struct ProxiesViewModel {
    rows: Vec<ProxyRowData>,
    filtered_rows: Vec<ProxyRowData>,
    countries: Vec<String>,
    cities: Vec<String>,
    protocols: Vec<String>,
    selected_row: Option<ProxyRowData>,
    selected_visible: bool,
}

fn proxy_grid_metrics() -> ProxyGridMetrics {
    ProxyGridMetrics {
        card_width: px(PROXIES_CARD_WIDTH),
        card_height: px(PROXIES_GALLERY_CARD_HEIGHT),
        gap: px(PROXIES_CARD_GAP),
    }
}

pub(crate) fn render_proxies(app: &mut WgApp, window: &mut Window, cx: &mut Context<WgApp>) -> Div {
    app.ensure_proxy_search_input(window, cx);
    let search_input = app
        .ui
        .proxy_search_input
        .clone()
        .expect("proxy search input should be initialized");
    let query = search_input.read(cx).value().as_ref().trim().to_lowercase();
    let model = build_proxies_view_model(app, &query);
    let total_nodes = model.rows.len();
    let filtered_nodes = model.filtered_rows.len();
    let selected_count = app.selection.proxy_selected_ids.len();
    let compact_layout = window.viewport_size().width < px(PROXIES_SPLIT_BREAKPOINT);
    let app_handle = cx.entity();
    let visible_ids = model
        .filtered_rows
        .iter()
        .map(|row| row.id)
        .collect::<Vec<_>>();

    let nodes_text = if query.is_empty()
        && app.selection.proxy_country_filter.is_none()
        && app.selection.proxy_city_filter.is_none()
        && app.selection.proxy_protocol_filter.is_none()
        && app.selection.proxy_running_filter == ProxyRunningFilter::All
    {
        format!("{total_nodes} nodes")
    } else {
        format!("{filtered_nodes}/{total_nodes} nodes")
    };

    let toolbar =
        render_proxies_toolbar(app, &search_input, &model, nodes_text, selected_count, cx);
    let filters = render_proxy_filters(app, &search_input, &model, app_handle.clone(), cx);
    let bulk_bar = render_proxy_bulk_bar(app, visible_ids, selected_count, cx);

    let content = if app.configs.is_empty() {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .w_full()
            .items_center()
            .justify_center()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("No configs yet"),
            )
    } else if model.filtered_rows.is_empty() {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .w_full()
            .items_center()
            .justify_center()
            .gap_2()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("No matching nodes"),
            )
            .child(
                Button::new("proxy-clear-empty")
                    .label("Clear Filters")
                    .outline()
                    .xsmall()
                    .on_click(cx.listener(|this, _, window, cx| {
                        clear_proxy_filters(this, window, cx);
                    })),
            )
    } else {
        let browser = match app.ui_prefs.proxies_view_mode {
            ProxiesViewMode::List => render_proxy_list_view(&model.filtered_rows, window, cx),
            ProxiesViewMode::Gallery => render_proxy_gallery_view(&model.filtered_rows, window, cx),
        };
        let detail = render_proxy_detail_pane(app, &model, cx);
        if compact_layout {
            div()
                .flex()
                .flex_col()
                .gap_3()
                .flex_1()
                .min_h(px(0.0))
                .child(browser)
                .child(detail)
        } else {
            div()
                .flex()
                .flex_row()
                .gap_3()
                .flex_1()
                .min_h(px(0.0))
                .child(browser)
                .child(detail.w(px(PROXIES_DETAIL_WIDTH)))
        }
    };

    div()
        .flex()
        .flex_col()
        .gap_3()
        .flex_grow()
        .w_full()
        .min_h(px(0.0))
        .p_3()
        .rounded_lg()
        .bg(cx.theme().tiles)
        .border_1()
        .border_color(cx.theme().border)
        .child(toolbar)
        .child(filters)
        .when(app.selection.proxy_select_mode, |this| this.child(bulk_bar))
        .child(content)
}

fn render_proxies_toolbar(
    app: &mut WgApp,
    search_input: &Entity<gpui_component::input::InputState>,
    model: &ProxiesViewModel,
    nodes_text: String,
    selected_count: usize,
    cx: &mut Context<WgApp>,
) -> Div {
    let active_filters = [
        app.selection.proxy_country_filter.is_some(),
        app.selection.proxy_city_filter.is_some(),
        app.selection.proxy_protocol_filter.is_some(),
        app.selection.proxy_running_filter != ProxyRunningFilter::All,
        !search_input.read(cx).value().as_ref().trim().is_empty(),
    ]
    .into_iter()
    .filter(|active| *active)
    .count();

    let view_mode = ButtonGroup::new("proxy-view-mode")
        .outline()
        .compact()
        .small()
        .child(
            Button::new("proxy-view-list")
                .label("List")
                .selected(app.ui_prefs.proxies_view_mode == ProxiesViewMode::List)
                .on_click(cx.listener(|this, _, _, cx| {
                    if this.ui_prefs.proxies_view_mode != ProxiesViewMode::List {
                        this.ui_prefs.proxies_view_mode = ProxiesViewMode::List;
                        this.persist_state_async(cx);
                        cx.notify();
                    }
                })),
        )
        .child(
            Button::new("proxy-view-gallery")
                .label("Gallery")
                .selected(app.ui_prefs.proxies_view_mode == ProxiesViewMode::Gallery)
                .on_click(cx.listener(|this, _, _, cx| {
                    if this.ui_prefs.proxies_view_mode != ProxiesViewMode::Gallery {
                        this.ui_prefs.proxies_view_mode = ProxiesViewMode::Gallery;
                        this.persist_state_async(cx);
                        cx.notify();
                    }
                })),
        );

    h_flex()
        .items_center()
        .justify_between()
        .gap_3()
        .child(
            h_flex()
                .items_center()
                .gap_2()
                .child(div().text_lg().child("Tunnels"))
                .child(Tag::secondary().small().child(nodes_text))
                .when(active_filters > 0, |this| {
                    this.child(
                        Tag::warning()
                            .small()
                            .child(format!("{active_filters} filters")),
                    )
                })
                .when(selected_count > 0, |this| {
                    this.child(
                        Tag::info()
                            .small()
                            .child(format!("{selected_count} selected")),
                    )
                })
                .when(
                    app.selection.selected_id.is_some() && !model.selected_visible,
                    |this| {
                        this.child(
                            Tag::secondary()
                                .small()
                                .child("Selection hidden by filters"),
                        )
                    },
                ),
        )
        .child(
            h_flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .w(px(220.0))
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .bg(cx.theme().secondary)
                        .child(
                            Input::new(search_input)
                                .appearance(false)
                                .bordered(false)
                                .cleanable(true),
                        ),
                )
                .child(view_mode)
                .child(
                    Button::new("proxy-select")
                        .label(if app.selection.proxy_select_mode {
                            "Done"
                        } else {
                            "Select"
                        })
                        .outline()
                        .xsmall()
                        .selected(app.selection.proxy_select_mode)
                        .disabled(app.runtime.busy)
                        .on_click(cx.listener(|this, _, _window, cx| {
                            this.selection.proxy_select_mode = !this.selection.proxy_select_mode;
                            if !this.selection.proxy_select_mode {
                                this.selection.proxy_selected_ids.clear();
                            }
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("proxy-import")
                        .icon(Icon::new(IconName::FolderOpen).size_3())
                        .label("Import")
                        .outline()
                        .xsmall()
                        .disabled(app.runtime.busy)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.handle_import_click(window, cx);
                        })),
                )
                .child(
                    Button::new("proxy-clear-filters")
                        .label("Reset")
                        .outline()
                        .xsmall()
                        .disabled(active_filters == 0)
                        .on_click(cx.listener(|this, _, window, cx| {
                            clear_proxy_filters(this, window, cx);
                        })),
                ),
        )
}

fn render_proxy_filters(
    app: &WgApp,
    search_input: &Entity<gpui_component::input::InputState>,
    model: &ProxiesViewModel,
    app_handle: Entity<WgApp>,
    cx: &mut Context<WgApp>,
) -> Div {
    let country_label = app
        .selection
        .proxy_country_filter
        .as_deref()
        .map(|value| format!("Country: {value}"))
        .unwrap_or_else(|| "Country".to_string());
    let city_label = app
        .selection
        .proxy_city_filter
        .as_deref()
        .map(|value| format!("City: {value}"))
        .unwrap_or_else(|| "City".to_string());
    let protocol_label = app
        .selection
        .proxy_protocol_filter
        .as_deref()
        .map(|value| format!("Type: {value}"))
        .unwrap_or_else(|| "Type".to_string());
    let running_group = ButtonGroup::new("proxy-running-filter")
        .outline()
        .compact()
        .xsmall()
        .child(
            Button::new("proxy-running-all")
                .label("All")
                .selected(app.selection.proxy_running_filter == ProxyRunningFilter::All)
                .on_click(cx.listener(|this, _, _, cx| {
                    if this.selection.proxy_running_filter != ProxyRunningFilter::All {
                        this.selection.proxy_running_filter = ProxyRunningFilter::All;
                        cx.notify();
                    }
                })),
        )
        .child(
            Button::new("proxy-running-only")
                .label("Running")
                .selected(app.selection.proxy_running_filter == ProxyRunningFilter::Running)
                .on_click(cx.listener(|this, _, _, cx| {
                    if this.selection.proxy_running_filter != ProxyRunningFilter::Running {
                        this.selection.proxy_running_filter = ProxyRunningFilter::Running;
                        cx.notify();
                    }
                })),
        )
        .child(
            Button::new("proxy-idle-only")
                .label("Idle")
                .selected(app.selection.proxy_running_filter == ProxyRunningFilter::Idle)
                .on_click(cx.listener(|this, _, _, cx| {
                    if this.selection.proxy_running_filter != ProxyRunningFilter::Idle {
                        this.selection.proxy_running_filter = ProxyRunningFilter::Idle;
                        cx.notify();
                    }
                })),
        );

    h_flex()
        .items_center()
        .gap_2()
        .flex_wrap()
        .child(proxy_filter_menu_button(
            "proxy-country-filter",
            country_label,
            app.selection.proxy_country_filter.as_deref(),
            &model.countries,
            app_handle.clone(),
            |this, value| {
                this.selection.proxy_country_filter = value;
                this.selection.proxy_city_filter = None;
            },
            cx,
        ))
        .child(proxy_filter_menu_button(
            "proxy-city-filter",
            city_label,
            app.selection.proxy_city_filter.as_deref(),
            &model.cities,
            app_handle.clone(),
            |this, value| {
                this.selection.proxy_city_filter = value;
            },
            cx,
        ))
        .child(proxy_filter_menu_button(
            "proxy-protocol-filter",
            protocol_label,
            app.selection.proxy_protocol_filter.as_deref(),
            &model.protocols,
            app_handle.clone(),
            |this, value| {
                this.selection.proxy_protocol_filter = value;
            },
            cx,
        ))
        .child(running_group)
        .when(
            !search_input.read(cx).value().as_ref().trim().is_empty(),
            |this| {
                this.child(
                    Tag::secondary()
                        .small()
                        .child(format!("Search: {}", search_input.read(cx).value())),
                )
            },
        )
}

fn render_proxy_bulk_bar(
    app: &WgApp,
    visible_ids: Vec<u64>,
    selected_count: usize,
    cx: &mut Context<WgApp>,
) -> Div {
    h_flex()
        .items_center()
        .justify_between()
        .gap_3()
        .px_3()
        .py_2()
        .rounded_md()
        .bg(cx.theme().secondary)
        .border_1()
        .border_color(cx.theme().border)
        .child(
            h_flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .font_semibold()
                        .child(format!("{selected_count} selected")),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("Bulk actions apply to the current filtered view"),
                ),
        )
        .child(
            h_flex()
                .items_center()
                .gap_2()
                .child(
                    Button::new("proxy-select-visible")
                        .label("Select Visible")
                        .outline()
                        .xsmall()
                        .disabled(visible_ids.is_empty())
                        .on_click({
                            let visible_ids = visible_ids.clone();
                            cx.listener(move |this, _, _, cx| {
                                this.selection.proxy_selected_ids =
                                    visible_ids.iter().copied().collect();
                                cx.notify();
                            })
                        }),
                )
                .child(
                    Button::new("proxy-clear-selection")
                        .label("Clear")
                        .outline()
                        .xsmall()
                        .disabled(selected_count == 0)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.selection.proxy_selected_ids.clear();
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("proxy-delete-selected")
                        .label("Delete Selected")
                        .danger()
                        .xsmall()
                        .disabled(app.runtime.busy || selected_count == 0)
                        .on_click(cx.listener(|this, _, window, cx| {
                            if this.selection.proxy_selected_ids.is_empty() {
                                this.set_error("Select configs first");
                                cx.notify();
                                return;
                            }
                            let ids: Vec<u64> =
                                this.selection.proxy_selected_ids.iter().copied().collect();
                            let count = ids.len();
                            let body = if count == 1 {
                                "Delete 1 selected config? This cannot be undone.".to_string()
                            } else {
                                format!("Delete {count} selected configs? This cannot be undone.")
                            };
                            open_delete_dialog(
                                window,
                                cx,
                                "Delete selected configs?",
                                body,
                                Some("Running tunnels will be skipped.".to_string()),
                                ids,
                                true,
                                true,
                            );
                        })),
                ),
        )
}

fn render_proxy_list_view(
    filtered_rows: &[ProxyRowData],
    window: &mut Window,
    cx: &mut Context<WgApp>,
) -> Div {
    let app_entity = cx.entity();
    let rows = filtered_rows.to_vec();
    let scroll_handle = window
        .use_keyed_state(PROXIES_LIST_SCROLL_STATE_ID, cx, |_, _| {
            UniformListScrollHandle::new()
        })
        .read(cx)
        .clone();
    let list = uniform_list(
        "proxy-list",
        rows.len(),
        move |visible_range, _window, cx| {
            app_entity.update(cx, |this, cx| {
                visible_range
                    .map(|ix| proxy_list_row(this, &rows[ix], cx))
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
        .flex_1()
        .min_h(px(0.0))
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().background)
        .child(proxy_list_header(cx))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h(px(0.0))
                .relative()
                .child(list)
                .child(Scrollbar::vertical(&scroll_handle)),
        )
}

fn render_proxy_gallery_view(
    filtered_rows: &[ProxyRowData],
    window: &mut Window,
    cx: &mut Context<WgApp>,
) -> Div {
    let app_entity = cx.entity();
    let rows = filtered_rows.to_vec();
    let scroll_handle = window
        .use_keyed_state(PROXIES_GALLERY_SCROLL_STATE_ID, cx, |_, _| {
            ScrollHandle::default()
        })
        .read(cx)
        .clone();
    let grid = proxy_grid(
        "proxy-gallery-grid",
        rows.len(),
        proxy_grid_metrics(),
        scroll_handle.clone(),
        move |visible_range, _window, cx| {
            app_entity.update(cx, |this, cx| {
                visible_range
                    .map(|ix| proxy_gallery_card(this, &rows[ix], cx))
                    .collect::<Vec<_>>()
            })
        },
    )
    .with_overscan_viewports(1.0);

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.0))
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().background)
        .child(
            div()
                .id("proxy-gallery-scroll")
                .flex()
                .flex_col()
                .flex_1()
                .min_h(px(0.0))
                .overflow_y_scroll()
                .track_scroll(&scroll_handle)
                .p_2()
                .child(grid),
        )
        .child(Scrollbar::vertical(&scroll_handle))
}

fn render_proxy_detail_pane(app: &WgApp, model: &ProxiesViewModel, cx: &mut Context<WgApp>) -> Div {
    let selected_config = app.selected_config();
    let selected_row = model.selected_row.as_ref();
    let is_running = selected_row.map(|row| row.is_running).unwrap_or(false);
    let selected_hidden = app.selection.selected_id.is_some() && !model.selected_visible;

    let detail_card = match (selected_config, selected_row) {
        (Some(config), Some(row)) => {
            let source_detail = match &config.source {
                ConfigSource::File { origin_path } => origin_path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| config.storage_path.display().to_string()),
                ConfigSource::Paste => "Pasted into local storage".to_string(),
            };
            GroupBox::new()
                .fill()
                .title("Selection")
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(div().text_lg().font_semibold().child(row.name.clone()))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!(
                                            "{} / {} / {} / {}",
                                            row.country_label(),
                                            row.city_label(),
                                            row.protocol_label(),
                                            row.sequence_label()
                                        )),
                                ),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .gap_2()
                                .child(endpoint_family_tag(row.endpoint_family).unwrap_or_else(|| {
                                    Tag::secondary().small().child("Unknown")
                                }))
                                .child(
                                    if row.is_running {
                                        Tag::success().small().child("Running")
                                    } else {
                                        Tag::secondary().small().child("Idle")
                                    },
                                )
                                .child(Tag::secondary().small().child(row.source_kind)),
                        )
                        .child(
                            DescriptionList::new()
                                .columns(1)
                                .item("Country", row.country_label().to_string(), 1)
                                .item("City", row.city_label().to_string(), 1)
                                .item("Type", row.protocol_label().to_string(), 1)
                                .item("Node ID", row.sequence_label().to_string(), 1)
                                .item("Storage", config.storage_path.display().to_string(), 1)
                                .item("Source", source_detail, 1),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    Button::new("proxy-detail-connect")
                                        .label(if is_running { "Disconnect" } else { "Connect" })
                                        .disabled(app.runtime.busy)
                                        .selected(is_running)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.handle_start_stop(window, cx);
                                        })),
                                )
                                .child(
                                    Button::new("proxy-detail-delete")
                                        .label("Delete")
                                        .danger()
                                        .xsmall()
                                        .disabled(
                                            app.runtime.busy
                                                || app.selection.proxy_select_mode
                                                || is_running,
                                        )
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            let Some(config) = this.selected_config().cloned() else {
                                                this.set_error("Select a tunnel first");
                                                cx.notify();
                                                return;
                                            };
                                            open_delete_dialog(
                                                window,
                                                cx,
                                                "Delete config?",
                                                format!(
                                                    "Delete \"{}\"? This cannot be undone.",
                                                    config.name
                                                ),
                                                None,
                                                vec![config.id],
                                                false,
                                                false,
                                            );
                                        })),
                                ),
                        )
                        .when(selected_hidden, |this| {
                            this.child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("This selection is currently hidden by filters."),
                            )
                        }),
                )
        }
        _ => GroupBox::new().fill().title("Selection").child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("Pick a tunnel to inspect its structured metadata."),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("The management view keeps dense rows on the left and details on the right."),
                ),
        ),
    };

    div()
        .flex()
        .flex_col()
        .gap_3()
        .min_h(px(0.0))
        .child(detail_card)
}

fn proxy_list_header(cx: &mut Context<WgApp>) -> Div {
    div()
        .flex()
        .items_center()
        .gap_3()
        .px_3()
        .h(px(PROXIES_LIST_ROW_HEIGHT))
        .border_b_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().secondary.alpha(0.55))
        .child(column_label("Name").flex_1())
        .child(column_label("Location").w(px(150.0)))
        .child(column_label("Type").w(px(72.0)))
        .child(column_label("Family").w(px(96.0)))
        .child(column_label("Source").w(px(72.0)))
        .child(column_label("Status").w(px(84.0)))
}

fn proxy_list_row(app: &WgApp, row: &ProxyRowData, cx: &mut Context<WgApp>) -> Stateful<Div> {
    let is_selected = app.selection.selected_id == Some(row.id);
    let is_multi_selected =
        app.selection.proxy_select_mode && app.selection.proxy_selected_ids.contains(&row.id);
    let bg = if is_selected {
        cx.theme().accent.alpha(0.16)
    } else if is_multi_selected {
        cx.theme().accent.alpha(0.08)
    } else {
        cx.theme().background
    };
    let border_color = if is_selected {
        cx.theme().accent.alpha(0.55)
    } else {
        cx.theme().border.alpha(0.35)
    };

    let status = if row.is_running { "Running" } else { "Idle" };
    let config_id = row.id;

    let mut base = div()
        .id(("proxy-list-row", config_id))
        .flex()
        .items_center()
        .gap_3()
        .px_3()
        .h(px(PROXIES_LIST_ROW_HEIGHT))
        .border_b_1()
        .border_color(border_color)
        .bg(bg)
        .child(
            div()
                .flex_1()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .font_semibold()
                        .truncate()
                        .child(row.name.clone()),
                )
                .when(is_multi_selected, |this| {
                    this.child(Tag::info().small().child("Selected"))
                }),
        )
        .child(column_value(row.location_label()).w(px(150.0)))
        .child(column_value(row.protocol_label().to_string()).w(px(72.0)))
        .child(
            div().w(px(96.0)).child(
                endpoint_family_tag(row.endpoint_family)
                    .unwrap_or_else(|| Tag::secondary().small().child("Unknown")),
            ),
        )
        .child(column_value(row.source_kind.to_string()).w(px(72.0)))
        .child(div().w(px(84.0)).child(if row.is_running {
            Tag::success().small().child(status)
        } else {
            Tag::secondary().small().child(status)
        }));

    if !app.runtime.busy {
        base = base.cursor_pointer();
    }

    base.on_click(cx.listener(move |this, _, window, cx| {
        if this.runtime.busy {
            return;
        }
        if this.selection.proxy_select_mode {
            if this.selection.proxy_selected_ids.contains(&config_id) {
                this.selection.proxy_selected_ids.remove(&config_id);
            } else {
                this.selection.proxy_selected_ids.insert(config_id);
            }
            cx.notify();
            return;
        }
        this.select_tunnel(config_id, window, cx);
    }))
}

fn proxy_gallery_card(app: &WgApp, row: &ProxyRowData, cx: &mut Context<WgApp>) -> Stateful<Div> {
    let is_selected = app.selection.selected_id == Some(row.id);
    let is_multi_selected =
        app.selection.proxy_select_mode && app.selection.proxy_selected_ids.contains(&row.id);
    let bg = if is_selected {
        cx.theme().accent.alpha(0.16)
    } else if is_multi_selected {
        cx.theme().accent.alpha(0.08)
    } else {
        cx.theme().secondary
    };
    let border_color = if is_selected {
        cx.theme().accent_foreground
    } else if is_multi_selected {
        cx.theme().accent.alpha(0.6)
    } else if cx.theme().is_dark() {
        cx.theme().foreground.alpha(0.12)
    } else {
        cx.theme().border
    };

    let mut badges = h_flex().gap_1();
    if is_multi_selected {
        badges = badges.child(Tag::info().small().child("Selected"));
    }
    if let Some(tag) = endpoint_family_tag(row.endpoint_family) {
        badges = badges.child(tag);
    }
    if row.is_running {
        badges = badges.child(Tag::success().small().child("Running"));
    }

    let mut item = div()
        .id(("proxy-gallery-card", row.id))
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .rounded_lg()
        .border_1()
        .border_color(border_color)
        .bg(bg)
        .w(px(PROXIES_CARD_WIDTH))
        .h(px(PROXIES_GALLERY_CARD_HEIGHT))
        .child(
            div()
                .text_sm()
                .font_semibold()
                .truncate()
                .child(row.name.clone()),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .truncate()
                .child(format!(
                    "{} / {} / {}",
                    row.country_label(),
                    row.city_label(),
                    row.protocol_label()
                )),
        )
        .child(badges);

    if !app.runtime.busy {
        item = item.cursor_pointer();
    }

    let config_id = row.id;
    item.on_click(cx.listener(move |this, _, window, cx| {
        if this.runtime.busy {
            return;
        }
        if this.selection.proxy_select_mode {
            if this.selection.proxy_selected_ids.contains(&config_id) {
                this.selection.proxy_selected_ids.remove(&config_id);
            } else {
                this.selection.proxy_selected_ids.insert(config_id);
            }
            cx.notify();
            return;
        }
        this.select_tunnel(config_id, window, cx);
    }))
}

fn proxy_filter_menu_button(
    id: &'static str,
    label: String,
    selected: Option<&str>,
    options: &[String],
    app_handle: Entity<WgApp>,
    apply: impl Fn(&mut WgApp, Option<String>) + Clone + 'static,
    _cx: &mut Context<WgApp>,
) -> impl IntoElement {
    let options = options.to_vec();
    let selected_value = selected.map(ToOwned::to_owned);
    Button::new(id)
        .label(label)
        .outline()
        .xsmall()
        .selected(selected_value.is_some())
        .dropdown_caret(true)
        .dropdown_menu_with_anchor(Corner::BottomLeft, move |menu, _, _| {
            let mut menu = menu
                .min_w(px(140.0))
                .max_h(px(280.0))
                .scrollable(true)
                .item(
                    PopupMenuItem::new("Any")
                        .checked(selected_value.is_none())
                        .on_click({
                            let app_handle = app_handle.clone();
                            let apply = apply.clone();
                            move |_, _, cx| {
                                app_handle.update(cx, |this, cx| {
                                    apply(this, None);
                                    cx.notify();
                                });
                            }
                        }),
                )
                .item(PopupMenuItem::separator());
            for option in &options {
                let value = option.clone();
                menu = menu.item(
                    PopupMenuItem::new(value.clone())
                        .checked(selected_value.as_deref() == Some(value.as_str()))
                        .on_click({
                            let app_handle = app_handle.clone();
                            let apply = apply.clone();
                            move |_, _, cx| {
                                app_handle.update(cx, |this, cx| {
                                    apply(this, Some(value.clone()));
                                    cx.notify();
                                });
                            }
                        }),
                );
            }
            menu
        })
}

fn build_proxies_view_model(app: &WgApp, query: &str) -> ProxiesViewModel {
    let rows = app
        .configs
        .iter()
        .map(|config| proxy_row_data(config, app.runtime.running_id == Some(config.id)))
        .collect::<Vec<_>>();
    let countries = rows
        .iter()
        .filter_map(|row| row.country.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let cities = rows
        .iter()
        .filter(|row| {
            app.selection
                .proxy_country_filter
                .as_deref()
                .is_none_or(|country| row.country.as_deref() == Some(country))
        })
        .filter_map(|row| row.city.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let protocols = rows
        .iter()
        .filter(|row| {
            app.selection
                .proxy_country_filter
                .as_deref()
                .is_none_or(|country| row.country.as_deref() == Some(country))
                && app
                    .selection
                    .proxy_city_filter
                    .as_deref()
                    .is_none_or(|city| row.city.as_deref() == Some(city))
        })
        .filter_map(|row| row.protocol.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let filtered_rows = rows
        .iter()
        .filter(|row| {
            (query.is_empty() || row.name_lower.contains(query))
                && app
                    .selection
                    .proxy_country_filter
                    .as_deref()
                    .is_none_or(|country| row.country.as_deref() == Some(country))
                && app
                    .selection
                    .proxy_city_filter
                    .as_deref()
                    .is_none_or(|city| row.city.as_deref() == Some(city))
                && app
                    .selection
                    .proxy_protocol_filter
                    .as_deref()
                    .is_none_or(|protocol| row.protocol.as_deref() == Some(protocol))
                && match app.selection.proxy_running_filter {
                    ProxyRunningFilter::All => true,
                    ProxyRunningFilter::Running => row.is_running,
                    ProxyRunningFilter::Idle => !row.is_running,
                }
        })
        .cloned()
        .collect::<Vec<_>>();
    let selected_row = app
        .selection
        .selected_id
        .and_then(|selected_id| rows.iter().find(|row| row.id == selected_id).cloned());
    let selected_visible = app
        .selection
        .selected_id
        .is_some_and(|selected_id| filtered_rows.iter().any(|row| row.id == selected_id));

    ProxiesViewModel {
        rows,
        filtered_rows,
        countries,
        cities,
        protocols,
        selected_row,
        selected_visible,
    }
}

fn proxy_row_data(config: &TunnelConfig, is_running: bool) -> ProxyRowData {
    let parts = parse_proxy_name(&config.name);
    ProxyRowData {
        id: config.id,
        name: config.name.clone(),
        name_lower: config.name_lower.clone(),
        country: parts.country,
        city: parts.city,
        protocol: parts.protocol,
        sequence: parts.sequence,
        endpoint_family: config.endpoint_family,
        is_running,
        source_kind: match config.source {
            ConfigSource::File { .. } => "File",
            ConfigSource::Paste => "Paste",
        },
    }
}

fn parse_proxy_name(name: &str) -> ProxyNameParts {
    let segments = name
        .split('-')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    ProxyNameParts {
        country: segments.first().map(|segment| segment.to_ascii_uppercase()),
        city: segments.get(1).map(|segment| segment.to_ascii_uppercase()),
        protocol: segments.get(2).map(|segment| segment.to_ascii_uppercase()),
        sequence: segments.last().map(|segment| segment.to_string()),
    }
}

fn clear_proxy_filters(app: &mut WgApp, window: &mut Window, cx: &mut Context<WgApp>) {
    app.selection.proxy_country_filter = None;
    app.selection.proxy_city_filter = None;
    app.selection.proxy_protocol_filter = None;
    app.selection.proxy_running_filter = ProxyRunningFilter::All;
    if let Some(search_input) = app.ui.proxy_search_input.clone() {
        search_input.update(cx, |input, cx| {
            if !input.value().is_empty() {
                input.set_value("", window, cx);
            }
        });
    }
    cx.notify();
}

fn endpoint_family_tag(family: EndpointFamily) -> Option<Tag> {
    Some(match family {
        EndpointFamily::V4 => Tag::secondary().small().child("IPv4"),
        EndpointFamily::V6 => Tag::info().small().child("IPv6"),
        EndpointFamily::Dual => Tag::warning().small().child("Dual"),
        EndpointFamily::Unknown => return None,
    })
}

fn column_label(text: impl Into<SharedString>) -> Div {
    div()
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgb(0x94a3b8))
        .child(text.into())
}

fn column_value(text: impl Into<SharedString>) -> Div {
    div().text_sm().truncate().child(text.into())
}

fn open_delete_dialog(
    window: &mut Window,
    cx: &mut Context<WgApp>,
    title: impl Into<String>,
    body: impl Into<String>,
    note: Option<String>,
    ids: Vec<u64>,
    skip_running: bool,
    clear_selection: bool,
) {
    let app_handle = cx.entity();
    let title = title.into();
    let body = body.into();
    let note = note.clone();

    window.open_dialog(cx, move |dialog, _window, cx| {
        let app_handle = app_handle.clone();
        let ids = ids.clone();
        let note_skip = skip_running;
        let clear_selection = clear_selection;
        let mut dialog = dialog
            .title(div().text_lg().child(title.clone()))
            .confirm()
            .button_props(
                DialogButtonProps::default()
                    .ok_text("Delete")
                    .ok_variant(ButtonVariant::Danger)
                    .cancel_text("Cancel"),
            )
            .child(div().text_sm().child(body.clone()));

        if let Some(note) = note.clone() {
            dialog = dialog.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(note),
            );
        }

        let delete_action = {
            let app_handle = app_handle.clone();
            let ids = ids.clone();
            move |window: &mut Window, cx: &mut App| {
                perform_delete(&app_handle, &ids, note_skip, clear_selection, window, cx);
            }
        };

        dialog = dialog.footer(move |_ok, _cancel, _window, _cx| {
            let app_handle = app_handle.clone();
            let ids = ids.clone();
            let delete_button = Button::new("proxy-dialog-delete")
                .label("Delete")
                .danger()
                .on_click(move |_, window, cx| {
                    perform_delete(&app_handle, &ids, note_skip, clear_selection, window, cx);
                    window.close_dialog(cx);
                });
            let cancel_button = Button::new("proxy-dialog-cancel")
                .label("Cancel")
                .outline()
                .on_click(|_, window, cx| {
                    window.close_dialog(cx);
                });
            vec![
                cancel_button.into_any_element(),
                delete_button.into_any_element(),
            ]
        });

        dialog = dialog.on_ok(move |_, window, cx| {
            delete_action(window, cx);
            true
        });

        dialog
    });
}

fn perform_delete(
    app_handle: &Entity<WgApp>,
    ids: &[u64],
    skip_running: bool,
    clear_selection: bool,
    window: &mut Window,
    _cx: &mut App,
) {
    let app_handle = app_handle.clone();
    let ids = ids.to_vec();
    let note_skip = skip_running;
    let clear_selection = clear_selection;
    window.on_next_frame(move |window, cx| {
        app_handle.update(cx, |this, cx| {
            if note_skip {
                this.delete_configs_skip_running(&ids, window, cx);
            } else {
                this.delete_configs_blocking_running(&ids, window, cx);
            }
            if clear_selection {
                this.selection.proxy_select_mode = false;
                this.selection.proxy_selected_ids.clear();
            }
        });
    });
}
