use gpui::*;
use gpui_component::{
    ActiveTheme as _, description_list::DescriptionList, group_box::{GroupBox, GroupBoxVariants},
};

use super::data::ViewData;
use super::widgets::tab_button;
use super::super::format::{
    format_addresses, format_allowed_ips, format_bytes, format_dns, format_peer_line,
    format_route_table,
};
use super::super::state::{RightTab, WgApp};

/// 右侧面板：状态/日志切换与卡片内容展示。
pub(crate) fn render_right_panel(
    app: &mut WgApp,
    data: &ViewData,
    cx: &mut Context<WgApp>,
) -> Div {
    // 顶部标签切换（状态/日志）。
    let right_tab_row = div()
        .flex()
        .gap_2()
        .child(tab_button(
            "Status",
            app.right_tab == RightTab::Status,
            cx,
            |this| this.right_tab = RightTab::Status,
        ))
        .child(tab_button(
            "Logs",
            app.right_tab == RightTab::Logs,
            cx,
            |this| this.right_tab = RightTab::Logs,
        ));

    // Network 卡片：展示本机地址/DNS/路由表/Allowed IPs。
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

    // Connection 卡片：展示连接状态与流量统计。
    let status_card = {
        let connection_state = if app.running { "Connected" } else { "Idle" };
        let active_tunnel = app
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

    // Peers 卡片：列出握手与流量详情。
    let peers_card = {
        let mut stats_items = Vec::new();
        stats_items.push(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(app.stats_note.clone())
                .into_any_element(),
        );
        if app.peer_stats.is_empty() {
            stats_items.push(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("No peer stats yet")
                    .into_any_element(),
            );
        } else {
            stats_items.extend(app.peer_stats.iter().map(|peer| {
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

    // Logs 卡片：集中显示最近状态与错误信息。
    let logs_card = {
        let last_error = app
            .last_error
            .clone()
            .unwrap_or_else(|| "None".into());
        let parse_state = data.parse_error.clone().unwrap_or_else(|| "None".to_string());
        GroupBox::new().fill().title("Logs").child(
            DescriptionList::new()
                .columns(1)
                .item("Latest Status", app.status.to_string(), 1)
                .item("Last Error", last_error.to_string(), 1)
                .item("Parse Error", parse_state, 1),
        )
    };

    // 根据标签切换右侧内容。
    let right_body = match app.right_tab {
        RightTab::Status => div()
            .flex()
            .flex_col()
            .gap_3()
            .child(status_card)
            .child(network_card)
            .child(peers_card)
            .into_any_element(),
        RightTab::Logs => div().flex().flex_col().gap_3().child(logs_card).into_any_element(),
    };

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
        .child(right_body)
}
