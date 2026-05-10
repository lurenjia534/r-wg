use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariants},
    description_list::DescriptionList,
    group_box::{GroupBox, GroupBoxVariants},
    h_flex,
    tag::Tag,
    ActiveTheme as _, Disableable as _, Selectable, Sizable as _, StyledExt as _,
};

use crate::ui::state::{ConfigSource, WgApp};

use super::badges::endpoint_family_tag;
use super::model::ProxiesViewModel;

pub(super) fn render_proxy_detail_pane(
    app: &WgApp,
    model: &ProxiesViewModel,
    cx: &mut Context<WgApp>,
) -> Div {
    let selected_config = app.selected_config();
    let selected_row = model.selected_row.as_ref();
    let is_running = selected_row.map(|row| row.is_running).unwrap_or(false);
    let selected_hidden = app.selection.selected_id.is_some() && !model.selected_visible;

    let detail_card = match (selected_config, selected_row) {
        (Some(config), Some(row)) => {
            let source_detail = match &config.source {
                ConfigSource::File { origin_path } => origin_path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| config.storage_path.display().to_string()),
                ConfigSource::Paste => "Pasted into local storage".to_string(),
            };
            GroupBox::new().fill().title("Selection").child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().text_lg().font_semibold().child(row.name.clone()))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!(
                                        "{} / {} / {} / {}",
                                        row.country_label(),
                                        row.city_label(),
                                        row.protocol_label(),
                                        row.sequence_label()
                                    )),
                            ),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                endpoint_family_tag(row.endpoint_family)
                                    .unwrap_or_else(|| Tag::secondary().small().child("Unknown")),
                            )
                            .child(if row.is_running {
                                Tag::success().small().child("Running")
                            } else {
                                Tag::secondary().small().child("Idle")
                            })
                            .child(Tag::secondary().small().child(row.source_kind)),
                    )
                    .child(
                        DescriptionList::new()
                            .columns(1)
                            .item("Country", row.country_label().to_string(), 1)
                            .item("City", row.city_label().to_string(), 1)
                            .item("Type", row.protocol_label().to_string(), 1)
                            .item("Node ID", row.sequence_label().to_string(), 1)
                            .item("Storage", config.storage_path.display().to_string(), 1)
                            .item("Source", source_detail, 1),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                Button::new("proxy-detail-connect")
                                    .label(if is_running { "Disconnect" } else { "Connect" })
                                    .disabled(app.runtime.busy)
                                    .selected(is_running)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.command_toggle_tunnel(window, cx);
                                    })),
                            )
                            .child(
                                Button::new("proxy-detail-delete")
                                    .label("Delete")
                                    .danger()
                                    .xsmall()
                                    .disabled(
                                        app.runtime.busy
                                            || app.selection.proxy_select_mode
                                            || is_running,
                                    )
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.command_prompt_delete_selected_proxy(window, cx);
                                    })),
                            ),
                    )
                    .when(selected_hidden, |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("This selection is currently hidden by filters."),
                        )
                    }),
            )
        }
        _ => GroupBox::new().fill().title("Selection").child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("Pick a tunnel to inspect its structured metadata."),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(
                            "The management view keeps dense rows on the left and details on the right.",
                        ),
                ),
        ),
    };

    div()
        .flex()
        .flex_col()
        .gap_3()
        .min_h(px(0.0))
        .child(detail_card)
}
