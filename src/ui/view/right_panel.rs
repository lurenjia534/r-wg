use gpui::*;
use gpui_component::{
    button::{Button, ButtonGroup},
    description_list::DescriptionList,
    group_box::{GroupBox, GroupBoxVariants},
    h_flex,
    scroll::ScrollableElement,
    ActiveTheme as _, Disableable as _, Selectable as _, Sizable as _,
};

use super::super::format::{
    format_addresses, format_allowed_ips, format_bytes, format_dns, format_peer_line,
    format_route_table,
};
use super::super::state::{RightTab, WgApp};
use super::data::ViewData;

/// 渲染右侧状态面板。
///
/// 这个面板承担两类职责：
/// 1. 展示当前连接、网络、对端和日志摘要；
/// 2. 为最新状态和最后错误提供显式复制入口。
///
/// 第二点是这次修复的重要兜底：即使 Windows 系统通知本身不可直接复制，
/// 用户仍然可以在应用内复制同一条状态或错误文本。
pub(crate) fn render_right_panel(app: &mut WgApp, data: &ViewData, cx: &mut Context<WgApp>) -> Div {
    // 顶部页签切换：在“状态”和“日志”两个视图之间切换右侧面板内容。
    let right_tab_row = ButtonGroup::new("right-panel-tabs")
        .outline()
        .compact()
        .small()
        .child(
            Button::new("right-panel-tab-status")
                .label("Status")
                .selected(app.ui_session.right_tab == RightTab::Status)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.set_session_right_tab(RightTab::Status, cx);
                })),
        )
        .child(
            Button::new("right-panel-tab-logs")
                .label("Logs")
                .selected(app.ui_session.right_tab == RightTab::Logs)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.set_session_right_tab(RightTab::Logs, cx);
                })),
        );

    // 网络信息卡片：展示当前配置解析出的地址、DNS、路由表和 Allowed IPs。
    let network_card = {
        let addresses = data
            .parsed_config
            .as_ref()
            .map(|cfg| format_addresses(&cfg.interface))
            .unwrap_or_else(|| "No config selected".to_string());
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

        GroupBox::new().fill().title("Network").child(
            DescriptionList::new()
                .columns(1)
                .item("Local Address", addresses, 1)
                .item("DNS", dns, 1)
                .item("Route Table", route_table, 1)
                .item("Allowed IPs", routes, 1),
        )
    };

    // 连接状态卡片：展示当前连接状态、激活中的隧道、最近握手和收发流量。
    let status_card = {
        let connection_state = if app.runtime.running {
            "Connected"
        } else {
            "Idle"
        };
        let active_tunnel = app
            .runtime
            .running_name
            .clone()
            .unwrap_or_else(|| "-".to_string());
        let rx = format_bytes(data.peer_summary.rx_bytes);
        let tx = format_bytes(data.peer_summary.tx_bytes);
        let peers = if data.peer_summary.peer_count == 0 {
            "0".to_string()
        } else {
            data.peer_summary.peer_count.to_string()
        };
        GroupBox::new().fill().title("Connection").child(
            DescriptionList::new()
                .columns(1)
                .item("Status", connection_state, 1)
                .item("Tunnel", active_tunnel, 1)
                .item("Peers", peers, 1)
                .item("Handshake", data.last_handshake.clone(), 1)
                .item("RX", rx, 1)
                .item("TX", tx, 1),
        )
    };

    // 对端状态卡片：列出对端统计摘要。
    // 当还没有统计数据时，显示说明性占位文本，避免面板空白。
    let peers_card = {
        let mut stats_items = Vec::new();
        stats_items.push(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(app.stats.stats_note.clone())
                .into_any_element(),
        );
        if app.stats.peer_stats.is_empty() {
            stats_items.push(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("No peer stats yet")
                    .into_any_element(),
            );
        } else {
            stats_items.extend(app.stats.peer_stats.iter().map(|peer| {
                div()
                    .text_sm()
                    .child(format_peer_line(peer))
                    .into_any_element()
            }));
        }
        GroupBox::new()
            .fill()
            .title("Peers")
            .child(div().flex().flex_col().gap_1().children(stats_items))
    };

    // 日志摘要卡片：集中显示最近状态、最后错误和解析错误。
    //
    // 这里新增两个复制按钮：
    // - Copy Status：复制最近状态文本
    // - Copy Last Error：复制最后一条错误文本
    //
    // 这样即使系统通知已经消失，或者系统通知本身不可直接选中，
    // 用户依然能从应用内复制同一条诊断信息。
    let logs_card = {
        let latest_status = app.ui.status.to_string();
        let last_error = app.ui.last_error.clone().unwrap_or_else(|| "None".into());
        let last_error_text = last_error.to_string();
        let parse_state = data
            .parse_error
            .clone()
            .unwrap_or_else(|| "None".to_string());
        GroupBox::new().fill().title("Logs").child(
            div()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(
                            // 复制最近状态，用于快速分享当前运行阶段提示。
                            Button::new("copy-latest-status")
                                .label("Copy Status")
                                .outline()
                                .small()
                                .compact()
                                .on_click({
                                    let latest_status = latest_status.clone();
                                    cx.listener(move |this, _, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            latest_status.clone(),
                                        ));
                                        this.set_status("Status copied to clipboard");
                                        cx.notify();
                                    })
                                }),
                        )
                        .child(
                            // 复制最后错误。
                            // 当没有错误时禁用按钮，避免把占位文本 `None` 误复制到剪贴板。
                            Button::new("copy-last-error")
                                .label("Copy Last Error")
                                .outline()
                                .small()
                                .compact()
                                .disabled(last_error_text == "None")
                                .on_click({
                                    let last_error_text = last_error_text.clone();
                                    cx.listener(move |this, _, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            last_error_text.clone(),
                                        ));
                                        this.set_status("Last error copied to clipboard");
                                        cx.notify();
                                    })
                                }),
                        ),
                )
                .child(
                    DescriptionList::new()
                        .columns(1)
                        .item("Latest Status", latest_status, 1)
                        .item("Last Error", last_error_text, 1)
                        .item("Parse Error", parse_state, 1),
                ),
        )
    };

    // 根据当前页签切换右侧主体内容。
    let right_body = match app.ui_session.right_tab {
        RightTab::Status => div()
            .flex()
            .flex_col()
            .gap_3()
            .child(status_card)
            .child(network_card)
            .child(peers_card)
            .into_any_element(),
        RightTab::Logs => div()
            .flex()
            .flex_col()
            .gap_3()
            .child(logs_card)
            .into_any_element(),
    };

    let right_scroll = div()
        .id("right-panel-scroll")
        .w_full()
        .flex_1()
        .min_h(px(0.0))
        .child(right_body)
        .overflow_y_scrollbar();

    div()
        .w(px(360.0))
        .h_full()
        .flex()
        .flex_col()
        .gap_3()
        .p_3()
        .rounded_lg()
        .bg(cx.theme().tiles)
        .border_1()
        .border_color(cx.theme().border)
        .child(div().text_lg().child("Status"))
        .child(right_tab_row)
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h(px(0.0))
                .w_full()
                .overflow_hidden()
                .child(right_scroll),
        )
}
