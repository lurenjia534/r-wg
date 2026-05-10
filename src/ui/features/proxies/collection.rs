use gpui::prelude::FluentBuilder as _;
use gpui::{uniform_list, InteractiveElement as _, StatefulInteractiveElement as _, *};
use gpui_component::{
    h_flex, scroll::Scrollbar, tag::Tag, ActiveTheme as _, Sizable as _, StyledExt as _,
};

use crate::ui::state::WgApp;

use super::badges::endpoint_family_tag;
use super::grid::{proxy_grid, ProxyGridMetrics};
use super::model::ProxyRowData;

const PROXIES_CARD_WIDTH: f32 = 240.0;
const PROXIES_GALLERY_CARD_HEIGHT: f32 = 104.0;
const PROXIES_CARD_GAP: f32 = 8.0;
const PROXIES_LIST_ROW_HEIGHT: f32 = 50.0;
const PROXIES_GALLERY_SCROLL_STATE_ID: &str = "proxies-gallery-scroll";
const PROXIES_LIST_SCROLL_STATE_ID: &str = "proxies-list-scroll";

pub(super) fn render_proxy_list_view(
    filtered_rows: &[ProxyRowData],
    window: &mut Window,
    cx: &mut Context<WgApp>,
) -> Div {
    let app_entity = cx.entity();
    let rows = filtered_rows.to_vec();
    let scroll_handle = window
        .use_keyed_state(PROXIES_LIST_SCROLL_STATE_ID, cx, |_, _| {
            UniformListScrollHandle::new()
        })
        .read(cx)
        .clone();
    let list = uniform_list(
        "proxy-list",
        rows.len(),
        move |visible_range, _window, cx| {
            app_entity.update(cx, |this, cx| {
                visible_range
                    .map(|ix| proxy_list_row(this, &rows[ix], cx))
                    .collect::<Vec<_>>()
            })
        },
    )
    .track_scroll(scroll_handle.clone())
    .w_full()
    .flex_1();

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.0))
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().background)
        .child(proxy_list_header(cx))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h(px(0.0))
                .relative()
                .child(list)
                .child(Scrollbar::vertical(&scroll_handle)),
        )
}

pub(super) fn render_proxy_gallery_view(
    filtered_rows: &[ProxyRowData],
    window: &mut Window,
    cx: &mut Context<WgApp>,
) -> Div {
    let app_entity = cx.entity();
    let rows = filtered_rows.to_vec();
    let scroll_handle = window
        .use_keyed_state(PROXIES_GALLERY_SCROLL_STATE_ID, cx, |_, _| {
            ScrollHandle::default()
        })
        .read(cx)
        .clone();
    let grid = proxy_grid(
        "proxy-gallery-grid",
        rows.len(),
        proxy_grid_metrics(),
        scroll_handle.clone(),
        move |visible_range, _window, cx| {
            app_entity.update(cx, |this, cx| {
                visible_range
                    .map(|ix| proxy_gallery_card(this, &rows[ix], cx))
                    .collect::<Vec<_>>()
            })
        },
    )
    .with_overscan_viewports(1.0);

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.0))
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().background)
        .child(
            div()
                .id("proxy-gallery-scroll")
                .flex()
                .flex_col()
                .flex_1()
                .min_h(px(0.0))
                .overflow_y_scroll()
                .track_scroll(&scroll_handle)
                .p_2()
                .child(grid),
        )
        .child(Scrollbar::vertical(&scroll_handle))
}

fn proxy_grid_metrics() -> ProxyGridMetrics {
    ProxyGridMetrics {
        card_width: px(PROXIES_CARD_WIDTH),
        card_height: px(PROXIES_GALLERY_CARD_HEIGHT),
        gap: px(PROXIES_CARD_GAP),
    }
}

fn proxy_list_header(cx: &mut Context<WgApp>) -> Div {
    div()
        .flex()
        .items_center()
        .gap_3()
        .px_3()
        .h(px(PROXIES_LIST_ROW_HEIGHT))
        .border_b_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().secondary.alpha(0.55))
        .child(column_label("Name", cx).flex_1())
        .child(column_label("Location", cx).w(px(150.0)))
        .child(column_label("Type", cx).w(px(72.0)))
        .child(column_label("Family", cx).w(px(96.0)))
        .child(column_label("Source", cx).w(px(72.0)))
        .child(column_label("Status", cx).w(px(84.0)))
}

