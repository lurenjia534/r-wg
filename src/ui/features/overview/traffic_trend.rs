use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{chart::BarChart, h_flex, v_flex, ActiveTheme as _, IconName, StyledExt as _};

use crate::ui::format::format_bytes;

use super::chart::{format_avg_bytes, TrafficTrendOverlay};
use super::common::{
    overview_section, section_title, tile_border, tile_header, tile_shell, tile_surface,
    OverviewSectionTone,
};
use super::traffic_analytics::TrafficTrendData;

pub(crate) fn traffic_trend_card<T>(trend: &TrafficTrendData, cx: &mut Context<T>) -> Div {
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
