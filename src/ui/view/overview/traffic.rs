use gpui::*;
use gpui_component::{
    button::{Button, ButtonGroup, ButtonVariants},
    chart::{BarChart, PieChart},
    divider::Divider,
    group_box::{GroupBox, GroupBoxVariants},
    h_flex, v_flex, ActiveTheme as _, Disableable as _, Icon, IconName, Selectable as _,
    Sizable as _, StyledExt as _,
};

use crate::ui::format::format_bytes;
use crate::ui::state::{TrafficPeriod, WgApp};
use crate::ui::view::data::{OverviewData, TrafficRankItem, TrafficTrendData};

use super::chart::{format_avg_bytes, TrafficAvgLine};
use super::common::{card_title, vertical_rule};

pub(super) fn traffic_trend_card(trend: &TrafficTrendData, cx: &mut Context<WgApp>) -> GroupBox {
    let avg_color = cx.theme().chart_4;
    let avg_line_color = avg_color.alpha(if cx.theme().is_dark() { 0.55 } else { 0.45 });
    let bar_color =
        cx.theme()
            .muted_foreground
            .alpha(if cx.theme().is_dark() { 0.16 } else { 0.12 });
    let bar_highlight = cx
        .theme()
        .chart_3
        .alpha(if cx.theme().is_dark() { 0.32 } else { 0.24 });
    let avg_text = format_avg_bytes(trend.average_bytes);

    GroupBox::new()
        .fill()
        .title(card_title(
            IconName::Calendar,
            "7-Day Traffic Trend",
            Some(IconName::Redo),
            cx,
        ))
        .child(Divider::horizontal().color(cx.theme().border))
        .child(
            v_flex()
                .gap_3()
                .child(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child("Daily Avg"),
                        )
                        .child(
                            div()
                                .text_3xl()
                                .font_semibold()
                                .text_color(avg_color)
                                .child(avg_text),
                        ),
                )
                .child(
                    div()
                        .h(px(140.0))
                        .w_full()
                        .relative()
                        .child(
                            BarChart::new(trend.points.clone())
                                .x(|point| point.label.clone())
                                .y(|point| point.bytes as f64)
                                .fill(move |point| {
                                    if point.is_today {
                                        bar_highlight
                                    } else {
                                        bar_color
                                    }
                                }),
                        )
                        .child(div().absolute().inset_0().child(TrafficAvgLine::new(
                            trend.points.clone(),
                            trend.average_bytes,
                            avg_line_color,
                        ))),
                ),
        )
}

pub(super) fn traffic_summary_card(overview: &OverviewData, cx: &mut Context<WgApp>) -> GroupBox {
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
                .label("Today")
                .selected(overview.traffic_period == TrafficPeriod::Today)
                .tooltip("Last 24 hours")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.set_session_traffic_period(TrafficPeriod::Today, cx);
                })),
        )
        .child(
            Button::new("traffic-period-month")
                .label("This Month")
                .selected(overview.traffic_period == TrafficPeriod::ThisMonth)
                .tooltip("Last 30 days")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.set_session_traffic_period(TrafficPeriod::ThisMonth, cx);
                })),
        )
        .child(
            Button::new("traffic-period-last")
                .label("Last Month")
                .selected(overview.traffic_period == TrafficPeriod::LastMonth)
                .tooltip("Previous 30 days")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.set_session_traffic_period(TrafficPeriod::LastMonth, cx);
                })),
        );

    let ranking_tabs = h_flex()
        .gap_2()
        .items_center()
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(cx.theme().muted_foreground)
                .child("RANKING BY:"),
        )
        .child(
            ButtonGroup::new("traffic-summary-ranking")
                .ghost()
                .compact()
                .xsmall()
                .child(
                    Button::new("traffic-ranking-proxy")
                        .label("Proxy")
                        .selected(true),
                )
                .child(
                    Button::new("traffic-ranking-process")
                        .label("Process")
                        .disabled(true),
                )
                .child(
                    Button::new("traffic-ranking-interface")
                        .label("Interface")
                        .disabled(true),
                )
                .child(
                    Button::new("traffic-ranking-host")
                        .label("Hostname")
                        .disabled(true),
                ),
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
                        .font_weight(FontWeight::BOLD)
                        .text_color(cx.theme().muted_foreground.opacity(0.7))
                        .child("TOTAL TRAFFIC"),
                )
                .child(
                    div()
                        .text_3xl()
                        .font_weight(FontWeight::BOLD)
                        .text_color(cx.theme().foreground)
                        .child(total_text),
                ),
        );

    let breakdown = v_flex()
        .gap_6()
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

    GroupBox::new()
        .fill()
        .title(card_title(IconName::ChartPie, "Traffic Summary", None, cx))
        .child(Divider::horizontal().color(cx.theme().border))
        .child(
            v_flex()
                .gap_6()
                .p_4()
                .child(
                    h_flex()
                        .items_center()
                        .justify_between()
                        .flex_wrap()
                        .gap_4()
                        .child(period_toggle)
                        .child(ranking_tabs),
                )
                .child(
                    h_flex()
                        .items_start()
                        .gap_8()
                        .child(
                            v_flex()
                                .w(relative(0.4))
                                .min_w(px(300.0))
                                .items_center()
                                .gap_8()
                                .child(donut)
                                .child(breakdown),
                        )
                        .child(vertical_rule(cx))
                        .child(v_flex().flex_grow().w(relative(0.6)).gap_4().child(ranking)),
                ),
        )
}

