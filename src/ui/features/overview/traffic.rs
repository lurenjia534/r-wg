use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonGroup},
    h_flex, v_flex, ActiveTheme as _, Icon, IconName, Selectable as _, Sizable as _,
    StyledExt as _,
};

use crate::ui::format::format_bytes;
use crate::ui::state::{TrafficPeriod, WgApp};

use super::common::{
    overview_section, section_title, tile_border, tile_header, tile_icon, tile_shell, tile_surface,
    OverviewSectionTone,
};
use super::traffic_share::{config_share_panel, percent, saved_config_total};
use super::view_model::OverviewData;

pub(crate) fn traffic_summary_card<T>(
    app_handle: &Entity<WgApp>,
    overview: &OverviewData,
    cx: &mut Context<T>,
) -> Div {
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
        .child(summary_bar_legend_item("Upload", upload_color, cx))
        .child(summary_bar_legend_item("Download", download_color, cx))
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(cx.theme().muted_foreground)
                .child("Bar length = config share"),
        )
}

fn summary_bar_legend_item<T>(label: &str, color: Hsla, cx: &mut Context<T>) -> Div {
    h_flex()
        .items_center()
        .gap_2()
        .child(div().w(px(12.0)).h(px(8.0)).rounded_sm().bg(color))
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(cx.theme().muted_foreground)
                .child(label.to_string()),
        )
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
