use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonGroup},
    chart::{BarChart, PieChart},
    group_box::GroupBox,
    h_flex, v_flex, ActiveTheme as _, Icon, IconName, Selectable as _, Sizable as _,
    StyledExt as _,
};

use crate::ui::format::format_bytes;
use crate::ui::state::{TrafficPeriod, WgApp};

use super::chart::{format_avg_bytes, TrafficTrendOverlay};
use super::common::{
    overview_section, section_title, tile_border, tile_header, tile_icon, tile_shell, tile_surface,
    OverviewSectionTone,
};
use super::traffic_analytics::{TrafficSummaryData, TrafficTrendData};
use super::view_model::OverviewData;

pub(super) fn traffic_trend_card<T>(trend: &TrafficTrendData, cx: &mut Context<T>) -> GroupBox {
    let total_text = format_bytes(trend.total_bytes);
    let avg_text = format_avg_bytes(trend.average_bytes);
    let show_avg_rule = trend.non_zero_days >= 2;
    let show_trend_line = trend.non_zero_days >= 3;
    let today_is_peak = trend
        .points
        .iter()
        .any(|point| point.is_today && point.bytes > 0 && point.bytes == trend.peak_bytes);
    let show_peak_marker = trend.peak_bytes > 0 && !today_is_peak;
    let peak_text = if trend.peak_bytes > 0 {
        format_bytes(trend.peak_bytes)
    } else {
        "—".to_string()
    };
    let peak_detail = if trend.peak_bytes > 0 {
        trend.peak_label.clone()
    } else {
        "No traffic".to_string()
    };
    let bar_base = cx
        .theme()
        .muted_foreground
        .alpha(if cx.theme().is_dark() { 0.16 } else { 0.1 });
    let bar_today = cx
        .theme()
        .chart_3
        .alpha(if cx.theme().is_dark() { 0.42 } else { 0.28 });
    let bar_peak = cx
        .theme()
        .chart_2
        .alpha(if cx.theme().is_dark() { 0.26 } else { 0.18 });
    let avg_rule = cx
        .theme()
        .chart_4
        .alpha(if cx.theme().is_dark() { 0.68 } else { 0.54 });
    let trend_line = cx.theme().chart_2;
    let label_peak = trend.peak_bytes;

    overview_section(
        OverviewSectionTone::Primary,
        section_title(
            IconName::Calendar,
            "7-Day Traffic Trend",
            Some("Daily totals • zero-traffic days included in average"),
            OverviewSectionTone::Primary,
            cx,
        ),
        v_flex()
            .gap_3()
            .p_3()
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .items_start()
                    .child(trend_metric_tile(
                        IconName::ChartPie,
                        "7d Total",
                        total_text,
                        None,
                        cx.theme().muted_foreground,
                        cx,
                    ))
                    .when(show_avg_rule, |this| {
                        this.child(trend_metric_tile(
                            IconName::Calendar,
                            "Daily Avg",
                            avg_text.clone(),
                            Some(SharedString::from("7d mean")),
                            cx.theme().chart_4,
                            cx,
                        ))
                    })
                    .child(trend_metric_tile(
                        IconName::ExternalLink,
                        "Peak",
                        peak_text,
                        Some(peak_detail.into()),
                        cx.theme().chart_2,
                        cx,
                    )),
            )
            .child(
                h_flex()
                    .items_center()
                    .flex_wrap()
                    .gap_3()
                    .child(trend_legend_item(
                        "Daily total",
                        bar_base,
                        LegendKind::Bar,
                        cx,
                    ))
                    .when(show_avg_rule, |this| {
                        this.child(trend_legend_item("7d avg", avg_rule, LegendKind::Line, cx))
                    })
                    .when(
                        trend
                            .points
                            .iter()
                            .any(|point| point.is_today && point.bytes > 0),
                        |this| {
                            this.child(trend_legend_item(
                                "Today",
                                cx.theme().chart_3,
                                LegendKind::Dot,
                                cx,
                            ))
                        },
                    )
                    .when(show_peak_marker, |this| {
                        this.child(trend_legend_item(
                            "Peak",
                            cx.theme().chart_2,
                            LegendKind::Dot,
                            cx,
                        ))
                    }),
            )
            .child(
                h_flex()
                    .items_start()
                    .gap_2()
                    .child(trend_y_axis_rail(trend, px(224.0), cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .h(px(224.0))
                            .rounded_lg()
                            .border_1()
                            .border_color(tile_border(cx))
                            .bg(tile_surface(cx))
                            .p_3()
                            .child(
                                div()
                                    .relative()
                                    .size_full()
                                    .child(
                                        BarChart::new(trend.points.clone())
                                            .x(|point| point.label.clone())
                                            .y(|point| point.bytes as f64)
                                            .tick_margin(1)
                                            .fill(move |point| {
                                                if point.is_today {
                                                    bar_today
                                                } else if label_peak > 0
                                                    && point.bytes == label_peak
                                                {
                                                    bar_peak
                                                } else {
                                                    bar_base
                                                }
                                            })
                                            .label(move |point| {
                                                if point.bytes > 0
                                                    && (point.is_today || point.bytes == label_peak)
                                                {
                                                    format_avg_bytes(point.bytes as f64)
                                                } else {
                                                    String::new()
                                                }
                                            }),
                                    )
                                    .child(div().absolute().inset_0().child(
                                        TrafficTrendOverlay::new(
                                            trend.points.clone(),
                                            trend.average_bytes,
                                            trend.max_bytes,
                                            trend.peak_bytes,
                                            show_avg_rule,
                                            show_trend_line,
                                            show_peak_marker,
                                            avg_rule,
                                            trend_line,
                                            cx.theme().chart_3,
                                            cx.theme().chart_2,
                                        ),
                                    ))
                                    .when(show_avg_rule, |this| {
                                        this.child(
                                            div()
                                                .absolute()
                                                .top_2()
                                                .right_2()
                                                .px_2()
                                                .py_1()
                                                .rounded_full()
                                                .border_1()
                                                .border_color(tile_border(cx))
                                                .bg(cx.theme().background.alpha(
                                                    if cx.theme().is_dark() { 0.82 } else { 0.92 },
                                                ))
                                                .text_xs()
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(cx.theme().muted_foreground)
                                                .font_family(cx.theme().mono_font_family.clone())
                                                .child(format!("avg {avg_text}")),
                                        )
                                    }),
                            ),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(cx.theme().muted_foreground)
                    .child(if trend.non_zero_days == 0 {
                        "No traffic recorded in the last 7 days".to_string()
                    } else if trend.non_zero_days == 1 {
                        format!(
                            "Only 1 active day in the last 7 days • peak on {}",
                            trend.peak_label
                        )
                    } else {
                        format!("{} non-zero day(s) in the last 7 days", trend.non_zero_days)
                    }),
            ),
        cx,
    )
}

pub(super) fn traffic_summary_card<T>(
    app_handle: &Entity<WgApp>,
    overview: &OverviewData,
    cx: &mut Context<T>,
) -> GroupBox {
    let summary = &overview.traffic_summary;
    let upload_color = cx.theme().chart_1;
    let download_color = cx.theme().chart_2;
    let total_bytes = summary.total_rx.saturating_add(summary.total_tx);
    let total_text = format_bytes(total_bytes);
    let saved_total = saved_config_total(summary);
    let upload_pct = percent(summary.total_tx, total_bytes);
    let download_pct = percent(summary.total_rx, total_bytes);
    let top_label = summary
        .top_config_name
        .as_deref()
        .map(|name| {
            if summary.active_configs == 1 {
                format!("{name} owns the full share")
            } else if saved_total > 0 {
                format!(
                    "{name} leads at {:.1}%",
                    percent(summary.top_config_total, saved_total)
                )
            } else {
                name.to_string()
            }
        })
        .unwrap_or_else(|| "No active config".to_string());
    let share_note = if summary.active_configs == 0 {
        "No saved config traffic in this period".to_string()
    } else if summary.others_total > 0 {
        format!(
            "Top {} of {} configs • tail in Others",
            summary.ranked.len(),
            summary.active_configs
        )
    } else {
        format!("{} active config(s)", summary.active_configs)
    };

    let period_toggle = ButtonGroup::new("traffic-summary-period")
        .outline()
        .compact()
        .small()
        .child(
            Button::new("traffic-period-today")
                .label("24h")
                .selected(overview.traffic_period == TrafficPeriod::Today)
                .tooltip("Last 24 hours")
                .on_click({
                    let app_handle = app_handle.clone();
                    move |_, _, cx| {
                        app_handle.update(cx, |app, cx| {
                            app.set_session_traffic_period(TrafficPeriod::Today, cx);
                        });
                    }
                }),
        )
        .child(
            Button::new("traffic-period-month")
                .label("30d")
                .selected(overview.traffic_period == TrafficPeriod::ThisMonth)
                .tooltip("Last 30 days")
                .on_click({
                    let app_handle = app_handle.clone();
                    move |_, _, cx| {
                        app_handle.update(cx, |app, cx| {
                            app.set_session_traffic_period(TrafficPeriod::ThisMonth, cx);
                        });
                    }
                }),
        )
        .child(
            Button::new("traffic-period-last")
                .label("Prev 30d")
                .selected(overview.traffic_period == TrafficPeriod::LastMonth)
                .tooltip("Previous 30 days")
                .on_click({
                    let app_handle = app_handle.clone();
                    move |_, _, cx| {
                        app_handle.update(cx, |app, cx| {
                            app.set_session_traffic_period(TrafficPeriod::LastMonth, cx);
                        });
                    }
                }),
        );

    overview_section(
        OverviewSectionTone::Primary,
        section_title(
            IconName::ChartPie,
            "Traffic Summary",
            Some("Config share first, traffic totals second."),
            OverviewSectionTone::Primary,
            cx,
        ),
        v_flex()
            .gap_4()
            .p_3()
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .flex_wrap()
                    .gap_3()
                    .child(period_toggle)
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(cx.theme().muted_foreground)
                            .child(share_note),
                    ),
            )
            .child(
                h_flex()
                    .items_start()
                    .flex_wrap()
                    .gap_3()
                    .child(
                        summary_kpi_tile(
                            IconName::ChartPie,
                            "Total Traffic",
                            total_text,
                            Some("selected period".to_string()),
                            cx.theme().chart_3,
                            true,
                            cx,
                        )
                        .min_w(px(168.0)),
                    )
                    .child(
                        summary_kpi_tile(
                            IconName::ArrowUp,
                            "Upload",
                            format_bytes(summary.total_tx),
                            Some(format!("{upload_pct:.1}% of total")),
                            upload_color,
                            true,
                            cx,
                        )
                        .min_w(px(168.0)),
                    )
                    .child(
                        summary_kpi_tile(
                            IconName::ArrowDown,
                            "Download",
                            format_bytes(summary.total_rx),
                            Some(format!("{download_pct:.1}% of total")),
                            download_color,
                            true,
                            cx,
                        )
                        .min_w(px(168.0)),
                    )
                    .child(
                        summary_kpi_tile(
                            IconName::Settings,
                            "Active Configs",
                            summary.active_configs.to_string(),
                            Some(top_label),
                            cx.theme().chart_4,
                            false,
                            cx,
                        )
                        .min_w(px(168.0)),
                    ),
            )
            .child(direction_split_legend(upload_color, download_color, cx))
            .child(config_share_panel(
                summary,
                saved_total,
                upload_color,
                download_color,
                cx,
            )),
        cx,
    )
}

