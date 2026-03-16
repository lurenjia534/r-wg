use std::{cmp, ops::Range};

use gpui::*;

#[derive(Clone, Copy, Debug)]
pub(super) struct ProxyGridMetrics {
    pub(super) card_width: Pixels,
    pub(super) card_height: Pixels,
    pub(super) gap: Pixels,
}

impl ProxyGridMetrics {
    fn columns_for_width(self, width: Pixels) -> usize {
        ((width + self.gap) / (self.card_width + self.gap))
            .floor()
            .max(1.0) as usize
    }

    fn row_stride(self) -> Pixels {
        self.card_height + self.gap
    }

    fn content_height(self, row_count: usize) -> Pixels {
        if row_count == 0 {
            px(0.0)
        } else {
            self.card_height + self.row_stride() * row_count.saturating_sub(1)
        }
    }
}

type RenderItems = dyn for<'a> Fn(Range<usize>, &'a mut Window, &'a mut App) -> Vec<AnyElement>;

#[track_caller]
pub(super) fn proxy_grid<R>(
    id: impl Into<ElementId>,
    item_count: usize,
    metrics: ProxyGridMetrics,
    scroll_handle: ScrollHandle,
    render_items: impl 'static + Fn(Range<usize>, &mut Window, &mut App) -> Vec<R>,
) -> ProxyGridElement
where
    R: IntoElement,
{
    ProxyGridElement {
        id: id.into(),
        item_count,
        metrics,
        scroll_handle,
        overscan_viewports: 1.0,
        render_items: Box::new(move |range, window, cx| {
            render_items(range, window, cx)
                .into_iter()
                .map(|item| item.into_any_element())
                .collect()
        }),
    }
}

pub(super) struct ProxyGridElement {
    id: ElementId,
    item_count: usize,
    metrics: ProxyGridMetrics,
    scroll_handle: ScrollHandle,
    overscan_viewports: f32,
    render_items: Box<RenderItems>,
}

pub(super) struct ProxyGridFrameState {
    items: Vec<AnyElement>,
}

impl ProxyGridElement {
    pub(super) fn with_overscan_viewports(mut self, viewports: f32) -> Self {
        self.overscan_viewports = viewports.max(1.0);
        self
    }
}

impl IntoElement for ProxyGridElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for ProxyGridElement {
    type RequestLayoutState = ProxyGridFrameState;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        _cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let metrics = self.metrics;
        let item_count = self.item_count;
        let style = Style::default();
        let layout_id = window.request_measured_layout(
            style,
            move |known_dimensions, available_space, _, _| {
                let width = known_dimensions
                    .width
                    .unwrap_or(match available_space.width {
                        AvailableSpace::Definite(width) => width,
                        AvailableSpace::MinContent | AvailableSpace::MaxContent => {
                            metrics.card_width
                        }
                    });
                let columns = metrics.columns_for_width(width);
                let row_count = item_count.div_ceil(columns);
                size(width, metrics.content_height(row_count))
            },
        );
        (layout_id, ProxyGridFrameState { items: Vec::new() })
    }

    fn prepaint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        frame_state: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        frame_state.items.clear();
        if self.item_count == 0 {
            return;
        }

        let viewport_bounds = self.scroll_handle.bounds();
        let viewport_height = if viewport_bounds.size.height > px(0.0) {
            viewport_bounds.size.height
        } else {
            bounds.size.height
        };
        let scroll_offset = self.scroll_handle.offset();
        let columns = self.metrics.columns_for_width(bounds.size.width);
        let row_count = self.item_count.div_ceil(columns);
        let row_stride = self.metrics.row_stride();
        let scroll_top = (-scroll_offset.y).max(px(0.0));
        let first_visible_row = (scroll_top / row_stride).floor().max(0.0) as usize;
        let last_visible_row = ((scroll_top + viewport_height) / row_stride)
            .ceil()
            .max(0.0) as usize;
        let overscan_rows = ((viewport_height / row_stride).ceil() * self.overscan_viewports)
            .ceil()
            .max(2.0) as usize;
        let first_row = first_visible_row.saturating_sub(overscan_rows);
        let last_row = cmp::min(row_count, last_visible_row.saturating_add(overscan_rows));
        let visible_range = first_row * columns..cmp::min(self.item_count, last_row * columns);
        let items = (self.render_items)(visible_range.clone(), window, cx);
        let content_mask = ContentMask {
            bounds: viewport_bounds,
        };

        window.with_content_mask(Some(content_mask), |window| {
            for (ix, mut item) in visible_range.zip(items) {
                let row = ix / columns;
                let column = ix % columns;
                let item_origin = bounds.origin
                    + point(
                        (self.metrics.card_width + self.metrics.gap) * column,
                        row_stride * row,
                    );
                let available_space = size(
                    AvailableSpace::Definite(self.metrics.card_width),
                    AvailableSpace::Definite(self.metrics.card_height),
                );
                item.layout_as_root(available_space, window, cx);
                item.prepaint_at(item_origin, window, cx);
                frame_state.items.push(item);
            }
        });
    }

    fn paint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        frame_state: &mut Self::RequestLayoutState,
        _prepaint_state: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        for item in &mut frame_state.items {
            item.paint(window, cx);
        }
    }
}
