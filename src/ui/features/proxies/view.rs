use crate::ui::state::{ProxiesViewMode, ProxyRunningFilter, WgApp};
use crate::ui::view::{PageShell, PageShellHeader};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonGroup, ButtonVariants},
    h_flex,
    input::Input,
    menu::{DropdownMenu, PopupMenuItem},
    tag::Tag,
    ActiveTheme as _, Disableable as _, Icon, IconName, Selectable, Sizable as _, StyledExt as _,
};

use super::collection::{render_proxy_gallery_view, render_proxy_list_view};
use super::detail::render_proxy_detail_pane;
use super::model::{build_proxies_view_model, ProxiesViewModel};

const PROXIES_DETAIL_WIDTH: f32 = 320.0;
const PROXIES_SPLIT_BREAKPOINT: f32 = 1180.0;

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
    let active_filters = proxy_active_filter_count(app, &search_input, cx);
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

    let header_actions = h_flex()
        .items_center()
        .gap_2()
        .flex_wrap()
        .child(
            Tag::secondary()
                .small()
                .rounded_full()
                .child(nodes_text.clone()),
        )
        .when(active_filters > 0, |this| {
            this.child(
                Tag::warning()
                    .small()
                    .rounded_full()
                    .child(format!("{active_filters} filters")),
            )
        })
        .when(selected_count > 0, |this| {
            this.child(
                Tag::info()
                    .small()
                    .rounded_full()
                    .child(format!("{selected_count} selected")),
            )
        })
        .when(
            app.selection.selected_id.is_some() && !model.selected_visible,
            |this| {
                this.child(
                    Tag::secondary()
                        .small()
                        .rounded_full()
                        .child("Selection hidden by filters"),
                )
            },
        );
    let toolbar = render_proxies_toolbar(app, &search_input, active_filters, cx);
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

    PageShell::new(
        PageShellHeader::new(
            "LIBRARY",
            "Proxies",
            "Browse saved configs and current tunnel selection.",
        )
        .actions(header_actions),
        div()
            .flex()
            .flex_col()
            .gap_3()
            .flex_grow()
            .w_full()
            .min_h(px(0.0))
            .p_3()
            .child(filters)
            .when(app.selection.proxy_select_mode, |this| this.child(bulk_bar))
            .child(content),
    )
    .toolbar(toolbar)
    .render(cx)
}

fn render_proxies_toolbar(
    app: &mut WgApp,
    search_input: &Entity<gpui_component::input::InputState>,
    active_filters: usize,
    cx: &mut Context<WgApp>,
) -> Div {
    let view_mode = ButtonGroup::new("proxy-view-mode")
        .outline()
        .compact()
        .small()
        .child(
            Button::new("proxy-view-list")
                .label("List")
                .selected(app.ui_prefs.proxies_view_mode == ProxiesViewMode::List)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.set_proxies_view_mode_pref(ProxiesViewMode::List, cx);
                })),
        )
        .child(
            Button::new("proxy-view-gallery")
                .label("Gallery")
                .selected(app.ui_prefs.proxies_view_mode == ProxiesViewMode::Gallery)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.set_proxies_view_mode_pref(ProxiesViewMode::Gallery, cx);
                })),
        );

    h_flex()
        .items_center()
        .justify_end()
        .flex_wrap()
        .gap_3()
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
                            this.command_toggle_proxy_select_mode(cx);
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
                            this.command_import_config(window, cx);
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

fn proxy_active_filter_count(
    app: &WgApp,
    search_input: &Entity<gpui_component::input::InputState>,
    cx: &mut Context<WgApp>,
) -> usize {
    [
        app.selection.proxy_country_filter.is_some(),
        app.selection.proxy_city_filter.is_some(),
        app.selection.proxy_protocol_filter.is_some(),
        app.selection.proxy_running_filter != ProxyRunningFilter::All,
        !search_input.read(cx).value().as_ref().trim().is_empty(),
    ]
    .into_iter()
    .filter(|active| *active)
    .count()
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
                                this.command_select_visible_proxies(&visible_ids, cx);
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
                            this.command_clear_proxy_selection(cx);
                        })),
                )
                .child(
                    Button::new("proxy-delete-selected")
                        .label("Delete Selected")
                        .danger()
                        .xsmall()
                        .disabled(app.runtime.busy || selected_count == 0)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.command_prompt_delete_selected_proxies(window, cx);
                        })),
                ),
        )
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