enum LegendKind {
    Bar,
    Line,
    Dot,
}

fn trend_metric_tile<T>(
    icon: IconName,
    label: &str,
    value: String,
    detail: Option<SharedString>,
    color: Hsla,
    cx: &mut Context<T>,
) -> Div {
    tile_shell(cx)
        .min_w(px(124.0))
        .gap_0p5()
        .p_2()
        .child(tile_header(icon, label, color, None, cx))
        .child(
            div()
                .text_base()
                .font_semibold()
                .text_color(cx.theme().foreground)
                .font_family(cx.theme().mono_font_family.clone())
                .child(value),
        )
        .when_some(detail, |this, detail| {
            this.child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(cx.theme().muted_foreground)
                    .child(detail),
            )
        })
}

fn trend_legend_item<T>(label: &str, color: Hsla, kind: LegendKind, cx: &mut Context<T>) -> Div {
    let marker = match kind {
        LegendKind::Bar => div().w(px(12.0)).h(px(8.0)).rounded_sm().bg(color),
        LegendKind::Line => div().w(px(14.0)).h(px(2.0)).rounded_full().bg(color),
        LegendKind::Dot => div().size(px(8.0)).rounded_full().bg(color),
    };

    h_flex().items_center().gap_2().child(marker).child(
        div()
            .text_xs()
            .font_weight(FontWeight::MEDIUM)
            .text_color(cx.theme().muted_foreground)
            .child(label.to_string()),
    )
}

