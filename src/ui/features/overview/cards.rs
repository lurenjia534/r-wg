use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{h_flex, tag::Tag, v_flex, ActiveTheme as _, IconName, Sizable as _};

use crate::ui::state::WgApp;

use super::chart::{build_sparkline_points, sparkline_chart};
use super::common::{
    copyable_metric_cell, metric_cell, overview_section, section_title, status_item,
    status_state_item, two_row_grid, vertical_rule, OverviewSectionTone,
};
use super::traffic::traffic_column;
use super::view_model::OverviewData;

pub(crate) fn running_status_card<T>(overview: &OverviewData, cx: &mut Context<T>) -> Div {
    let runtime = &overview.runtime;
    let running_name = if runtime.is_running {
        runtime.running_name_text.as_str()
    } else {
        "Tunnel idle"
    };
    let running_name: SharedString = running_name.to_string().into();
    let status_tag = if runtime.is_running {
        Tag::success().rounded_full().small().child("Connected")
    } else {
        Tag::secondary()
            .outline()
            .rounded_full()
            .small()
            .child("Idle")
    };

    overview_section(
        OverviewSectionTone::Primary,
        section_title(
            IconName::PanelBottom,
            "Runtime Health",
            Some("Live session metrics and transport state"),
            OverviewSectionTone::Primary,
            cx,
        ),
        v_flex()
            .gap_4()
            .child(
                h_flex()
                    .items_start()
                    .justify_between()
                    .gap_3()
                    .flex_wrap()
                    .child(
                        v_flex()
                            .gap_1()
                            .min_w(px(220.0))
                            .child(h_flex().items_center().gap_2().child(status_tag).when(
                                runtime.is_running && runtime.quantum_protected,
                                |this| {
                                    this.child(
                                        Tag::secondary()
                                            .rounded_full()
                                            .small()
                                            .child("Quantum protected"),
                                    )
                                },
                            )
                            .when(runtime.is_running && runtime.daita_active, |this| {
                                this.child(Tag::secondary().rounded_full().small().child("DAITA"))
                            }))
                            .child(
                                div()
                                    .text_xl()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(running_name),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("Last updated {}", runtime.last_updated_text)),
                            ),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap_3()
                            .flex_wrap()
                            .child(
                                metric_cell(
                                    IconName::LoaderCircle,
                                    "Uptime",
                                    &runtime.uptime_text,
                                    cx.theme().muted_foreground,
                                    false,
                                    cx,
                                )
                                .min_w(px(148.0)),
                            )
                            .child(
                                status_item(
                                    IconName::CircleUser,
                                    "Peers",
                                    &runtime.peer_count_text,
                                    cx.theme().muted_foreground,
                                    false,
                                    cx,
                                )
                                .min_w(px(128.0)),
                            )
                            .child(
                                status_item(
                                    IconName::ExternalLink,
                                    "Handshake",
                                    &runtime.handshake_text,
                                    cx.theme().muted_foreground,
                                    false,
                                    cx,
                                )
                                .min_w(px(168.0)),
                            ),
                    ),
            )
            .child(two_row_grid(
                [
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
                    status_state_item(runtime.is_running, cx),
                ],
                [
                    status_item(
                        IconName::PanelBottom,
                        "Running Tunnel",
                        &runtime.running_name_text,
                        cx.theme().muted_foreground,
                        false,
                        cx,
                    ),
                    status_item(
                        IconName::LayoutDashboard,
                        "Memory",
                        &runtime.memory_text,
                        cx.theme().muted_foreground,
                        false,
                        cx,
                    ),
                    status_item(
                        IconName::LoaderCircle,
                        "Updated",
                        &runtime.last_updated_text,
                        cx.theme().muted_foreground,
                        false,
                        cx,
                    ),
                ],
                cx,
            ))
            .when(runtime.is_running && runtime.daita_active && runtime.daita_stats_active, |this| {
                this.child(
                    v_flex()
                        .gap_2()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(cx.theme().muted_foreground)
                                .child("DAITA overhead"),
                        )
                        .child(
                            div()
                                .grid()
                                .grid_cols(4)
                                .gap_3()
                                .child(
                                    metric_cell(
                                        IconName::ArrowUp,
                                        "TX Padding",
                                        &runtime.daita_tx_padding_text,
                                        cx.theme().chart_1,
                                        false,
                                        cx,
                                    ),
                                )
                                .child(
                                    metric_cell(
                                        IconName::ArrowUp,
                                        "TX Decoy",
                                        &runtime.daita_tx_decoy_text,
                                        cx.theme().warning,
                                        false,
                                        cx,
                                    ),
                                )
                                .child(
                                    metric_cell(
                                        IconName::ArrowDown,
                                        "RX Padding",
                                        &runtime.daita_rx_padding_text,
                                        cx.theme().chart_2,
                                        false,
                                        cx,
                                    ),
                                )
                                .child(
                                    metric_cell(
                                        IconName::ArrowDown,
                                        "RX Decoy",
                                        &runtime.daita_rx_decoy_text,
                                        cx.theme().success,
                                        false,
                                        cx,
                                    ),
                                ),
                        ),
                )
            }),
        cx,
    )
}

pub(crate) fn network_status_card<T>(
    app_handle: &Entity<WgApp>,
    overview: &OverviewData,
    cx: &mut Context<T>,
) -> Div {
    let preview = &overview.preview;
    let selected_name: SharedString = preview.selected_name_text.clone().into();
    let preview_tag = if preview.has_selection {
        Tag::secondary()
            .outline()
            .rounded_full()
            .small()
            .child("Selected preview")
    } else {
        Tag::secondary()
            .rounded_full()
            .small()
            .child("No selection")
    };

    overview_section(
        OverviewSectionTone::Secondary,
        section_title(
            IconName::Globe,
            "Selected Config Preview",
            Some("Saved config values shown as a stable reference"),
            OverviewSectionTone::Secondary,
            cx,
        ),
        v_flex()
            .gap_4()
            .child(
                h_flex()
                    .items_start()
                    .justify_between()
                    .gap_3()
                    .flex_wrap()
                    .child(
                        v_flex()
                            .gap_1()
                            .min_w(px(220.0))
                            .child(preview_tag)
                            .child(
                                div()
                                    .text_xl()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(selected_name),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(preview.context_text.clone()),
                            ),
                    )
                    .child(
                        status_item(
                            IconName::FolderOpen,
                            "Source",
                            &preview.source_text,
                            cx.theme().muted_foreground,
                            false,
                            cx,
                        )
                        .min_w(px(220.0)),
                    ),
            )
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
                        IconName::Map,
                        "Route Table",
                        &preview.route_table_text,
                        cx.theme().muted_foreground,
                        true,
                        cx,
                    ),
                    status_item(
                        IconName::SortAscending,
                        "Allowed IPs",
                        &preview.allowed_text,
                        cx.theme().muted_foreground,
                        true,
                        cx,
                    ),
                ],
                cx,
            )),
        cx,
    )
}

pub(crate) fn traffic_stats_card<T>(overview: &OverviewData, cx: &mut Context<T>) -> Div {
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
            Some("Current throughput and accumulated transfer"),
            OverviewSectionTone::Primary,
            cx,
        ),
        h_flex()
            .gap_5()
            .items_start()
            .flex_wrap()
            .child(
                traffic_column(
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
                )
                .min_w(px(260.0))
                .flex_1(),
            )
            .child(vertical_rule(cx).h(px(152.0)))
            .child(
                traffic_column(
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
                )
                .min_w(px(260.0))
                .flex_1(),
            ),
        cx,
    )
}
