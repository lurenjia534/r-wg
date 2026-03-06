use std::net::IpAddr;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariant, ButtonVariants},
    dialog::DialogButtonProps,
    h_flex,
    input::Input,
    scroll::Scrollbar,
    tag::Tag,
    v_virtual_list, ActiveTheme as _, Disableable as _, Icon, IconName, Selectable, Sizable as _,
    StyledExt as _, VirtualListScrollHandle, WindowExt,
};

use r_wg::backend::wg::config;

use super::super::state::{EndpointFamily, TunnelConfig, WgApp};

// 卡片固定尺寸用于虚拟化行高计算与渲染稳定性。
const PROXIES_CARD_WIDTH: f32 = 240.0;
const PROXIES_CARD_HEIGHT: f32 = 72.0;
const PROXIES_CARD_GAP: f32 = 8.0;
// 估算内容区宽度：减去左侧导航与主内容内边距。
const PROXIES_SIDEBAR_WIDTH: f32 = 230.0;
const PROXIES_HORIZONTAL_PADDING: f32 = 48.0;
// 复用滚动状态，避免每次重建滚动位置丢失。
const PROXIES_SCROLL_STATE_ID: &str = "proxies-scroll";

fn proxies_columns(window: &Window) -> usize {
    let viewport_width = window.viewport_size().width;
    // 可用宽度的估算值：窗口宽度减去左侧栏与内边距。
    let available_width = viewport_width - px(PROXIES_SIDEBAR_WIDTH + PROXIES_HORIZONTAL_PADDING);
    let available_width = available_width.max(px(PROXIES_CARD_WIDTH));
    let card_width = px(PROXIES_CARD_WIDTH);
    let gap = px(PROXIES_CARD_GAP);
    // 根据卡片宽度 + 间距估算列数，至少 1 列。
    ((available_width + gap) / (card_width + gap))
        .floor()
        .max(1.0) as usize
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
            app.selection.proxy_filtered_indices = app
                .configs
                .iter()
                .enumerate()
                .filter_map(|(idx, config)| config.name_lower.contains(&query).then_some(idx))
                .collect();
        }
        app.selection.proxy_filtered_indices.len()
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
        let columns = proxies_columns(window);
        // 以“行”为虚拟化单位，每行放 columns 张卡片。
        let row_count = (filtered_nodes + columns - 1) / columns;
        let row_height = px(PROXIES_CARD_HEIGHT);
        // 虚拟化列表需要每行的固定高度；宽度由布局自动撑满。
        let item_sizes = Rc::new(vec![size(px(0.0), row_height); row_count]);
        let scroll_handle = window
            .use_keyed_state(PROXIES_SCROLL_STATE_ID, cx, |_, _| ScrollHandle::default())
            .read(cx)
            .clone();
        let scroll_handle = VirtualListScrollHandle::from(scroll_handle);

        let list = v_virtual_list(
            cx.entity(),
            "proxy-virtual-list",
            item_sizes,
            move |this, visible_range, _window, cx| {
                // 仅渲染当前可见行，显著减少 DOM/布局/绘制成本。
                let total = if use_filter {
                    this.selection.proxy_filtered_indices.len()
                } else {
                    this.configs.len()
                };
                visible_range
                    .map(|row_ix| {
                        let start = row_ix * columns;
                        let end = (start + columns).min(total);
                        let mut row = div().flex().flex_row().gap_2().w_full().h(row_height);
                        // 从索引映射到真实配置，保证过滤后仍可正确选择。
                        for idx in start..end {
                            let config_idx = if use_filter {
                                this.selection.proxy_filtered_indices[idx]
                            } else {
                                idx
                            };
                            let endpoint_tag = {
                                let config = &this.configs[config_idx];
                                endpoint_family_tag(
                                    this,
                                    config.id,
                                    config.text.clone(),
                                    config.storage_path.clone(),
                                    cx,
                                )
                            };
                            let config = &this.configs[config_idx];
                            row = row.child(config_list_item(
                                this,
                                config_idx,
                                config,
                                endpoint_tag,
                                cx,
                            ));
                        }
                        row
                    })
                    .collect::<Vec<_>>()
            },
        )
        .gap_2()
        .w_full()
        .flex_1()
        // 绑定滚动句柄，支持滚动条与滚动位置同步。
        .track_scroll(&scroll_handle);

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .w_full()
            .relative()
            .child(list)
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
                                        || app.selection.selected.is_none(),
                                )
                                .on_click(cx.listener(|this, _, window, cx| {
                                    let Some(idx) = this.selection.selected else {
                                        this.set_error("Select a tunnel first");
                                        cx.notify();
                                        return;
                                    };
                                    let config = &this.configs[idx];
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

fn config_list_item(
    app: &WgApp,
    idx: usize,
    config: &TunnelConfig,
    endpoint_tag: Option<Tag>,
    cx: &mut Context<WgApp>,
) -> Stateful<Div> {
    let is_selected = app.selection.selected == Some(idx);
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
    if let Some(tag) = endpoint_tag {
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
        // 阴影改为更轻量的样式，降低 GPU 绘制负担。
        .shadow_xs()
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
    let item = item.id(("config-item", idx));
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
        this.select_tunnel(idx, window, cx);
    }))
}

fn endpoint_family_tag(
    app: &mut WgApp,
    config_id: u64,
    text: Option<SharedString>,
    storage_path: PathBuf,
    cx: &mut Context<WgApp>,
) -> Option<Tag> {
    let family = endpoint_family_for_config(app, config_id, text, storage_path, cx)?;
    let tag = match family {
        EndpointFamily::V4 => Tag::secondary().small().child("IPv4"),
        EndpointFamily::V6 => Tag::info().small().child("IPv6"),
        EndpointFamily::Dual => Tag::warning().small().child("Dual Stack"),
        EndpointFamily::Unknown => return None,
    };
    Some(tag)
}

fn endpoint_family_for_config(
    app: &mut WgApp,
    config_id: u64,
    text: Option<SharedString>,
    storage_path: PathBuf,
    cx: &mut Context<WgApp>,
) -> Option<EndpointFamily> {
    if let Some(family) = app.selection.proxy_endpoint_family.get(&config_id) {
        return Some(*family);
    }
    if app.selection.proxy_endpoint_loading.contains(&config_id) {
        return None;
    }

    let text = text.or_else(|| app.cached_config_text(&storage_path));
    if let Some(text) = text {
        let hint = endpoint_family_hint_from_text(text.as_ref());
        if hint.pending_hosts.is_empty() {
            app.selection
                .proxy_endpoint_family
                .insert(config_id, hint.base_family);
            return Some(hint.base_family);
        }

        app.selection.proxy_endpoint_loading.insert(config_id);
        let id = config_id;
        let pending_hosts = hint.pending_hosts;
        let base_family = hint.base_family;
        cx.spawn(async move |view, cx| {
            let resolve_task = cx.background_spawn(async move {
                resolve_endpoint_family(base_family, pending_hosts).await
            });
            let family = resolve_task.await;
            view.update(cx, |this, cx| {
                this.selection.proxy_endpoint_loading.remove(&id);
                this.selection.proxy_endpoint_family.insert(id, family);
                cx.notify();
            })
            .ok();
        })
        .detach();

        return None;
    }

    app.selection.proxy_endpoint_loading.insert(config_id);
    let id = config_id;
    let path = storage_path;
    cx.spawn(async move |view, cx| {
        let resolve_task = cx.background_spawn(async move {
            let text = std::fs::read_to_string(&path).ok()?;
            let family = resolve_endpoint_family_from_text(text).await;
            Some(family)
        });
        let result = resolve_task.await;
        view.update(cx, |this, cx| {
            this.selection.proxy_endpoint_loading.remove(&id);
            if let Some(family) = result {
                this.selection.proxy_endpoint_family.insert(id, family);
                cx.notify();
            }
        })
        .ok();
    })
    .detach();

    None
}

struct EndpointFamilyHint {
    base_family: EndpointFamily,
    pending_hosts: Vec<(String, u16)>,
}

fn endpoint_family_hint_from_text(text: &str) -> EndpointFamilyHint {
    let parsed = config::parse_config(text);
    let Ok(parsed) = parsed else {
        return EndpointFamilyHint {
            base_family: EndpointFamily::Unknown,
            pending_hosts: Vec::new(),
        };
    };
    endpoint_family_hint_from_config(&parsed)
}

fn endpoint_family_hint_from_config(cfg: &config::WireGuardConfig) -> EndpointFamilyHint {
    let mut has_v4 = false;
    let mut has_v6 = false;
    let mut pending_hosts = Vec::new();

    for peer in &cfg.peers {
        let Some(endpoint) = &peer.endpoint else {
            continue;
        };
        let host = endpoint.host.trim();
        if host.is_empty() {
            continue;
        }
        if let Ok(addr) = host.parse::<IpAddr>() {
            if addr.is_ipv4() {
                has_v4 = true;
            } else {
                has_v6 = true;
            }
            continue;
        }

        if host.contains(':') {
            continue;
        }

        pending_hosts.push((host.to_string(), endpoint.port));
    }

    let base_family = endpoint_family_from_flags(has_v4, has_v6);
    if base_family == EndpointFamily::Dual {
        pending_hosts.clear();
    }

    EndpointFamilyHint {
        base_family,
        pending_hosts,
    }
}

async fn resolve_endpoint_family_from_text(text: String) -> EndpointFamily {
    let hint = endpoint_family_hint_from_text(&text);
    if hint.pending_hosts.is_empty() {
        return hint.base_family;
    }
    resolve_endpoint_family(hint.base_family, hint.pending_hosts).await
}

async fn resolve_endpoint_family(
    base_family: EndpointFamily,
    pending_hosts: Vec<(String, u16)>,
) -> EndpointFamily {
    if base_family == EndpointFamily::Dual {
        return EndpointFamily::Dual;
    }

    let mut has_v4 = base_family == EndpointFamily::V4;
    let mut has_v6 = base_family == EndpointFamily::V6;

    for (host, port) in pending_hosts {
        let addrs = tokio::net::lookup_host((host.as_str(), port)).await;
        if let Ok(addrs) = addrs {
            for addr in addrs {
                if addr.is_ipv4() {
                    has_v4 = true;
                } else {
                    has_v6 = true;
                }
            }
        }
    }

    endpoint_family_from_flags(has_v4, has_v6)
}

fn endpoint_family_from_flags(has_v4: bool, has_v6: bool) -> EndpointFamily {
    match (has_v4, has_v6) {
        (true, true) => EndpointFamily::Dual,
        (true, false) => EndpointFamily::V4,
        (false, true) => EndpointFamily::V6,
        _ => EndpointFamily::Unknown,
    }
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