fn trend_y_axis_rail<T>(trend: &TrafficTrendData, plot_height: Pixels, cx: &mut Context<T>) -> Div {
    let max_text = format_avg_bytes(trend.max_bytes.max(1) as f64);
    let bottom_text = format_avg_bytes(0.0);
    let show_mid = trend.non_zero_days >= 3 && trend.max_bytes > 0;

    v_flex()
        .w(px(52.0))
        .h(plot_height)
        .justify_between()
        .pb(px(gpui_component::plot::AXIS_GAP))
        .font_family(cx.theme().mono_font_family.clone())
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(cx.theme().muted_foreground)
                .child(max_text),
        )
        .child(if show_mid {
            div()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(cx.theme().muted_foreground.opacity(0.7))
                .child(format_avg_bytes((trend.max_bytes as f64) / 2.0))
        } else {
            div().h(px(12.0))
        })
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(cx.theme().muted_foreground)
                .child(bottom_text),
        )
}

fn summary_kpi_tile<T>(
    icon: IconName,
    label: &str,
    value: impl Into<String>,
    detail: Option<String>,
    color: Hsla,
    monospace: bool,
    cx: &mut Context<T>,
) -> Div {
    tile_shell(cx)
        .flex_1()
        .min_w(px(0.0))
        .gap_1()
        .child(tile_header(icon, label, color, None, cx))
        .child(
            div()
                .text_lg()
                .font_semibold()
                .text_color(cx.theme().foreground)
                .when(monospace, |this| {
                    this.font_family(cx.theme().mono_font_family.clone())
                })
                .child(value.into()),
        )
        .when_some(detail, |this, detail| {
            this.child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(cx.theme().muted_foreground)
                    .child(detail),
            )
        })
}

