use gpui::*;
use gpui::prelude::FluentBuilder as _;

use gpui_component::{
    ActiveTheme as _, Icon, IconName, StyledExt as _, divider::Divider,
    group_box::{GroupBox, GroupBoxVariants},
    h_flex, v_flex,
};

use super::data::ViewData;
use super::super::state::WgApp;

/// Overview 页：两张状态卡片（运行状态 / 网络状态）。
pub(crate) fn render_overview(
    app: &mut WgApp,
    data: &ViewData,
    cx: &mut Context<WgApp>,
) -> Div {
    let uptime = format_uptime(app);
    let rx = super::super::format::format_bytes(data.peer_summary.rx_bytes);
    let tx = super::super::format::format_bytes(data.peer_summary.tx_bytes);
    let peers = data.peer_summary.peer_count.to_string();
    let handshake = data.last_handshake.clone();

    let local_ip = format_local_ip(data);
    let dns = format_dns(data);
    let endpoint = format_endpoint(data);
    let allowed = format_allowed_summary(data);
    let network_name = app
        .running_name
        .clone()
        .unwrap_or_else(|| "-".to_string());
    let route_table = data
        .parsed_config
        .as_ref()
        .map(|cfg| super::super::format::format_route_table(cfg.interface.table))
        .unwrap_or_else(|| "-".to_string());

    let status_text = if app.running { "On" } else { "Off" };
    let status_color = if app.running {
        rgb(0x22c55e)
    } else {
        rgb(0x64748b)
    };

    div()
        .flex()
        .flex_col()
        .gap_3()
        .flex_grow()
        .child(
            div()
                .flex()
                .flex_row()
                .gap_3()
                .child(
                    running_status_card(
                        cx,
                        &uptime,
                        &rx,
                        &tx,
                        status_text,
                        status_color,
                        &peers,
                        &handshake,
                    )
                    .flex_grow(),
                )
                .child(
                    network_status_card(
                        cx,
                        &local_ip,
                        &dns,
                        &endpoint,
                        &network_name,
                        &route_table,
                        &allowed,
                    )
                    .flex_grow(),
                ),
        )
}

/// 其它菜单项的占位页。
pub(crate) fn render_placeholder(cx: &mut Context<WgApp>) -> Div {
    div().child(
        GroupBox::new()
            .fill()
            .title("Coming Soon")
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("This section is under construction."),
            ),
    )
}

fn running_status_card(
    cx: &mut Context<WgApp>,
    uptime: &str,
    rx: &str,
    tx: &str,
    status_text: &str,
    status_color: Rgba,
    peers: &str,
    handshake: &str,
) -> GroupBox {
    GroupBox::new()
        .fill()
        .title(card_title(
            IconName::PanelBottom,
            "Running Status",
            None,
            cx,
        ))
        .child(
            h_flex()
                .gap_4()
                .items_start()
                .child(metric_cell(
                    IconName::LoaderCircle,
                    "Uptime",
                    uptime,
                    rgb(0x3a8bd6),
                    cx,
                ))
                .child(vertical_rule(cx))
                .child(metric_cell(
                    IconName::ArrowDown,
                    "RX",
                    rx,
                    rgb(0xf59e0b),
                    cx,
                ))
                .child(vertical_rule(cx))
                .child(metric_cell(
                    IconName::ArrowUp,
                    "TX",
                    tx,
                    rgb(0x2dd4bf),
                    cx,
                )),
        )
        .child(Divider::horizontal().color(cx.theme().border))
        .child(
            h_flex()
                .gap_4()
                .items_start()
                .child(status_item(
                    IconName::CircleCheck,
                    "Status",
                    status_text,
                    status_color,
                    cx,
                ))
                .child(vertical_rule(cx))
                .child(status_item(
                    IconName::CircleUser,
                    "Peers",
                    peers,
                    rgb(0x60a5fa),
                    cx,
                ))
                .child(vertical_rule(cx))
                .child(status_item(
                    IconName::ExternalLink,
                    "Handshake",
                    handshake,
                    rgb(0xa3a3a3),
                    cx,
                )),
        )
}

