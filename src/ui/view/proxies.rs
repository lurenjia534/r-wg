use std::rc::Rc;

use gpui::*;
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, StyledExt as _,
    VirtualListScrollHandle, button::Button, h_flex, input::Input, scroll::Scrollbar, tag::Tag,
    v_virtual_list,
};

use super::super::state::{ConfigSource, TunnelConfig, WgApp};

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
    let available_width =
        viewport_width - px(PROXIES_SIDEBAR_WIDTH + PROXIES_HORIZONTAL_PADDING);
    let available_width = available_width.max(px(PROXIES_CARD_WIDTH));
    let card_width = px(PROXIES_CARD_WIDTH);
    let gap = px(PROXIES_CARD_GAP);
    // 根据卡片宽度 + 间距估算列数，至少 1 列。
    ((available_width + gap) / (card_width + gap))
        .floor()
        .max(1.0) as usize
}

/// Proxies 页面：配置列表入口，用于快速选择隧道。
pub(crate) fn render_proxies(
    app: &mut WgApp,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) -> Div {
    // 初始化搜索输入框并读取当前查询，用于过滤节点列表。
    app.ensure_proxy_search_input(window, cx);
    let search_input = app
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
        if app.proxy_filter_query != query || app.proxy_filter_total != total_nodes {
            app.proxy_filter_query = query.clone();
            app.proxy_filter_total = total_nodes;
            app.proxy_filtered_indices = app
                .configs
                .iter()
                .enumerate()
                .filter_map(|(idx, config)| {
                    config
                        .name_lower
                        .contains(&query)
                        .then_some(idx)
                })
                .collect();
        }
        app.proxy_filtered_indices.len()
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
                let indices = &this.proxy_filtered_indices;
                let total = if use_filter {
                    indices.len()
                } else {
                    this.configs.len()
                };
                visible_range
                    .map(|row_ix| {
                        let start = row_ix * columns;
                        let end = (start + columns).min(total);
                        let mut row = div()
                            .flex()
                            .flex_row()
                            .gap_2()
                            .w_full()
                            .h(row_height);
                        // 从索引映射到真实配置，保证过滤后仍可正确选择。
                        for idx in start..end {
                            let config_idx = if use_filter { indices[idx] } else { idx };
                            let config = &this.configs[config_idx];
                            row = row.child(config_list_item(this, config_idx, config, cx));
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
                        .child(
                            Button::new("cfg-list-import")
                                .icon(Icon::new(IconName::FolderOpen).size_3())
                                .label("Import")
                                .outline()
                                .xsmall()
                                .disabled(app.busy)
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
    cx: &mut Context<WgApp>,
) -> Stateful<Div> {
    let is_selected = app.selected == Some(idx);
    let is_running = app.running_name.as_deref() == Some(config.name.as_str());
    let name_color = cx.theme().foreground;
    let bg = if is_selected {
        cx.theme().accent.alpha(0.16)
    } else {
        cx.theme().secondary
    };
    let border_color = if is_selected {
        cx.theme().accent
    } else if cx.theme().is_dark() {
        cx.theme().foreground.alpha(0.12)
    } else {
        cx.theme().border
    };

    let mut badges = h_flex()
        .gap_1()
        .child(Tag::secondary().small().child(config_source_label(&config.source)));
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

    if !app.busy {
        item = item.cursor_pointer();
    }

    let item = item.id(("config-item", idx));
    item.on_click(cx.listener(move |this, _event, window, cx| {
        if this.busy {
            return;
        }
        this.select_tunnel(idx, window, cx);
    }))
}

fn config_source_label(source: &ConfigSource) -> &'static str {
    match source {
        ConfigSource::File { .. } => "File",
        ConfigSource::Paste => "Pasted",
    }
}