fn direction_split_legend<T>(upload_color: Hsla, download_color: Hsla, cx: &mut Context<T>) -> Div {
    h_flex()
        .items_center()
        .flex_wrap()
        .gap_2()
        .child(trend_legend_item(
            "Upload",
            upload_color,
            LegendKind::Bar,
            cx,
        ))
        .child(trend_legend_item(
            "Download",
            download_color,
            LegendKind::Bar,
            cx,
        ))
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(cx.theme().muted_foreground)
                .child("Bar length = config share"),
        )
}

fn config_share_panel<T>(
    summary: &TrafficSummaryData,
    saved_total: u64,
    upload_color: Hsla,
    download_color: Hsla,
    cx: &mut Context<T>,
) -> Div {
    let rows = config_share_rows(summary, saved_total, cx);

    match config_share_mode(summary, saved_total) {
        ConfigShareMode::Empty => config_share_empty_state(cx),
        ConfigShareMode::Single => v_flex()
            .gap_2()
            .child(config_share_insight(summary, saved_total, cx))
            .child(config_share_list(&rows, upload_color, download_color, cx)),
        ConfigShareMode::Donut => h_flex()
            .items_start()
            .flex_wrap()
            .gap_3()
            .child(config_share_donut(summary, &rows, saved_total, cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(296.0))
                    .gap_2()
                    .child(config_share_insight(summary, saved_total, cx))
                    .child(config_share_list(&rows, upload_color, download_color, cx)),
            ),
        ConfigShareMode::Bars => v_flex()
            .gap_2()
            .child(config_share_insight(summary, saved_total, cx))
            .child(config_share_list(&rows, upload_color, download_color, cx)),
    }
}

