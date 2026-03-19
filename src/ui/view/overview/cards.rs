use gpui::*;
use gpui_component::{
    divider::Divider,
    group_box::{GroupBox, GroupBoxVariants},
    h_flex, v_flex, ActiveTheme as _, IconName,
};

use crate::ui::view::data::OverviewData;

use super::chart::{build_sparkline_points, sparkline_chart};
use super::common::{
    card_title, metric_cell, status_item, status_state_item, two_row_grid, vertical_rule,
};
use super::traffic::traffic_column;

pub(super) fn running_status_card<T>(overview: &OverviewData, cx: &mut Context<T>) -> GroupBox {
    let border = cx.theme().border;
    let runtime = &overview.runtime;
    GroupBox::new()
        .fill()
        .title(card_title(
            IconName::PanelBottom,
            "Runtime Health",
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
                            &runtime.uptime_text,
                            cx.theme().muted_foreground,
                            cx,
                        ),
                        metric_cell(
                            IconName::ArrowDown,
                            "RX",
                            &runtime.rx_total_text,
                            cx.theme().chart_2,
                            cx,
                        ),
                        metric_cell(
                            IconName::ArrowUp,
                            "TX",
                            &runtime.tx_total_text,
                            cx.theme().chart_1,
                            cx,
                        ),
                    ],
                    [
                        status_state_item(runtime.is_running, cx),
                        status_item(
                            IconName::CircleUser,
                            "Peers",
                            &runtime.peer_count_text,
                            cx.theme().muted_foreground,
                            cx,
                        ),
                        status_item(
                            IconName::ExternalLink,
                            "Handshake",
                            &runtime.handshake_text,
                            cx.theme().muted_foreground,
                            cx,
                        ),
                    ],
                    cx,
                ))
                .child(Divider::horizontal().color(border))
                .child(
                    h_flex()
                        .gap_0()
                        .child(
                            status_item(
                                IconName::PanelBottom,
                                "Running Tunnel",
                                &runtime.running_name_text,
                                cx.theme().muted_foreground,
                                cx,
                            )
                            .w(relative(0.6))
                            .border_r_1()
                            .border_color(border),
                        )
                        .child(
                            status_item(
                                IconName::LayoutDashboard,
                                "Memory",
                                &runtime.memory_text,
                                cx.theme().muted_foreground,
                                cx,
                            )
                            .w(relative(0.4)),
                        ),
                ),
        )
}

pub(super) fn network_status_card<T>(overview: &OverviewData, cx: &mut Context<T>) -> GroupBox {
    let preview = &overview.preview;
    GroupBox::new()
        .fill()
        .title(card_title(
            IconName::Globe,
            "Selected Config Preview",
            None,
            cx,
        ))
        .child(
            v_flex()
                .gap_0()
                .child(two_row_grid(
                    [
                        metric_cell(
                            IconName::ArrowUp,
                            "Local IP",
                            &preview.local_ip_text,
                            cx.theme().muted_foreground,
                            cx,
                        ),
                        metric_cell(
                            IconName::Search,
                            "DNS",
                            &preview.dns_text,
                            cx.theme().muted_foreground,
                            cx,
                        ),
                        metric_cell(
                            IconName::Globe,
                            "Endpoint",
                            &preview.endpoint_text,
                            cx.theme().muted_foreground,
                            cx,
                        ),
                    ],
                    [
                        status_item(
                            IconName::PanelBottom,
                            "Selected",
                            &preview.selected_name_text,
                            cx.theme().muted_foreground,
                            cx,
                        ),
                        status_item(
                            IconName::FolderOpen,
                            "Source",
                            &preview.source_text,
                            cx.theme().muted_foreground,
                            cx,
                        ),
                        status_item(
                            IconName::Map,
                            "Route Table",
                            &preview.route_table_text,
                            cx.theme().muted_foreground,
                            cx,
                        ),
                    ],
                    cx,
                ))
                .child(
                    status_item(
                        IconName::SortAscending,
                        "Allowed IPs",
                        &preview.allowed_text,
                        cx.theme().muted_foreground,
                        cx,
                    )
                    .w_full()
                    .border_t_1()
                    .border_color(cx.theme().border),
                )
                .child(
                    div()
                        .px_4()
                        .pb_4()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(preview.context_text.clone()),
                ),
        )
}

pub(super) fn traffic_stats_card<T>(overview: &OverviewData, cx: &mut Context<T>) -> GroupBox {
    let runtime = &overview.runtime;
    let upload_sparkline = sparkline_chart(
        build_sparkline_points(&runtime.upload_series),
        cx.theme().chart_1,
    );
    let download_sparkline = sparkline_chart(
        build_sparkline_points(&runtime.download_series),
        cx.theme().chart_2,
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
                        speed: &runtime.upload_speed_text,
                        total: &runtime.upload_total_text,
                        color: cx.theme().chart_1,
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
                        speed: &runtime.download_speed_text,
                        total: &runtime.download_total_text,
                        color: cx.theme().chart_2,
                        sparkline: download_sparkline,
                    },
                    cx,
                )),
        )
}
