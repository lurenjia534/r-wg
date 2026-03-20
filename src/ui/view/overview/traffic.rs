use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonGroup},
    chart::{BarChart, PieChart},
    group_box::GroupBox,
    h_flex,
    v_flex, ActiveTheme as _, Icon, IconName, Selectable as _, Sizable as _, StyledExt as _,
};

use crate::ui::format::format_bytes;
use crate::ui::state::{TrafficPeriod, WgApp};
use crate::ui::view::data::{OverviewData, TrafficRankItem, TrafficTrendData};

use super::chart::{format_avg_bytes, TrafficTrendOverlay};
use super::common::{
    overview_section, section_title, tile_border, tile_header, tile_icon, tile_shell,
    tile_surface, vertical_rule, OverviewSectionTone,
};

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
    let bar_base =
        cx.theme()
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
    let avg_rule = cx.theme().chart_4.alpha(if cx.theme().is_dark() { 0.68 } else { 0.54 });
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
                    .child(trend_legend_item("Daily total", bar_base, LegendKind::Bar, cx))
                    .when(show_avg_rule, |this| {
                        this.child(trend_legend_item("7d avg", avg_rule, LegendKind::Line, cx))
                    })
                    .when(trend.points.iter().any(|point| point.is_today && point.bytes > 0), |this| {
                        this.child(trend_legend_item("Today", cx.theme().chart_3, LegendKind::Dot, cx))
                    })
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
                                                } else if label_peak > 0 && point.bytes == label_peak {
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
                                    .child(
                                        div().absolute().inset_0().child(TrafficTrendOverlay::new(
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
                                        )),
                                    )
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
                                                .bg(cx.theme().background.alpha(if cx.theme().is_dark() {
                                                    0.82
                                                } else {
                                                    0.92
                                                }))
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
                        format!("Only 1 active day in the last 7 days • peak on {}", trend.peak_label)
                    } else {
                        format!(
                            "{} non-zero day(s) in the last 7 days",
                            trend.non_zero_days
                        )
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
    let rank_color = cx.theme().chart_3;

    let total_bytes = summary.total_rx.saturating_add(summary.total_tx);
    let total_text = format_bytes(total_bytes);

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
                        let _ = app_handle.update(cx, |app, cx| {
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
                        let _ = app_handle.update(cx, |app, cx| {
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
                        let _ = app_handle.update(cx, |app, cx| {
                            app.set_session_traffic_period(TrafficPeriod::LastMonth, cx);
                        });
                    }
                }),
        );

    let pie_data = vec![
        TrafficSlice {
            value: summary.total_rx,
            color: download_color,
        },
        TrafficSlice {
            value: summary.total_tx,
            color: upload_color,
        },
    ];

    let donut = div()
        .size(px(180.0))
        .relative()
        .child(
            PieChart::new(pie_data)
                .value(|slice| slice.value as f32)
                .inner_radius(65.0)
                .outer_radius(80.0)
                .pad_angle(0.02)
                .color(|slice| slice.color)
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
                        .text_color(cx.theme().muted_foreground.opacity(0.7))
                        .child("TOTAL TRAFFIC"),
                )
                .child(
                    div()
                        .text_2xl()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(cx.theme().foreground)
                        .font_family(cx.theme().mono_font_family.clone())
                        .child(total_text),
                ),
        );

    let breakdown = v_flex()
        .gap_4()
        .w_full()
        .child(metric_progress_modern(
            IconName::ArrowUp,
            "Upload",
            summary.total_tx,
            total_bytes,
            upload_color,
            cx,
        ))
        .child(metric_progress_modern(
            IconName::ArrowDown,
            "Download",
            summary.total_rx,
            total_bytes,
            download_color,
            cx,
        ));

    let ranking = traffic_ranking_list_modern(&summary.ranked, rank_color, cx);

    overview_section(
        OverviewSectionTone::Primary,
        section_title(
            IconName::ChartPie,
            "Traffic Summary",
            Some("Saved config distribution and traffic totals."),
            OverviewSectionTone::Primary,
            cx,
        ),
        v_flex()
            .gap_4()
            .p_3()
            .child(
                h_flex()
                    .items_center()
                    .flex_wrap()
                    .gap_3()
                    .child(period_toggle)
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(cx.theme().muted_foreground)
                            .child("Ranking by saved config"),
                    ),
            )
            .child(
                h_flex()
                    .items_start()
                    .flex_wrap()
                    .gap_6()
                    .child(
                        v_flex()
                            .w(relative(0.4))
                            .min_w(px(300.0))
                            .items_center()
                            .gap_4()
                            .child(
                                div()
                                    .p_3()
                                    .rounded_lg()
                                    .border_1()
                                    .border_color(tile_border(cx))
                                    .bg(tile_surface(cx))
                                    .child(donut),
                            )
                            .child(breakdown),
                    )
                    .child(vertical_rule(cx).h(px(168.0)))
                    .child(v_flex().flex_grow().w(relative(0.6)).gap_4().child(ranking)),
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

fn trend_legend_item<T>(
    label: &str,
    color: Hsla,
    kind: LegendKind,
    cx: &mut Context<T>,
) -> Div {
    let marker = match kind {
        LegendKind::Bar => div()
            .w(px(12.0))
            .h(px(8.0))
            .rounded_sm()
            .bg(color),
        LegendKind::Line => div()
            .w(px(14.0))
            .h(px(2.0))
            .rounded_full()
            .bg(color),
        LegendKind::Dot => div()
            .size(px(8.0))
            .rounded_full()
            .bg(color),
    };

    h_flex()
        .items_center()
        .gap_2()
        .child(marker)
        .child(
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

fn metric_progress_modern<T>(
    icon: IconName,
    label: &str,
    value: u64,
    total: u64,
    color: Hsla,
    cx: &mut Context<T>,
) -> Div {
    let pct = percent(value, total);
    let value_text = format_bytes(value);
    let track = cx
        .theme()
        .secondary
        .alpha(if cx.theme().is_dark() { 0.7 } else { 0.9 });

    v_flex()
        .gap_1()
        .w_full()
        .p_3()
        .rounded_lg()
        .border_1()
        .border_color(tile_border(cx))
        .bg(tile_surface(cx))
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(tile_icon(icon, color, cx))
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(cx.theme().muted_foreground)
                                .child(label.to_string()),
                        ),
                )
                .child(
                    h_flex()
                        .items_baseline()
                        .gap_1()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(cx.theme().foreground)
                                .font_family(cx.theme().mono_font_family.clone())
                                .child(value_text),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground.opacity(0.7))
                                .child(format!("({pct:.1}%)")),
                        ),
                ),
        )
        .child(
            div().h(px(5.0)).w_full().bg(track).rounded_full().child(
                div()
                    .h_full()
                    .w(relative(pct / 100.0))
                    .bg(color)
                    .rounded_full(),
            ),
        )
}