fn config_share_empty_state<T>(cx: &mut Context<T>) -> Div {
    div()
        .min_h(px(196.0))
        .w_full()
        .rounded_xl()
        .border_1()
        .border_color(tile_border(cx))
        .bg(tile_surface(cx))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_2()
        .child(
            div()
                .size(px(40.0))
                .rounded_full()
                .bg(cx
                    .theme()
                    .secondary
                    .alpha(if cx.theme().is_dark() { 0.34 } else { 0.5 }))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Icon::new(IconName::ChartPie)
                        .size_4()
                        .text_color(cx.theme().chart_3),
                ),
        )
        .child(
            div()
                .text_sm()
                .font_semibold()
                .text_color(cx.theme().foreground)
                .child("No saved config traffic in this period"),
        )
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(cx.theme().muted_foreground)
                .child("Switch the period or wait for a config to record traffic."),
        )
}

fn config_share_insight<T>(
    summary: &TrafficSummaryData,
    saved_total: u64,
    cx: &mut Context<T>,
) -> Div {
    let lead_name = summary
        .top_config_name
        .as_deref()
        .unwrap_or("No active config");
    let lead_share = if summary.active_configs <= 1 {
        "100% share".to_string()
    } else {
        format!(
            "{:.1}% share",
            percent(summary.top_config_total, saved_total)
        )
    };
    let shell_bg = cx
        .theme()
        .secondary
        .alpha(if cx.theme().is_dark() { 0.2 } else { 0.34 });
    let shell_border = cx
        .theme()
        .border
        .alpha(if cx.theme().is_dark() { 0.38 } else { 0.3 });

    v_flex()
        .gap_2()
        .p_3()
        .rounded_xl()
        .border_1()
        .border_color(shell_border)
        .bg(shell_bg)
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(cx.theme().muted_foreground)
                .child("Lead Config"),
        )
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .flex_wrap()
                .gap_2()
                .child(
                    div()
                        .text_base()
                        .font_semibold()
                        .text_color(cx.theme().foreground)
                        .child(lead_name.to_string()),
                )
                .child(summary_stat_chip(lead_share, cx.theme().chart_3, cx)),
        )
        .child(
            h_flex()
                .items_center()
                .flex_wrap()
                .gap_2()
                .child(summary_stat_chip(
                    format_bytes(summary.top_config_total),
                    cx.theme().foreground,
                    cx,
                ))
                .child(summary_stat_chip(
                    format!("{} active", summary.active_configs),
                    cx.theme().muted_foreground,
                    cx,
                ))
                .when(summary.others_total > 0, |this| {
                    this.child(summary_stat_chip(
                        format!("{} in Others", format_bytes(summary.others_total)),
                        cx.theme().muted_foreground,
                        cx,
                    ))
                })
                .when(summary.others_total == 0 && saved_total > 0, |this| {
                    this.child(summary_stat_chip(
                        format!("{} total", format_bytes(saved_total)),
                        cx.theme().muted_foreground,
                        cx,
                    ))
                }),
        )
}

fn summary_stat_chip<T>(label: impl Into<SharedString>, color: Hsla, cx: &mut Context<T>) -> Div {
    let label: SharedString = label.into();
    div()
        .px_2()
        .py_1()
        .rounded_full()
        .bg(color.alpha(if cx.theme().is_dark() { 0.14 } else { 0.1 }))
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(color)
        .child(label)
}

fn config_share_donut<T>(
    summary: &TrafficSummaryData,
    rows: &[ConfigShareRow],
    saved_total: u64,
    cx: &mut Context<T>,
) -> Div {
    div().min_w(px(248.0)).flex_1().child(
        div()
            .p_4()
            .rounded_xl()
            .bg(cx
                .theme()
                .secondary
                .alpha(if cx.theme().is_dark() { 0.12 } else { 0.24 }))
            .flex()
            .justify_center()
            .child(
                div()
                    .size(px(196.0))
                    .relative()
                    .child(
                        PieChart::new(rows.to_vec())
                            .value(|row| row.total as f32)
                            .inner_radius(66.0)
                            .outer_radius(88.0)
                            .pad_angle(0.022)
                            .color(|row| row.color)
                            .into_any_element(),
                    )
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(cx.theme().muted_foreground.opacity(0.72))
                                    .child("SHARE"),
                            )
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(cx.theme().foreground)
                                    .font_family(cx.theme().mono_font_family.clone())
                                    .child(format_bytes(saved_total)),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(cx.theme().muted_foreground.opacity(0.82))
                                    .child(if summary.active_configs == 1 {
                                        "1 config".to_string()
                                    } else {
                                        format!("{} configs", summary.active_configs)
                                    }),
                            ),
                    ),
            ),
    )
}