fn proxy_list_row(app: &WgApp, row: &ProxyRowData, cx: &mut Context<WgApp>) -> Stateful<Div> {
    let is_selected = app.selection.selected_id == Some(row.id);
    let is_multi_selected =
        app.selection.proxy_select_mode && app.selection.proxy_selected_ids.contains(&row.id);
    let bg = if is_selected {
        cx.theme().list_active
    } else if is_multi_selected {
        cx.theme().info.alpha(0.08)
    } else {
        cx.theme().background
    };
    let border_color = if is_selected {
        cx.theme().list_active_border
    } else if is_multi_selected {
        cx.theme().info.alpha(0.32)
    } else {
        cx.theme().border.alpha(0.35)
    };

    let status = if row.is_running { "Running" } else { "Idle" };
    let config_id = row.id;

    let mut base = div()
        .id(("proxy-list-row", config_id))
        .flex()
        .items_center()
        .gap_3()
        .px_3()
        .h(px(PROXIES_LIST_ROW_HEIGHT))
        .border_b_1()
        .border_color(border_color)
        .bg(bg)
        .child(
            div()
                .flex_1()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .font_semibold()
                        .truncate()
                        .child(row.name.clone()),
                )
                .when(is_multi_selected, |this| {
                    this.child(Tag::info().small().child("Selected"))
                }),
        )
        .child(column_value(row.location_label()).w(px(150.0)))
        .child(column_value(row.protocol_label().to_string()).w(px(72.0)))
        .child(
            div().w(px(96.0)).child(
                endpoint_family_tag(row.endpoint_family)
                    .unwrap_or_else(|| Tag::secondary().small().child("Unknown")),
            ),
        )
        .child(column_value(row.source_kind.to_string()).w(px(72.0)))
        .child(div().w(px(84.0)).child(if row.is_running {
            Tag::success().small().child(status)
        } else {
            Tag::secondary().small().child(status)
        }));

    if !app.runtime.busy {
        base = base.cursor_pointer();
    }

    base.on_click(cx.listener(move |this, _, window, cx| {
        if this.runtime.busy {
            return;
        }
        if this.selection.proxy_select_mode {
            this.command_toggle_proxy_multi_selection(config_id, cx);
            return;
        }
        this.select_tunnel(config_id, window, cx);
    }))
}

fn proxy_gallery_card(app: &WgApp, row: &ProxyRowData, cx: &mut Context<WgApp>) -> Stateful<Div> {
    let is_selected = app.selection.selected_id == Some(row.id);
    let is_multi_selected =
        app.selection.proxy_select_mode && app.selection.proxy_selected_ids.contains(&row.id);
    let bg = if is_selected {
        cx.theme().list_active
    } else if is_multi_selected {
        cx.theme().info.alpha(0.08)
    } else {
        cx.theme().secondary
    };
    let border_color = if is_selected {
        cx.theme().list_active_border
    } else if is_multi_selected {
        cx.theme().info.alpha(0.45)
    } else if cx.theme().is_dark() {
        cx.theme().foreground.alpha(0.12)
    } else {
        cx.theme().border
    };

    let mut badges = h_flex().gap_1();
    if is_multi_selected {
        badges = badges.child(Tag::info().small().child("Selected"));
    }
    if let Some(tag) = endpoint_family_tag(row.endpoint_family) {
        badges = badges.child(tag);
    }
    if row.is_running {
        badges = badges.child(Tag::success().small().child("Running"));
    }

    let mut item = div()
        .id(("proxy-gallery-card", row.id))
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .rounded_lg()
        .border_1()
        .border_color(border_color)
        .bg(bg)
        .w(px(PROXIES_CARD_WIDTH))
        .h(px(PROXIES_GALLERY_CARD_HEIGHT))
        .child(
            div()
                .text_sm()
                .font_semibold()
                .truncate()
                .child(row.name.clone()),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .truncate()
                .child(format!(
                    "{} / {} / {}",
                    row.country_label(),
                    row.city_label(),
                    row.protocol_label()
                )),
        )
        .child(badges);

    if !app.runtime.busy {
        item = item.cursor_pointer();
    }

    let config_id = row.id;
    item.on_click(cx.listener(move |this, _, window, cx| {
        if this.runtime.busy {
            return;
        }
        if this.selection.proxy_select_mode {
            this.command_toggle_proxy_multi_selection(config_id, cx);
            return;
        }
        this.select_tunnel(config_id, window, cx);
    }))
}

fn column_label(text: impl Into<SharedString>, cx: &Context<WgApp>) -> Div {
    div()
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

fn column_value(text: impl Into<SharedString>) -> Div {
    div().text_sm().truncate().child(text.into())
}
