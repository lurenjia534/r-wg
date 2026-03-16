use super::super::state::{EndpointFamily, TunnelConfig, WgApp};
use super::proxies_grid::{proxy_grid, ProxyGridMetrics};
use gpui::prelude::FluentBuilder as _;
use gpui::InteractiveElement as _;
use gpui::StatefulInteractiveElement as _;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariant, ButtonVariants},
    dialog::DialogButtonProps,
    h_flex,
    input::Input,
    scroll::Scrollbar,
    tag::Tag,
    ActiveTheme as _, Disableable as _, Icon, IconName, Selectable, Sizable as _, StyledExt as _,
    WindowExt,
};

// 卡片固定尺寸用于虚拟化行高计算与渲染稳定性。
const PROXIES_CARD_WIDTH: f32 = 240.0;
const PROXIES_CARD_HEIGHT: f32 = 72.0;
const PROXIES_CARD_GAP: f32 = 8.0;
// 复用滚动状态，避免每次重建滚动位置丢失。
const PROXIES_SCROLL_STATE_ID: &str = "proxies-scroll";

fn proxy_grid_metrics() -> ProxyGridMetrics {
    ProxyGridMetrics {
        card_width: px(PROXIES_CARD_WIDTH),
        card_height: px(PROXIES_CARD_HEIGHT),
        gap: px(PROXIES_CARD_GAP),
    }
}

/// Proxies 页面：配置列表入口，用于快速选择隧道。
pub(crate) fn render_proxies(app: &mut WgApp, window: &mut Window, cx: &mut Context<WgApp>) -> Div {
    // 初始化搜索输入框并读取当前查询，用于过滤节点列表。
    app.ensure_proxy_search_input(window, cx);
    let search_input = app
        .ui
        .proxy_search_input
        .clone()
        .expect("proxy search input should be initialized");
    let total_nodes = app.configs.len();
    let query = search_input.read(cx).value();
    let query = query.as_ref().trim().to_lowercase();
    let use_filter = !query.is_empty();
    // 过滤逻辑说明：
    // - 查询为空时不构建索引，直接按原列表渲染，减少无意义的遍历；
    // - 查询不为空时，缓存过滤结果，只有“查询串变化或总数变化”才重新计算。
    let filtered_nodes = if use_filter {
        if app.selection.proxy_filter_query != query
            || app.selection.proxy_filter_total != total_nodes
        {
            app.selection.proxy_filter_query = query.clone();
            app.selection.proxy_filter_total = total_nodes;
            app.selection.proxy_filtered_ids = app
                .configs
                .iter()
                .filter_map(|config| config.name_lower.contains(&query).then_some(config.id))
                .collect();
        }
        app.selection.proxy_filtered_ids.len()
    } else {
        total_nodes
    };
    // 查询为空显示总数；查询有值时显示“匹配/总数”。
    let nodes_text = if query.is_empty() {
        format!("{total_nodes} nodes")
    } else {
        format!("{filtered_nodes}/{total_nodes} nodes")
    };
    let nodes_tag = Tag::secondary().small().child(nodes_text);
    let selected_count = app.selection.proxy_selected_ids.len();
    let selected_tag = Tag::info()
        .small()
        .child(format!("{selected_count} selected"));

    let list_scroll = if app.configs.is_empty() {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .w_full()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("No configs yet"),
            )
    } else if use_filter && filtered_nodes == 0 {
        // 有配置但没有匹配项时给出提示。
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .w_full()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("No matching nodes"),
            )
    } else {
        let app_entity = cx.entity();
        let config_ids = if use_filter {
            app.selection.proxy_filtered_ids.clone()
        } else {
            app.configs.iter().map(|config| config.id).collect()
        };
        let scroll_handle = window
            .use_keyed_state(PROXIES_SCROLL_STATE_ID, cx, |_, _| ScrollHandle::default())
            .read(cx)
            .clone();
        let grid = proxy_grid(
            "proxy-grid",
            config_ids.len(),
            proxy_grid_metrics(),
            scroll_handle.clone(),
            move |visible_range, _window, cx| {
                app_entity.update(cx, |this, cx| {
                    visible_range
                        .filter_map(|ix| this.configs.get_by_id(config_ids[ix]))
                        .map(|config| config_list_item(this, config, cx))
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
            .w_full()
            .relative()
            .child(
                div()
                    .id("proxy-grid-scroll")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.0))
                    .w_full()
                    .overflow_y_scroll()
                    .track_scroll(&scroll_handle)
                    .child(grid),
            )
            .child(Scrollbar::vertical(&scroll_handle))
    };

    let panel = div()
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
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .child(div().text_lg().child("Tunnels"))
                .child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(
                            // 搜索框：轻量样式，与右侧标签/按钮并排。
                            div()
                                .w(px(200.0))
                                .px_2()
                                .py_1()
                                .rounded_md()
                                .bg(cx.theme().secondary)
                                .child(
                                    Input::new(&search_input)
                                        .appearance(false)
                                        .bordered(false)
                                        .cleanable(true),
                                ),
                        )
                        .child(nodes_tag)
                        .when(app.selection.proxy_select_mode, |this| this.child(selected_tag))
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
                                    this.selection.proxy_select_mode =
                                        !this.selection.proxy_select_mode;
                                    if !this.selection.proxy_select_mode {
                                        this.selection.proxy_selected_ids.clear();
                                    }
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("proxy-delete-selected")
                                .label("Delete Selected")
                                .danger()
                                .xsmall()
                                .disabled(
                                    app.runtime.busy
                                        || !app.selection.proxy_select_mode
                                        || app.selection.proxy_selected_ids.is_empty(),
                                )
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
                                        "Delete 1 selected config? This cannot be undone."
                                            .to_string()
                                    } else {
                                        format!(
                                            "Delete {count} selected configs? This cannot be undone."
                                        )
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
                        )
                        .child(
                            Button::new("proxy-delete")
                                .label("Delete")
                                .danger()
                                .xsmall()
                                .disabled(
                                    app.runtime.busy
                                        || app.selection.proxy_select_mode
                                        || app.selection.selected_id.is_none(),
                                )
                                .on_click(cx.listener(|this, _, window, cx| {
                                    let Some(config) = this.selected_config().cloned() else {
                                        this.set_error("Select a tunnel first");
                                        cx.notify();
                                        return;
                                    };
                                    let is_running = this.runtime.running_id == Some(config.id)
                                        || this.runtime.running_name.as_deref()
                                            == Some(config.name.as_str());
                                    if is_running {
                                        this.set_error("Stop the tunnel before deleting");
                                        cx.notify();
                                        return;
                                    }
                                    let name = config.name.clone();
                                    let id = config.id;
                                    let body = format!(
                                        "Delete \"{name}\"? This cannot be undone."
                                    );
                                    open_delete_dialog(
                                        window,
                                        cx,
                                        "Delete config?",
                                        body,
                                        None,
                                        vec![id],
                                        false,
                                        false,
                                    );
                                })),
                        )
                        .child(
                            Button::new("cfg-list-import")
                                .icon(Icon::new(IconName::FolderOpen).size_3())
                                .label("Import")
                                .outline()
                                .xsmall()
                                .disabled(app.runtime.busy)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.handle_import_click(window, cx);
                                })),
                        ),
                ),
        );
    panel.child(list_scroll)
}

