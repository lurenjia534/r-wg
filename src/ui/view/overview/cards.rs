use gpui::*;
use gpui_component::{group_box::GroupBox, h_flex, v_flex, ActiveTheme as _, IconName};

use crate::ui::state::WgApp;
use crate::ui::view::data::OverviewData;

use super::chart::{build_sparkline_points, sparkline_chart};
use super::common::{
    copyable_metric_cell, metric_cell, overview_section, section_title, status_item,
    status_state_item, two_row_grid, vertical_rule, OverviewSectionTone,
};
use super::traffic::traffic_column;

pub(super) fn running_status_card<T>(overview: &OverviewData, cx: &mut Context<T>) -> GroupBox {
    let runtime = &overview.runtime;
    overview_section(
        OverviewSectionTone::Primary,
        section_title(
            IconName::PanelBottom,
            "Runtime Health",
            None::<SharedString>,
            OverviewSectionTone::Primary,
            cx,
        ),
        v_flex()
            .gap_0()
            .child(two_row_grid(
                [
                    metric_cell(
                        IconName::LoaderCircle,
                        "Uptime",
                        &runtime.uptime_text,
                        cx.theme().muted_foreground,
                        false,
                        cx,
                    ),
                    metric_cell(
                        IconName::ArrowDown,
                        "RX",
                        &runtime.rx_total_text,
                        cx.theme().chart_2,
                        false,
                        cx,
                    ),
                    metric_cell(
                        IconName::ArrowUp,
                        "TX",
                        &runtime.tx_total_text,
                        cx.theme().chart_1,
                        false,
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
                        false,
                        cx,
                    ),
                    status_item(
                        IconName::ExternalLink,
                        "Handshake",
                        &runtime.handshake_text,
                        cx.theme().muted_foreground,
                        false,
                        cx,
                    ),
                ],
                cx,
            ))
            .child(
                h_flex()
                    .gap_2()
                    .pt_2()
                    .child(
                        status_item(
                            IconName::PanelBottom,
                            "Running Tunnel",
                            &runtime.running_name_text,
                            cx.theme().muted_foreground,
                            true,
                            cx,
                        )
                        .w(relative(0.6)),
                    )
                    .child(
                        status_item(
                            IconName::LayoutDashboard,
                            "Memory",
                            &runtime.memory_text,
                            cx.theme().muted_foreground,
                            true,
                            cx,
                        )
                        .w(relative(0.4)),
                    ),
            ),
        cx,
    )
}

pub(super) fn network_status_card<T>(
    app_handle: &Entity<WgApp>,
    overview: &OverviewData,
    cx: &mut Context<T>,
) -> GroupBox {
    let preview = &overview.preview;
    overview_section(
        OverviewSectionTone::Secondary,
        section_title(
            IconName::Globe,
            "Selected Config Preview",
            if preview.has_selection {
                Some(preview.context_text.clone())
            } else {
                Some("No saved config selected for preview.".to_string())
            },
            OverviewSectionTone::Secondary,
            cx,
        ),
        v_flex()
            .gap_0()
            .child(two_row_grid(
                [
                    copyable_metric_cell(
                        app_handle,
                        "overview-copy-local-ip",
                        IconName::ArrowUp,
                        "Local IP",
                        &preview.local_ip_text,
                        cx.theme().muted_foreground,
                        true,
                        cx,
                    ),
                    copyable_metric_cell(
                        app_handle,
                        "overview-copy-dns",
                        IconName::Search,
                        "DNS",
                        &preview.dns_text,
                        cx.theme().muted_foreground,
                        true,
                        cx,
                    ),
                    copyable_metric_cell(
                        app_handle,
                        "overview-copy-endpoint",
                        IconName::Globe,
                        "Endpoint",
                        &preview.endpoint_text,
                        cx.theme().muted_foreground,
                        true,
                        cx,
                    ),
                ],
                [
                    status_item(
                        IconName::PanelBottom,
                        "Selected",
                        &preview.selected_name_text,
                        cx.theme().muted_foreground,
                        false,
                        cx,
                    ),
                    status_item(
                        IconName::FolderOpen,
                        "Source",
                        &preview.source_text,
                        cx.theme().muted_foreground,
                        false,
                        cx,
                    ),
                    status_item(
                        IconName::Map,
                        "Route Table",
                        &preview.route_table_text,
                        cx.theme().muted_foreground,
                        true,
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
                    true,
                    cx,
                )
                .w_full()
                .mt_3(),
            ),
        cx,
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

    overview_section(
        OverviewSectionTone::Primary,
        section_title(
            IconName::ChartPie,
            "Traffic Stats",
            None::<SharedString>,
            OverviewSectionTone::Primary,
            cx,
        ),
        h_flex()
            .gap_4()
            .items_start()
            .p_3()
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
            .child(vertical_rule(cx).h(px(152.0)))
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
        cx,
    )
}
