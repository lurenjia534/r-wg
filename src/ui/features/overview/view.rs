use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    group_box::{GroupBox, GroupBoxVariants},
    h_flex,
    scroll::ScrollableElement as _,
    tag::Tag,
    ActiveTheme as _, Sizable as _,
};

use crate::ui::state::WgApp;
use crate::ui::view::overview::{
    cards::{network_status_card, running_status_card, traffic_stats_card},
    traffic::{traffic_summary_card, traffic_trend_card},
    view_model::OverviewData,
};
use crate::ui::view::{PageShell, PageShellHeader};

const OVERVIEW_COMPACT_BREAKPOINT: f32 = 1180.0;
const OVERVIEW_STACK_BREAKPOINT: f32 = 980.0;

#[derive(Clone, PartialEq, Eq)]
struct OverviewCacheKey {
    stats_revision: u64,
    traffic_revision: u64,
    selection_revision: u64,
    runtime_revision: u64,
    traffic_period: crate::ui::state::TrafficPeriod,
    configs_fingerprint: u64,
}

impl OverviewCacheKey {
    fn from_app(app: &WgApp) -> Self {
        Self {
            stats_revision: app.stats.stats_revision,
            traffic_revision: app.stats.traffic.rev,
            selection_revision: app.selection.selection_revision,
            runtime_revision: app.runtime.runtime_revision,
            traffic_period: app.ui_session.traffic_period,
            configs_fingerprint: fingerprint_configs(app),
        }
    }
}

fn fingerprint_configs(app: &WgApp) -> u64 {
    let mut hasher = DefaultHasher::new();
    for config in app.configs.iter() {
        config.id.hash(&mut hasher);
        config.name.hash(&mut hasher);
        config.storage_path.hash(&mut hasher);
        match &config.source {
            crate::ui::state::ConfigSource::File { origin_path } => {
                "file".hash(&mut hasher);
                origin_path.hash(&mut hasher);
            }
            crate::ui::state::ConfigSource::Paste => {
                "paste".hash(&mut hasher);
            }
        }
        config
            .text
            .as_ref()
            .map(|text| text.as_ref())
            .hash(&mut hasher);
    }
    hasher.finish()
}

pub(crate) struct OverviewPageState {
    app: Entity<WgApp>,
    cache_key: Option<OverviewCacheKey>,
    snapshot: Option<Arc<OverviewData>>,
}

impl OverviewPageState {
    fn new(app: Entity<WgApp>) -> Self {
        Self {
            app,
            cache_key: None,
            snapshot: None,
        }
    }

    fn refresh_snapshot(&mut self, cx: &mut Context<Self>) {
        let app = self.app.read(cx);
        let next_key = OverviewCacheKey::from_app(app);
        if self.cache_key.as_ref() == Some(&next_key) {
            return;
        }

        self.cache_key = Some(next_key);
        self.snapshot = Some(Arc::new(OverviewData::new(app)));
    }
}

impl Render for OverviewPageState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.refresh_snapshot(cx);
        let snapshot = self
            .snapshot
            .as_ref()
            .expect("overview snapshot should exist")
            .clone();
        render_overview_snapshot(&self.app, &snapshot, window, cx)
    }
}

pub(crate) fn ensure_overview_page(
    app: Entity<WgApp>,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) -> Entity<OverviewPageState> {
    let app_handle = app.clone();
    window.use_keyed_state("overview-page-entity", cx, move |_, cx| {
        let app_handle = app_handle.clone();
        let _ = cx;
        OverviewPageState::new(app_handle)
    })
}

fn render_overview_snapshot<T>(
    app: &Entity<WgApp>,
    overview: &OverviewData,
    window: &mut Window,
    cx: &mut Context<T>,
) -> Div {
    let compact = window.viewport_size().width < px(OVERVIEW_COMPACT_BREAKPOINT);
    let stacked = window.viewport_size().width < px(OVERVIEW_STACK_BREAKPOINT);
    let runtime = &overview.runtime;
    let preview = &overview.preview;
    let header_actions = h_flex()
        .items_center()
        .flex_wrap()
        .gap_2()
        .child(
            Tag::secondary()
                .xsmall()
                .rounded_full()
                .child(format!("Updated {}", runtime.last_updated_text)),
        )
        .when(preview.has_selection, |this| {
            this.child(
                Tag::secondary()
                    .outline()
                    .xsmall()
                    .rounded_full()
                    .child(format!("Selected {}", preview.selected_name_text)),
            )
        });

    PageShell::new(
        PageShellHeader::new(
            "DASHBOARD",
            "Overview",
            "Runtime health, traffic, and selected config context in one place.",
        )
        .actions(header_actions),
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scrollbar()
            .px_5()
            .py_3()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .w_full()
                    .child(
                        h_flex()
                            .items_start()
                            .gap_3()
                            .flex_wrap()
                            .when(stacked, |this| this.flex_col())
                            .child(running_status_card(overview, cx).min_w(px(320.0)).flex_1())
                            .child(
                                network_status_card(app, overview, cx)
                                    .min_w(px(320.0))
                                    .flex_1(),
                            ),
                    )
                    .child(
                        h_flex()
                            .items_start()
                            .gap_3()
                            .flex_wrap()
                            .when(stacked, |this| this.flex_col())
                            .child(
                                traffic_stats_card(overview, cx)
                                    .min_w(px(if compact { 320.0 } else { 420.0 }))
                                    .flex_1(),
                            )
                            .child(
                                traffic_trend_card(&overview.traffic_trend, cx)
                                    .min_w(px(if compact { 320.0 } else { 360.0 }))
                                    .flex_1(),
                            ),
                    )
                    .child(traffic_summary_card(app, overview, cx)),
            ),
    )
    .render(cx)
}

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