fn metric_progress_modern(
    icon: IconName,
    label: &str,
    value: u64,
    total: u64,
    color: Hsla,
    cx: &mut Context<WgApp>,
) -> Div {
    let pct = percent(value, total);
    let value_text = format_bytes(value);

    v_flex()
        .gap_1()
        .w_full()
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(Icon::new(icon).size(px(14.0)).text_color(color))
                        .child(
                            div()
                                .text_sm()
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
                                .font_weight(FontWeight::BOLD)
                                .text_color(cx.theme().foreground)
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
            div()
                .h(px(6.0))
                .w_full()
                .bg(cx.theme().secondary)
                .rounded_full()
                .child(
                    div()
                        .h_full()
                        .w(relative(pct / 100.0))
                        .bg(color)
                        .rounded_full(),
                ),
        )
}

fn traffic_ranking_list_modern(
    ranked: &[TrafficRankItem],
    color: Hsla,
    cx: &mut Context<WgApp>,
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

    let rows = ranked.iter().enumerate().map(|(i, item)| {
        let total = item.total_bytes();
        let pct = percent(total, max_total);
        let rank_num = i + 1;

        h_flex()
            .items_center()
            .gap_3()
            .py_1()
            .child(
                div()
                    .w(px(20.0))
                    .text_xs()
                    .font_weight(FontWeight::BOLD)
                    .text_color(cx.theme().muted_foreground.opacity(0.5))
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
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(cx.theme().foreground)
                                    .child(item.name.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format_bytes(total)),
                            ),
                    )
                    .child(
                        div()
                            .h(px(4.0))
                            .w_full()
                            .bg(cx.theme().secondary)
                            .rounded_full()
                            .child(
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
        .p_2()
        .bg(cx.theme().secondary.opacity(0.3))
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border.opacity(0.5))
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

pub(super) fn traffic_column(props: TrafficColumnProps<'_>, cx: &mut Context<WgApp>) -> Div {
    let icon_small = props.icon.clone();
    v_flex()
        .gap_2()
        .flex_grow()
        .child(
            h_flex()
                .items_center()
                .gap_2()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(Icon::new(props.icon).size_4().text_color(props.color))
                .child(props.label.to_string()),
        )
        .child(
            div()
                .text_3xl()
                .font_semibold()
                .text_color(props.color)
                .child(props.speed.to_string()),
        )
        .child(div().h(px(140.0)).w_full().child(props.sparkline))
        .child(
            h_flex()
                .items_center()
                .gap_2()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(Icon::new(icon_small).size_3().text_color(props.color))
                .child(format!("{} {}", props.footer_label, props.total)),
        )
}

struct TrafficSlice {
    value: u64,
    color: Hsla,
}
