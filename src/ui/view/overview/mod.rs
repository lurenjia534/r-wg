mod cards;
mod chart;
mod common;
mod traffic;

use gpui::*;
use gpui_component::{
    group_box::{GroupBox, GroupBoxVariants},
    ActiveTheme as _,
};

use crate::ui::state::WgApp;
use crate::ui::view::data::OverviewData;

use self::cards::{network_status_card, running_status_card, traffic_stats_card};
use self::traffic::{traffic_summary_card, traffic_trend_card};

/// Overview 页入口：
/// - 只负责页面级组装；
/// - 不再直接读取 `WgApp` 内部状态；
/// - 具体卡片、图表和通用 primitive 交给子模块。
pub(crate) fn render_overview(overview: &OverviewData, cx: &mut Context<WgApp>) -> Div {
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
                .child(running_status_card(overview, cx).flex_grow())
                .child(network_status_card(overview, cx).flex_grow()),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .w_full()
                .gap_3()
                .items_start()
                .child(traffic_stats_card(overview, cx).w(relative(0.5)))
                .child(traffic_trend_card(&overview.traffic_trend, cx).w(relative(0.5))),
        )
        .child(traffic_summary_card(overview, cx))
}

/// 非核心页面的统一占位内容。
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
