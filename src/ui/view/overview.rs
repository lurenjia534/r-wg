use std::collections::HashMap;

use chrono::{Duration as ChronoDuration, Local, NaiveDate};
use gpui::prelude::FluentBuilder as _;
use gpui::*;

use gpui_component::{
    chart::{BarChart, LineChart},
    divider::Divider,
    group_box::{GroupBox, GroupBoxVariants},
    h_flex,
    plot::{
        scale::{Scale, ScaleLinear},
        shape::Line,
        StrokeStyle, AXIS_GAP,
    },
    tag::Tag,
    v_flex, ActiveTheme as _, Icon, IconName, PixelsExt, Sizable as _, StyledExt as _,
};

use super::super::state::{WgApp, TRAFFIC_TREND_DAYS};
use super::data::ViewData;

/// Overview 页：两张状态卡片（运行状态 / 网络状态）。
pub(crate) fn render_overview(app: &mut WgApp, data: &ViewData, cx: &mut Context<WgApp>) -> Div {
    let uptime = format_uptime(app);
    let memory = format_memory_usage();
    let rx = super::super::format::format_bytes(data.peer_summary.rx_bytes);
    let tx = super::super::format::format_bytes(data.peer_summary.tx_bytes);
    let peers = data.peer_summary.peer_count.to_string();
    let handshake = data.last_handshake.clone();
    let (upload_speed, download_speed) = format_speeds(app, data);
    let upload_total = super::super::format::format_bytes(data.peer_summary.tx_bytes);
    let download_total = super::super::format::format_bytes(data.peer_summary.rx_bytes);
    let upload_series: Vec<f32> = app.tx_rate_history.iter().copied().collect();
    let download_series: Vec<f32> = app.rx_rate_history.iter().copied().collect();
    let upload_sparkline = sparkline_chart(build_sparkline_points(&upload_series), rgb(0x6366f1));
    let download_sparkline =
        sparkline_chart(build_sparkline_points(&download_series), rgb(0x22d3ee));

    let local_ip = format_local_ip(data);
    let dns = format_dns(data);
    let endpoint = format_endpoint(data);
    let allowed = format_allowed_summary(data);
    let network_name = app.running_name.clone().unwrap_or_else(|| "-".to_string());
    let route_table = data
        .parsed_config
        .as_ref()
        .map(|cfg| super::super::format::format_route_table(cfg.interface.table))
        .unwrap_or_else(|| "-".to_string());
    let traffic_trend = build_traffic_trend(app);

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
                        &memory,
                        &rx,
                        &tx,
                        app.running,
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
        .child(
            h_flex()
                .w_full()
                .gap_3()
                .items_start()
                .child(
                    traffic_stats_card(
                        cx,
                        &upload_speed,
                        &download_speed,
                        &upload_total,
                        &download_total,
                        upload_sparkline,
                        download_sparkline,
                    )
                    .w(relative(0.5)),
                )
                .child(traffic_trend_card(cx, &traffic_trend).w(relative(0.5))),
        )
}

/// 其它菜单项的占位页。
pub(crate) fn render_placeholder(cx: &mut Context<WgApp>) -> Div {
    div().child(
        GroupBox::new().fill().title("Coming Soon").child(
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
    memory: &str,
    rx: &str,
    tx: &str,
    is_running: bool,
    peers: &str,
    handshake: &str,
) -> GroupBox {
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
                        metric_cell(IconName::LoaderCircle, "Uptime", uptime, rgb(0x3a8bd6), cx),
                        metric_cell(IconName::ArrowDown, "RX", rx, rgb(0xf59e0b), cx),
                        metric_cell(IconName::ArrowUp, "TX", tx, rgb(0x2dd4bf), cx),
                    ],
                    [
                        status_state_item(is_running, cx),
                        status_item(IconName::CircleUser, "Peers", peers, rgb(0x60a5fa), cx),
                        status_item(
                            IconName::ExternalLink,
                            "Handshake",
                            handshake,
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
                        memory,
                        rgb(0x22d3ee),
                        cx,
                    )
                    .w_full()
                    .border_t_1()
                    .border_color(border),
                ),
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
        .child(two_row_grid(
            [
                metric_cell(IconName::ArrowUp, "Local IP", local_ip, rgb(0x22c55e), cx),
                metric_cell(IconName::Search, "DNS", dns, rgb(0x22c55e), cx),
                metric_cell(IconName::Globe, "Endpoint", endpoint, rgb(0x22c55e), cx),
            ],
            [
                status_item(IconName::Globe, "Network", network_name, rgb(0x38bdf8), cx),
                status_item(IconName::Map, "Route", route_table, rgb(0x60a5fa), cx),
                status_item(
                    IconName::SortAscending,
                    "Allowed IPs",
                    allowed,
                    rgb(0x22c55e),
                    cx,
                ),
            ],
            cx,
        ))
}

fn traffic_stats_card(
    cx: &mut Context<WgApp>,
    upload_speed: &str,
    download_speed: &str,
    upload_total: &str,
    download_total: &str,
    upload_sparkline: AnyElement,
    download_sparkline: AnyElement,
) -> GroupBox {
    GroupBox::new()
        .fill()
        .title(card_title(IconName::ChartPie, "Traffic Stats", None, cx))
        .child(Divider::horizontal().color(cx.theme().border))
        .child(
            h_flex()
                .gap_6()
                .items_start()
                .child(traffic_column(
                    IconName::ArrowUp,
                    "Upload Speed",
                    "Upload",
                    upload_speed,
                    upload_total,
                    rgb(0x6366f1),
                    upload_sparkline,
                    cx,
                ))
                .child(vertical_rule(cx).h(px(160.0)))
                .child(traffic_column(
                    IconName::ArrowDown,
                    "Download Speed",
                    "Download",
                    download_speed,
                    download_total,
                    rgb(0x22d3ee),
                    download_sparkline,
                    cx,
                )),
        )
}

#[derive(Clone)]
struct TrafficTrendPoint {
    label: String,
    bytes: u64,
    is_today: bool,
}

struct TrafficTrendData {
    points: Vec<TrafficTrendPoint>,
    average_bytes: f64,
}

fn traffic_trend_card(cx: &mut Context<WgApp>, trend: &TrafficTrendData) -> GroupBox {
    let avg_color: Hsla = rgb(0xf59e0b).into();
    let bar_color = cx.theme().muted_foreground.alpha(0.25);
    let bar_highlight = cx.theme().muted_foreground.alpha(0.5);
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
                            avg_color,
                        ))),
                ),
        )
}