fn traffic_ranking_list_modern<T>(
    ranked: &[TrafficRankItem],
    color: Hsla,
    cx: &mut Context<T>,
) -> Div {
    if ranked.is_empty() {
        return div()
            .flex()
            .items_center()
            .justify_center()
            .h(px(100.0))
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child("No traffic data available");
    }

    let max_total = ranked
        .iter()
        .map(|item| item.total_bytes())
        .max()
        .unwrap_or(0);
    let row_border = tile_border(cx);
    let row_surface = tile_surface(cx);
    let list_border = tile_border(cx);
    let list_surface = tile_surface(cx);

    let rows = ranked.iter().enumerate().map(|(i, item)| {
        let total = item.total_bytes();
        let pct = percent(total, max_total);
        let rank_num = i + 1;
        let track = cx
            .theme()
            .secondary
            .alpha(if cx.theme().is_dark() { 0.65 } else { 0.85 });

        h_flex()
            .items_center()
            .gap_2()
            .p_3()
            .rounded_lg()
            .border_1()
            .border_color(row_border)
            .bg(row_surface)
            .child(
                div()
                    .w(px(28.0))
                    .h(px(28.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(color.alpha(if cx.theme().is_dark() { 0.18 } else { 0.12 }))
                    .text_xs()
                    .font_weight(FontWeight::BOLD)
                    .text_color(color)
                    .child(rank_num.to_string()),
            )
            .child(
                v_flex()
                    .flex_grow()
                    .gap_1()
                    .child(
                        h_flex()
                            .justify_between()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(cx.theme().foreground)
                                    .child(item.name.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .font_family(cx.theme().mono_font_family.clone())
                                    .child(format_bytes(total)),
                            ),
                    )
                    .child(
                        div().h(px(4.0)).w_full().bg(track).rounded_full().child(
                            div()
                                .h_full()
                                .w(relative(pct / 100.0))
                                .bg(color.opacity(0.8))
                                .rounded_full(),
                        ),
                    ),
            )
    });

    v_flex()
        .gap_2()
        .p_3()
        .bg(list_surface)
        .rounded_xl()
        .border_1()
        .border_color(list_border)
        .children(rows)
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
                .bg(cx.theme().secondary.alpha(if cx.theme().is_dark() { 0.28 } else { 0.38 }))
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

struct TrafficSlice {
    value: u64,
    color: Hsla,
}