fn config_share_list<T>(
    rows: &[ConfigShareRow],
    upload_color: Hsla,
    download_color: Hsla,
    cx: &mut Context<T>,
) -> Div {
    if rows.is_empty() {
        return config_share_empty_state(cx);
    }

    let row_border = cx
        .theme()
        .border
        .alpha(if cx.theme().is_dark() { 0.34 } else { 0.26 });
    let row_surface = cx
        .theme()
        .background
        .alpha(if cx.theme().is_dark() { 0.34 } else { 0.64 });
    let list_border = tile_border(cx);
    let list_surface = tile_surface(cx);
    let track = cx
        .theme()
        .secondary
        .alpha(if cx.theme().is_dark() { 0.46 } else { 0.58 });

    let rows = rows.iter().map(|row| {
        let split_total = row.rx_bytes.saturating_add(row.tx_bytes);
        let upload_ratio = if split_total == 0 {
            0.0
        } else {
            row.tx_bytes as f32 / split_total as f32
        };

        v_flex()
            .gap_1()
            .px_3()
            .py_1()
            .rounded_lg()
            .border_1()
            .border_color(row_border)
            .bg(row_surface)
            .child(
                h_flex()
                    .items_start()
                    .justify_between()
                    .gap_1p5()
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(div().size(px(8.0)).rounded_full().bg(row.color))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(cx.theme().foreground)
                                    .child(row.name.clone()),
                            ),
                    )
                    .child(
                        h_flex()
                            .items_baseline()
                            .gap_1p5()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(cx.theme().foreground)
                                    .font_family(cx.theme().mono_font_family.clone())
                                    .child(format_bytes(row.total)),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("{:.1}%", row.share_pct)),
                            ),
                    ),
            )
            .child(
                div()
                    .h(px(6.0))
                    .w_full()
                    .rounded_full()
                    .overflow_hidden()
                    .bg(track)
                    .child(if row.is_other {
                        div()
                            .h_full()
                            .w(relative(row.share_pct / 100.0))
                            .bg(row.color)
                    } else {
                        h_flex()
                            .h_full()
                            .w(relative(row.share_pct / 100.0))
                            .child(div().h_full().w(relative(upload_ratio)).bg(upload_color))
                            .child(div().h_full().flex_grow().bg(download_color))
                    }),
            )
            .child(if row.is_other {
                div()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(cx.theme().muted_foreground)
                    .child("Tail configs collapsed")
                    .into_any_element()
            } else {
                h_flex()
                    .items_center()
                    .flex_wrap()
                    .gap_1p5()
                    .child(direction_value("Up", row.tx_bytes, upload_color, cx))
                    .child(direction_value("Down", row.rx_bytes, download_color, cx))
                    .into_any_element()
            })
    });

    v_flex()
        .gap_1p5()
        .p_2()
        .bg(list_surface)
        .rounded_xl()
        .border_1()
        .border_color(list_border)
        .children(rows)
}

fn direction_value<T>(label: &str, value: u64, color: Hsla, cx: &mut Context<T>) -> Div {
    h_flex()
        .items_center()
        .gap_1p5()
        .child(div().size(px(7.0)).rounded_full().bg(color))
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(cx.theme().muted_foreground)
                .child(label.to_string()),
        )
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(cx.theme().foreground)
                .font_family(cx.theme().mono_font_family.clone())
                .child(format_bytes(value)),
        )
}

fn config_share_mode(summary: &TrafficSummaryData, saved_total: u64) -> ConfigShareMode {
    if summary.active_configs == 0 || saved_total == 0 {
        ConfigShareMode::Empty
    } else if summary.active_configs == 1 && summary.others_total == 0 {
        ConfigShareMode::Single
    } else if summary.active_configs <= 5 {
        ConfigShareMode::Donut
    } else {
        ConfigShareMode::Bars
    }
}

