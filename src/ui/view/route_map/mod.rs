pub(crate) mod data;
pub(crate) mod explain;
mod presenter;
use gpui::*;
use gpui_component::{
    group_box::{GroupBox, GroupBoxVariants},
    tag::Tag,
    ActiveTheme as _, Sizable as _,
};

use crate::ui::state::WgApp;

use self::data::{RouteMapChip, RouteMapItemStatus, RouteMapTone};

pub(crate) fn summary_chip(chip: &RouteMapChip) -> Tag {
    match chip.tone {
        RouteMapTone::Secondary => Tag::secondary()
            .small()
            .rounded_full()
            .child(chip.label.clone()),
        RouteMapTone::Info => Tag::info().small().rounded_full().child(chip.label.clone()),
        RouteMapTone::Warning => Tag::warning()
            .small()
            .rounded_full()
            .child(chip.label.clone()),
    }
}

pub(crate) fn status_chip(status: RouteMapItemStatus) -> Tag {
    match status {
        RouteMapItemStatus::Planned => Tag::secondary()
            .small()
            .rounded_full()
            .child(status.label()),
        RouteMapItemStatus::Applied => Tag::success().small().rounded_full().child(status.label()),
        RouteMapItemStatus::Skipped => Tag::secondary()
            .outline()
            .small()
            .rounded_full()
            .child(status.label()),
        RouteMapItemStatus::Failed => Tag::danger().small().rounded_full().child(status.label()),
        RouteMapItemStatus::Warning => Tag::warning().small().rounded_full().child(status.label()),
    }
}

pub(crate) fn empty_group(
    title: &str,
    body: impl IntoElement,
    cx: &mut Context<WgApp>,
) -> GroupBox {
    GroupBox::new().fill().title(title.to_string()).child(
        div()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(body),
    )
}
