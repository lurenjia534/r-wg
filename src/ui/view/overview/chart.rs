use gpui::*;
use gpui_component::{
    chart::LineChart,
    plot::{
        scale::{Scale, ScaleBand, ScaleLinear},
        shape::Line,
        StrokeStyle, AXIS_GAP,
    },
    PixelsExt,
};

use crate::ui::view::data::TrafficTrendPoint;

pub(super) struct SparklinePoint {
    label: String,
    value: f64,
}

pub(super) fn build_sparkline_points(series: &[f32]) -> Vec<SparklinePoint> {
    series
        .iter()
        .enumerate()
        .map(|(idx, value)| SparklinePoint {
            label: idx.to_string(),
            value: *value as f64,
        })
        .collect()
}

pub(super) fn sparkline_chart(points: Vec<SparklinePoint>, stroke: impl Into<Hsla>) -> AnyElement {
    let tick_margin = points.len().saturating_add(1);
    LineChart::new(points)
        .x(|point| point.label.clone())
        .y(|point| point.value)
        .stroke(stroke)
        .linear()
        .tick_margin(tick_margin)
        .into_any_element()
}

pub(super) fn format_avg_bytes(bytes: f64) -> String {
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
        format!("{bytes:.0}B")
    }
}

pub(super) struct TrafficTrendOverlay {
    points: Vec<TrafficTrendPoint>,
    average_bytes: f64,
    max_bytes: u64,
    peak_bytes: u64,
    show_avg_rule: bool,
    show_trend_line: bool,
    show_peak_marker: bool,
    avg_color: Hsla,
    trend_color: Hsla,
    today_color: Hsla,
    peak_color: Hsla,
}

impl TrafficTrendOverlay {
    pub(super) fn new(
        points: Vec<TrafficTrendPoint>,
        average_bytes: f64,
        max_bytes: u64,
        peak_bytes: u64,
        show_avg_rule: bool,
        show_trend_line: bool,
        show_peak_marker: bool,
        avg_color: Hsla,
        trend_color: Hsla,
        today_color: Hsla,
        peak_color: Hsla,
    ) -> Self {
        Self {
            points,
            average_bytes,
            max_bytes,
            peak_bytes,
            show_avg_rule,
            show_trend_line,
            show_peak_marker,
            avg_color,
            trend_color,
            today_color,
            peak_color,
        }
    }
}

impl IntoElement for TrafficTrendOverlay {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TrafficTrendOverlay {
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
        let labels = self
            .points
            .iter()
            .map(|point| point.label.clone())
            .collect::<Vec<_>>();
        let x_scale = ScaleBand::new(labels, vec![0.0, width])
            .padding_inner(0.4)
            .padding_outer(0.2);
        let band_width = x_scale.band_width();
        let max_domain =
            ((self.max_bytes.max(self.average_bytes.ceil() as u64).max(1) as f64) * 1.08).max(1.0);
        let y_scale = ScaleLinear::new(vec![0.0, max_domain], vec![height, 10.0]);

        #[derive(Clone)]
        struct OverlayPoint {
            x: f32,
            y: f32,
            bytes: u64,
            is_today: bool,
            is_peak: bool,
        }

        let overlay_points = self
            .points
            .iter()
            .filter_map(|point| {
                let x = x_scale.tick(&point.label)?;
                let y = y_scale.tick(&(point.bytes as f64))?;
                Some(OverlayPoint {
                    x: x + band_width / 2.0,
                    y,
                    bytes: point.bytes,
                    is_today: point.is_today,
                    is_peak: self.peak_bytes > 0 && point.bytes == self.peak_bytes,
                })
            })
            .collect::<Vec<_>>();

        if self.show_avg_rule {
            let avg_y = y_scale.tick(&self.average_bytes).unwrap_or(height);
            let dash_width = 10.0_f32;
            let dash_gap = 6.0_f32;
            let stroke_height = px(1.0);
            let mut start_x = 0.0_f32;

            while start_x < width {
                let segment_width = (width - start_x).min(dash_width).max(0.0);
                if segment_width <= 0.0 {
                    break;
                }
                let dash_origin =
                    gpui::point(px(start_x), px(avg_y) - stroke_height / 2.0) + bounds.origin;
                window.paint_quad(gpui::quad(
                    gpui::Bounds::new(dash_origin, gpui::size(px(segment_width), stroke_height)),
                    px(0.5),
                    self.avg_color,
                    px(0.0),
                    gpui::transparent_black(),
                    gpui::BorderStyle::default(),
                ));
                start_x += dash_width + dash_gap;
            }
        }

        if self.show_trend_line {
            let trend_line = Line::new()
                .data(overlay_points.clone())
                .x(|point| Some(point.x))
                .y(|point| Some(point.y))
                .stroke(self.trend_color)
                .stroke_width(px(1.5))
                .stroke_style(StrokeStyle::Linear)
                .dot()
                .dot_size(px(5.0))
                .dot_fill_color(self.trend_color)
                .dot_stroke_color(self.trend_color);
            trend_line.paint(&bounds, window);
        }

        for point in overlay_points {
            if point.bytes == 0 {
                continue;
            }

            let (size_px, fill_color) = if point.is_today {
                (px(9.0), self.today_color)
            } else if self.show_peak_marker && point.is_peak {
                (px(8.0), self.peak_color)
            } else {
                continue;
            };
            let radius = size_px / 2.0;
            let dot_origin =
                gpui::point(px(point.x) - radius, px(point.y) - radius) + bounds.origin;
            window.paint_quad(gpui::quad(
                gpui::Bounds::new(dot_origin, gpui::size(size_px, size_px)),
                radius,
                fill_color,
                px(1.0),
                gpui::transparent_white(),
                gpui::BorderStyle::default(),
            ));
        }
    }
}