fn build_traffic_trend(app: &WgApp) -> TrafficTrendData {
    let mut by_date: HashMap<NaiveDate, u64> = HashMap::new();
    for day in &app.traffic_days {
        if let Ok(date) = NaiveDate::parse_from_str(&day.date, "%Y-%m-%d") {
            let entry = by_date.entry(date).or_insert(0);
            *entry = entry.saturating_add(day.bytes);
        }
    }

    let today = Local::now().date_naive();
    let mut points = Vec::with_capacity(TRAFFIC_TREND_DAYS);
    for offset in (0..TRAFFIC_TREND_DAYS).rev() {
        let date = today - ChronoDuration::days(offset as i64);
        let bytes = by_date.get(&date).copied().unwrap_or(0);
        let label = date.format("%a").to_string();
        points.push(TrafficTrendPoint {
            label,
            bytes,
            is_today: offset == 0,
        });
    }

    let total: u64 = points.iter().map(|point| point.bytes).sum();
    let average_bytes = total as f64 / TRAFFIC_TREND_DAYS as f64;

    TrafficTrendData {
        points,
        average_bytes,
    }
}

fn traffic_column(
    icon: IconName,
    label: &str,
    footer_label: &str,
    speed: &str,
    total: &str,
    color: impl Into<Hsla>,
    sparkline: AnyElement,
    cx: &mut Context<WgApp>,
) -> Div {
    let color: Hsla = color.into();
    let icon_small = icon.clone();
    v_flex()
        .gap_2()
        .flex_grow()
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
                .text_3xl()
                .font_semibold()
                .text_color(color)
                .child(speed.to_string()),
        )
        .child(div().h(px(140.0)).w_full().child(sparkline))
        .child(
            h_flex()
                .items_center()
                .gap_2()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(Icon::new(icon_small).size_3().text_color(color))
                .child(format!("{footer_label} {total}")),
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
                .child(
                    Icon::new(icon)
                        .size_4()
                        .text_color(cx.theme().accent_foreground),
                )
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
        .flex_grow()
        .min_w(px(0.0))
        .px_4()
        .py_2()
        .child(
            h_flex()
                .items_center()
                .gap_2()
                .text_sm()
                .font_semibold()
                .text_color(cx.theme().foreground)
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
        .flex_grow()
        .min_w(px(0.0))
        .px_4()
        .py_2()
        .child(
            h_flex()
                .items_center()
                .gap_2()
                .text_sm()
                .font_semibold()
                .text_color(cx.theme().foreground)
                .child(Icon::new(icon).size_3().text_color(color))
                .child(label.to_string()),
        )
        .child(
            div()
                .text_base()
                .font_semibold()
                .text_color(cx.theme().foreground)
                .child(value.to_string()),
        )
}

fn status_state_item(is_running: bool, cx: &mut Context<WgApp>) -> Div {
    let (state_text, state_icon, tag) = if is_running {
        ("On", IconName::CircleCheck, Tag::success())
    } else {
        ("Off", IconName::CircleX, Tag::secondary().outline())
    };

    v_flex()
        .gap_1()
        .flex_grow()
        .min_w(px(0.0))
        .px_4()
        .py_2()
        .child(
            div()
                .text_sm()
                .font_semibold()
                .text_color(cx.theme().foreground)
                .child("Status"),
        )
        .child(
            tag.small()
                .gap_1()
                .child(Icon::new(state_icon).size_3())
                .child(state_text),
        )
}

fn two_row_grid(top: [Div; 3], bottom: [Div; 3], cx: &mut Context<WgApp>) -> Div {
    let [top_left, top_mid, top_right] = top;
    let [bottom_left, bottom_mid, bottom_right] = bottom;
    let border = cx.theme().border;
    div()
        .grid()
        .grid_cols(3)
        .gap_0()
        .child(top_left.border_r_1().border_color(border))
        .child(top_mid.border_r_1().border_color(border))
        .child(top_right)
        .child(bottom_left.border_r_1().border_t_1().border_color(border))
        .child(bottom_mid.border_r_1().border_t_1().border_color(border))
        .child(bottom_right.border_t_1().border_color(border))
}

fn vertical_rule(cx: &mut Context<WgApp>) -> Div {
    div().w(px(1.0)).h(px(64.0)).bg(cx.theme().border)
}

fn format_speeds(app: &WgApp, _data: &ViewData) -> (String, String) {
    if !app.running {
        return ("0.0 KB/s".to_string(), "0.0 KB/s".to_string());
    }
    let upload = app.tx_rate_bps;
    let download = app.rx_rate_bps;
    (format_speed(upload), format_speed(download))
}

fn format_speed(bytes_per_sec: f64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * KB;
    if bytes_per_sec >= MB {
        format!("{:.1} MB/s", bytes_per_sec / MB)
    } else if bytes_per_sec >= KB {
        format!("{:.1} KB/s", bytes_per_sec / KB)
    } else {
        format!("{:.0} B/s", bytes_per_sec)
    }
}

struct SparklinePoint {
    label: String,
    value: f64,
}

fn build_sparkline_points(series: &[f32]) -> Vec<SparklinePoint> {
    series
        .iter()
        .enumerate()
        .map(|(idx, value)| SparklinePoint {
            label: idx.to_string(),
            value: *value as f64,
        })
        .collect()
}

fn sparkline_chart(points: Vec<SparklinePoint>, stroke: impl Into<Hsla>) -> AnyElement {
    let tick_margin = points.len().saturating_add(1);
    LineChart::new(points)
        .x(|point| point.label.clone())
        .y(|point| point.value)
        .stroke(stroke)
        .linear()
        .tick_margin(tick_margin)
        .into_any_element()
}

fn format_avg_bytes(bytes: f64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * KB;
    const GB: f64 = 1024.0 * MB;

    if bytes >= GB {
        format!("{:.2}GiB", bytes / GB)
    } else if bytes >= MB {
        format!("{:.2}MiB", bytes / MB)
    } else if bytes >= KB {
        format!("{:.2}KiB", bytes / KB)
    } else {
        format!("{:.0}B", bytes)
    }
}

struct TrafficAvgLine {
    points: Vec<TrafficTrendPoint>,
    average_bytes: f64,
    avg_color: Hsla,
}

impl TrafficAvgLine {
    fn new(points: Vec<TrafficTrendPoint>, average_bytes: f64, avg_color: Hsla) -> Self {
        Self {
            points,
            average_bytes,
            avg_color,
        }
    }
}

impl IntoElement for TrafficAvgLine {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TrafficAvgLine {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let style = Style {
            size: Size::full(),
            ..Default::default()
        };
        (window.request_layout(style, None, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Window,
        _: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        _cx: &mut App,
    ) {
        if self.points.is_empty() {
            return;
        }

        let width = bounds.size.width.as_f32();
        let height = bounds.size.height.as_f32() - AXIS_GAP;

        let mut domain: Vec<f64> = self.points.iter().map(|point| point.bytes as f64).collect();
        domain.push(0.0);
        let y_scale = ScaleLinear::new(domain, vec![height, 10.0]);

        let avg_y = y_scale.tick(&self.average_bytes).unwrap_or(height);
        let avg_line = Line::new()
            .data(vec![(0.0_f32, avg_y), (width, avg_y)])
            .x(|point| Some(point.0))
            .y(|point| Some(point.1))
            .stroke(self.avg_color)
            .stroke_width(px(2.0))
            .stroke_style(StrokeStyle::Linear);

        avg_line.paint(&bounds, window);
    }
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
        .map(|cfg| {
            cfg.peers
                .iter()
                .map(|peer| peer.allowed_ips.len())
                .sum::<usize>()
        })
        .unwrap_or(0);
    if count == 0 {
        "-".to_string()
    } else {
        format!("{count} routes")
    }
}

fn format_memory_usage() -> String {
    match read_process_rss_bytes() {
        Some(bytes) => format_memory(bytes),
        None => "-".to_string(),
    }
}

fn read_process_rss_bytes() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            let mut parts = rest.split_whitespace();
            let kb = parts.next()?.parse::<u64>().ok()?;
            return Some(kb.saturating_mul(1024));
        }
    }
    None
}

fn format_memory(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * KB;
    const GB: f64 = 1024.0 * MB;

    let value = bytes as f64;
    if value >= GB {
        format!("{:.1} GB", value / GB)
    } else if value >= MB {
        format!("{:.0} MB", value / MB)
    } else if value >= KB {
        format!("{:.0} KB", value / KB)
    } else {
        format!("{bytes} B")
    }
}
