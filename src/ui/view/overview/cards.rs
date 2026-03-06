use gpui::*;
use gpui_component::{
    divider::Divider,
    group_box::{GroupBox, GroupBoxVariants},
    h_flex, v_flex, ActiveTheme as _, IconName,
};

use crate::ui::state::WgApp;
use crate::ui::view::data::OverviewData;

use super::chart::{build_sparkline_points, sparkline_chart};
use super::common::{
    card_title, metric_cell, status_item, status_state_item, two_row_grid, vertical_rule,
};
use super::traffic::traffic_column;

pub(super) fn running_status_card(overview: &OverviewData, cx: &mut Context<WgApp>) -> GroupBox {
    let border = cx.theme().border;
    GroupBox::new()
        .fill()
        .title(card_title(
            IconName::PanelBottom,
            "Running Status",
            None,
            cx,
        ))
        .child(
            v_flex()
                .gap_0()
                .child(two_row_grid(
                    [
                        metric_cell(
                            IconName::LoaderCircle,
                            "Uptime",
                            &overview.uptime_text,
                            rgb(0x3a8bd6),
                            cx,
                        ),
                        metric_cell(
                            IconName::ArrowDown,
                            "RX",
                            &overview.rx_total_text,
                            rgb(0xf59e0b),
                            cx,
                        ),
                        metric_cell(
                            IconName::ArrowUp,
                            "TX",
                            &overview.tx_total_text,
                            rgb(0x2dd4bf),
                            cx,
                        ),
                    ],
                    [
                        status_state_item(overview.is_running, cx),
                        status_item(
                            IconName::CircleUser,
                            "Peers",
                            &overview.peer_count_text,
                            rgb(0x60a5fa),
                            cx,
                        ),
                        status_item(
                            IconName::ExternalLink,
                            "Handshake",
                            &overview.handshake_text,
                            rgb(0xa3a3a3),
                            cx,
                        ),
                    ],
                    cx,
                ))
                .child(
                    metric_cell(
                        IconName::LayoutDashboard,
                        "Memory",
                        &overview.memory_text,
                        rgb(0x22d3ee),
                        cx,
                    )
                    .w_full()
                    .border_t_1()
                    .border_color(border),
                ),
        )
}

pub(super) fn network_status_card(overview: &OverviewData, cx: &mut Context<WgApp>) -> GroupBox {
    GroupBox::new()
        .fill()
        .title(card_title(
            IconName::Globe,
            "Network Status",
            Some(IconName::Redo),
            cx,
        ))
        .child(two_row_grid(
            [
                metric_cell(
                    IconName::ArrowUp,
                    "Local IP",
                    &overview.local_ip_text,
                    rgb(0x22c55e),
                    cx,
                ),
                metric_cell(
                    IconName::Search,
                    "DNS",
                    &overview.dns_text,
                    rgb(0x22c55e),
                    cx,
                ),
                metric_cell(
                    IconName::Globe,
                    "Endpoint",
                    &overview.endpoint_text,
                    rgb(0x22c55e),
                    cx,
                ),
            ],
            [
                status_item(
                    IconName::Globe,
                    "Network",
                    &overview.network_name_text,
                    rgb(0x38bdf8),
                    cx,
                ),
                status_item(
                    IconName::Map,
                    "Route",
                    &overview.route_table_text,
                    rgb(0x60a5fa),
                    cx,
                ),
                status_item(
                    IconName::SortAscending,
                    "Allowed IPs",
                    &overview.allowed_text,
                    rgb(0x22c55e),
                    cx,
                ),
            ],
            cx,
        ))
}

pub(super) fn traffic_stats_card(overview: &OverviewData, cx: &mut Context<WgApp>) -> GroupBox {
    let upload_sparkline = sparkline_chart(
        build_sparkline_points(&overview.upload_series),
        rgb(0x6366f1),
    );
    let download_sparkline = sparkline_chart(
        build_sparkline_points(&overview.download_series),
        rgb(0x22d3ee),
    );

    GroupBox::new()
        .fill()
        .title(card_title(IconName::ChartPie, "Traffic Stats", None, cx))
        .child(Divider::horizontal().color(cx.theme().border))
        .child(
            h_flex()
                .gap_6()
                .items_start()
                .child(traffic_column(
                    super::traffic::TrafficColumnProps {
                        icon: IconName::ArrowUp,
                        label: "Upload Speed",
                        footer_label: "Upload",
                        speed: &overview.upload_speed_text,
                        total: &overview.upload_total_text,
                        color: rgb(0x6366f1).into(),
                        sparkline: upload_sparkline,
                    },
                    cx,
                ))
                .child(vertical_rule(cx).h(px(160.0)))
                .child(traffic_column(
                    super::traffic::TrafficColumnProps {
                        icon: IconName::ArrowDown,
                        label: "Download Speed",
                        footer_label: "Download",
                        speed: &overview.download_speed_text,
                        total: &overview.download_total_text,
                        color: rgb(0x22d3ee).into(),
                        sparkline: download_sparkline,
                    },
                    cx,
                )),
        )
}