fn network_status_card(
    cx: &mut Context<WgApp>,
    local_ip: &str,
    dns: &str,
    endpoint: &str,
    network_name: &str,
    route_table: &str,
    allowed: &str,
) -> GroupBox {
    GroupBox::new()
        .fill()
        .title(card_title(
            IconName::Globe,
            "Network Status",
            Some(IconName::Redo),
            cx,
        ))
        .child(
            h_flex()
                .gap_4()
                .items_start()
                .child(metric_cell(
                    IconName::ArrowUp,
                    "Local IP",
                    local_ip,
                    rgb(0x22c55e),
                    cx,
                ))
                .child(vertical_rule(cx))
                .child(metric_cell(
                    IconName::Search,
                    "DNS",
                    dns,
                    rgb(0x22c55e),
                    cx,
                ))
                .child(vertical_rule(cx))
                .child(metric_cell(
                    IconName::Globe,
                    "Endpoint",
                    endpoint,
                    rgb(0x22c55e),
                    cx,
                )),
        )
        .child(Divider::horizontal().color(cx.theme().border))
        .child(
            h_flex()
                .gap_4()
                .items_start()
                .child(status_item(
                    IconName::Globe,
                    "Network",
                    network_name,
                    rgb(0x38bdf8),
                    cx,
                ))
                .child(vertical_rule(cx))
                .child(status_item(
                    IconName::Map,
                    "Route",
                    route_table,
                    rgb(0x60a5fa),
                    cx,
                ))
                .child(vertical_rule(cx))
                .child(status_item(
                    IconName::SortAscending,
                    "Allowed IPs",
                    allowed,
                    rgb(0x22c55e),
                    cx,
                )),
        )
}

fn card_title(
    icon: IconName,
    label: &str,
    trailing_icon: Option<IconName>,
    cx: &mut Context<WgApp>,
) -> Div {
    h_flex()
        .items_center()
        .justify_between()
        .child(
            h_flex()
                .items_center()
                .gap_2()
                .child(Icon::new(icon).size_4().text_color(cx.theme().accent))
                .child(div().text_base().font_semibold().child(label.to_string())),
        )
        .when_some(trailing_icon, |this, icon| {
            this.child(
                Icon::new(icon)
                    .size_4()
                    .text_color(cx.theme().muted_foreground),
            )
        })
}

fn metric_cell(
    icon: IconName,
    label: &str,
    value: &str,
    color: impl Into<Hsla>,
    cx: &mut Context<WgApp>,
) -> Div {
    let color: Hsla = color.into();
    v_flex()
        .gap_1()
        .child(
            h_flex()
                .items_center()
                .gap_2()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(Icon::new(icon).size_4().text_color(color))
                .child(label.to_string()),
        )
        .child(
            div()
                .text_2xl()
                .font_semibold()
                .text_color(color)
                .child(value.to_string()),
        )
}

fn status_item(
    icon: IconName,
    label: &str,
    value: &str,
    color: impl Into<Hsla>,
    cx: &mut Context<WgApp>,
) -> Div {
    let color: Hsla = color.into();
    v_flex()
        .gap_1()
        .child(
            h_flex()
                .items_center()
                .gap_2()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(Icon::new(icon).size_3().text_color(color))
                .child(label.to_string()),
        )
        .child(
            div()
                .text_base()
                .font_medium()
                .text_color(cx.theme().foreground)
                .child(value.to_string()),
        )
}

fn vertical_rule(cx: &mut Context<WgApp>) -> Div {
    div()
        .w(px(1.0))
        .h(px(64.0))
        .bg(cx.theme().border)
}

fn format_uptime(app: &WgApp) -> String {
    let Some(start) = app.started_at else {
        return "0:00".to_string();
    };
    let elapsed = start.elapsed();
    let total_secs = elapsed.as_secs();
    let minutes = total_secs / 60;
    let seconds = total_secs % 60;
    if minutes >= 60 {
        let hours = minutes / 60;
        let mins = minutes % 60;
        format!("{hours}:{mins:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

fn format_local_ip(data: &ViewData) -> String {
    data.parsed_config
        .as_ref()
        .and_then(|cfg| cfg.interface.addresses.first())
        .map(|addr| format!("{}/{}", addr.addr, addr.cidr))
        .unwrap_or_else(|| "-".to_string())
}

fn format_dns(data: &ViewData) -> String {
    data.parsed_config
        .as_ref()
        .and_then(|cfg| cfg.interface.dns_servers.first())
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "-".to_string())
}

fn format_endpoint(data: &ViewData) -> String {
    data.parsed_config
        .as_ref()
        .and_then(|cfg| cfg.peers.first())
        .and_then(|peer| peer.endpoint.as_ref())
        .map(|endpoint| format!("{}:{}", endpoint.host, endpoint.port))
        .unwrap_or_else(|| "-".to_string())
}

fn format_allowed_summary(data: &ViewData) -> String {
    let count = data
        .parsed_config
        .as_ref()
        .map(|cfg| cfg.peers.iter().map(|peer| peer.allowed_ips.len()).sum::<usize>())
        .unwrap_or(0);
    if count == 0 {
        "-".to_string()
    } else {
        format!("{count} routes")
    }
}