fn config_share_rows<T>(
    summary: &TrafficSummaryData,
    saved_total: u64,
    cx: &mut Context<T>,
) -> Vec<ConfigShareRow> {
    let palette = config_share_palette(cx);
    let mut rows = summary
        .ranked
        .iter()
        .enumerate()
        .map(|(index, item)| ConfigShareRow {
            name: item.name.clone(),
            total: item.total_bytes(),
            rx_bytes: item.rx_bytes,
            tx_bytes: item.tx_bytes,
            share_pct: percent(item.total_bytes(), saved_total),
            color: palette[index % palette.len()],
            is_other: false,
        })
        .collect::<Vec<_>>();

    if summary.others_total > 0 {
        rows.push(ConfigShareRow {
            name: format!(
                "Others ({})",
                summary.active_configs.saturating_sub(summary.ranked.len())
            ),
            total: summary.others_total,
            rx_bytes: 0,
            tx_bytes: 0,
            share_pct: percent(summary.others_total, saved_total),
            color: cx
                .theme()
                .muted_foreground
                .alpha(if cx.theme().is_dark() { 0.3 } else { 0.22 }),
            is_other: true,
        });
    }

    rows
}

fn config_share_palette<T>(cx: &mut Context<T>) -> [Hsla; 7] {
    [
        cx.theme().chart_3,
        cx.theme().chart_4,
        cx.theme().chart_5,
        cx.theme().chart_3.opacity(0.8),
        cx.theme().chart_4.opacity(0.8),
        cx.theme().chart_5.opacity(0.8),
        cx.theme()
            .muted_foreground
            .alpha(if cx.theme().is_dark() { 0.68 } else { 0.52 }),
    ]
}

fn saved_config_total(summary: &TrafficSummaryData) -> u64 {
    summary
        .ranked
        .iter()
        .fold(summary.others_total, |acc, item| {
            acc.saturating_add(item.total_bytes())
        })
}

fn percent(value: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        (value as f64 / total as f64 * 100.0) as f32
    }
}

pub(super) struct TrafficColumnProps<'a> {
    pub(super) icon: IconName,
    pub(super) label: &'a str,
    pub(super) footer_label: &'a str,
    pub(super) speed: &'a str,
    pub(super) total: &'a str,
    pub(super) color: Hsla,
    pub(super) sparkline: AnyElement,
}

pub(super) fn traffic_column<T>(props: TrafficColumnProps<'_>, cx: &mut Context<T>) -> Div {
    let icon_small = props.icon.clone();
    v_flex()
        .gap_2()
        .flex_grow()
        .p_3()
        .rounded_lg()
        .border_1()
        .border_color(tile_border(cx))
        .bg(tile_surface(cx))
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(tile_icon(props.icon, props.color, cx))
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(cx.theme().muted_foreground)
                                .child(props.label.to_string()),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(cx.theme().muted_foreground)
                        .child(props.footer_label.to_string()),
                ),
        )
        .child(
            div()
                .text_2xl()
                .font_semibold()
                .text_color(cx.theme().foreground)
                .font_family(cx.theme().mono_font_family.clone())
                .child(props.speed.to_string()),
        )
        .child(
            div()
                .h(px(140.0))
                .w_full()
                .px_2()
                .py_3()
                .rounded_lg()
                .bg(cx
                    .theme()
                    .secondary
                    .alpha(if cx.theme().is_dark() { 0.28 } else { 0.38 }))
                .child(props.sparkline),
        )
        .child(
            h_flex()
                .items_center()
                .gap_2()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(Icon::new(icon_small).size_3().text_color(props.color))
                .child(
                    div()
                        .font_family(cx.theme().mono_font_family.clone())
                        .child(format!("{} {}", props.footer_label, props.total)),
                ),
        )
}

#[derive(Clone, Copy)]
enum ConfigShareMode {
    Empty,
    Single,
    Donut,
    Bars,
}

#[derive(Clone)]
struct ConfigShareRow {
    name: String,
    total: u64,
    rx_bytes: u64,
    tx_bytes: u64,
    share_pct: f32,
    color: Hsla,
    is_other: bool,
}
