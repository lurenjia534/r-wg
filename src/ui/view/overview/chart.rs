use gpui::*;
use gpui_component::{
    chart::LineChart,
    plot::{
        scale::{Scale, ScaleLinear},
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

pub(super) struct TrafficAvgLine {
    points: Vec<TrafficTrendPoint>,
    average_bytes: f64,
    avg_color: Hsla,
}

impl TrafficAvgLine {
    pub(super) fn new(points: Vec<TrafficTrendPoint>, average_bytes: f64, avg_color: Hsla) -> Self {
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
            .stroke_width(px(1.0))
            .stroke_style(StrokeStyle::Linear);

        avg_line.paint(&bounds, window);
    }
}