fn config_list_item(app: &WgApp, config: &TunnelConfig, cx: &mut Context<WgApp>) -> Stateful<Div> {
    let is_selected = app.selection.selected_id == Some(config.id);
    let is_multi_selected =
        app.selection.proxy_select_mode && app.selection.proxy_selected_ids.contains(&config.id);
    let is_running = app.runtime.running_name.as_deref() == Some(config.name.as_str());
    let name_color = cx.theme().foreground;
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
    if let Some(tag) = endpoint_family_tag(config.endpoint_family) {
        badges = badges.child(tag);
    }
    if is_running {
        badges = badges.child(Tag::success().small().child("Running"));
    }

    let mut item = div()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .rounded_lg()
        .border_1()
        .border_color(border_color)
        .bg(bg)
        .w(px(PROXIES_CARD_WIDTH))
        .h(px(PROXIES_CARD_HEIGHT))
        .child(
            div()
                .text_sm()
                .font_semibold()
                .text_color(name_color)
                // 避免长名称导致多行布局波动，保持行高稳定。
                .truncate()
                .child(config.name.clone()),
        );

    item = item.child(badges);

    if !app.runtime.busy {
        item = item.cursor_pointer();
    }

    let config_id = config.id;
    let item = item.id(("config-item", config_id));
    item.on_click(cx.listener(move |this, _event, window, cx| {
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

fn endpoint_family_tag(family: EndpointFamily) -> Option<Tag> {
    Some(match family {
        EndpointFamily::V4 => Tag::secondary().small().child("IPv4"),
        EndpointFamily::V6 => Tag::info().small().child("IPv6"),
        EndpointFamily::Dual => Tag::warning().small().child("Dual Stack"),
        EndpointFamily::Unknown => return None,
    })
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
    // 延迟到下一帧执行，避免 dialog 渲染期借用冲突。
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
