mod data;
mod events;
mod explain;
mod graph;
mod inspector;
mod inventory;

use gpui::*;
use gpui_component::{
    button::{Button, ButtonGroup},
    group_box::{GroupBox, GroupBoxVariants},
    h_flex,
    input::Input,
    resizable::{h_resizable, resizable_panel},
    tag::Tag,
    v_flex, ActiveTheme as _, Selectable, Sizable as _, StyledExt as _,
};

use crate::ui::state::{RouteFamilyFilter, RouteMapMode, WgApp};
use crate::ui::view::data::ViewData;

use self::data::{RouteMapChip, RouteMapData, RouteMapItemStatus, RouteMapTone};

pub(crate) fn render_route_map(
    app: &mut WgApp,
    data: &ViewData,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) -> Div {
    app.ensure_route_map_search_input(window, cx);
    let search_input = app
        .ui
        .route_map_search_input
        .clone()
        .expect("route map search input should be initialized");
    let query = search_input.read(cx).value().to_string();
    let model = RouteMapData::new(app, data, &query);

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.0))
        .rounded_lg()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().tiles)
        .overflow_hidden()
        .child(render_header(&model, cx))
        .child(render_toolbar(app, &search_input, cx))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .w_full()
                .min_h(px(0.0))
                .child(
                    h_resizable("route-map-layout")
                        .child(
                            resizable_panel()
                                .size(px(280.0))
                                .size_range(px(240.0)..px(360.0))
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .flex_1()
                                        .w_full()
                                        .h_full()
                                        .min_h(px(0.0))
                                        .p_3()
                                        .child(inventory::render_inventory(&model, cx)),
                                ),
                        )
                        .child(
                            resizable_panel().size_range(px(420.0)..Pixels::MAX).child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .w_full()
                                    .h_full()
                                    .min_h(px(0.0))
                                    .p_3()
                                    .child(graph::render_graph(
                                        &model,
                                        app.ui_session.route_map_mode,
                                        cx,
                                    )),
                            ),
                        )
                        .child(
                            resizable_panel()
                                .size(px(340.0))
                                .size_range(px(280.0)..px(420.0))
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .flex_1()
                                        .w_full()
                                        .h_full()
                                        .min_h(px(0.0))
                                        .p_3()
                                        .child(inspector::render_inspector(&model, cx)),
                                ),
                        ),
                ),
        )
}

fn render_header(model: &RouteMapData, cx: &mut Context<WgApp>) -> Div {
    div()
        .px_6()
        .py_5()
        .border_b_1()
        .border_color(cx.theme().border)
        .bg(linear_gradient(
            135.0,
            linear_color_stop(cx.theme().background.alpha(0.98), 0.0),
            linear_color_stop(cx.theme().muted.alpha(0.72), 1.0),
        ))
        .child(
            h_flex()
                .items_start()
                .justify_between()
                .gap_4()
                .flex_wrap()
                .child(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_xs()
                                .font_semibold()
                                .text_color(cx.theme().muted_foreground)
                                .child("ROUTE DECISION MAP"),
                        )
                        .child(div().text_xl().font_semibold().child("Route Map"))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(model.plan_status.clone()),
                        ),
                )
                .child(
                    h_flex()
                        .items_center()
                        .flex_wrap()
                        .gap_2()
                        .child(
                            Tag::secondary()
                                .small()
                                .rounded_full()
                                .child(model.source_label.clone()),
                        )
                        .child(
                            Tag::secondary()
                                .small()
                                .rounded_full()
                                .child(model.platform_label.clone()),
                        )
                        .children(model.summary_chips.iter().map(summary_chip)),
                ),
        )
}

fn render_toolbar(
    app: &mut WgApp,
    search_input: &Entity<gpui_component::input::InputState>,
    cx: &mut Context<WgApp>,
) -> Div {
    let mode_group = ButtonGroup::new("route-map-mode")
        .outline()
        .compact()
        .small()
        .child(mode_button(
            RouteMapMode::Flow,
            app.ui_session.route_map_mode,
            cx,
        ))
        .child(mode_button(
            RouteMapMode::Routes,
            app.ui_session.route_map_mode,
            cx,
        ))
        .child(mode_button(
            RouteMapMode::Explain,
            app.ui_session.route_map_mode,
            cx,
        ))
        .child(mode_button(
            RouteMapMode::Events,
            app.ui_session.route_map_mode,
            cx,
        ));

    let family_group = ButtonGroup::new("route-map-family")
        .outline()
        .compact()
        .small()
        .child(family_button(
            RouteFamilyFilter::All,
            app.ui_session.route_map_family_filter,
            cx,
        ))
        .child(family_button(
            RouteFamilyFilter::Ipv4,
            app.ui_session.route_map_family_filter,
            cx,
        ))
        .child(family_button(
            RouteFamilyFilter::Ipv6,
            app.ui_session.route_map_family_filter,
            cx,
        ));

    div()
        .px_6()
        .py_3()
        .border_b_1()
        .border_color(cx.theme().border.alpha(0.6))
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .gap_3()
                .flex_wrap()
                .child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(mode_group)
                        .child(family_group),
                )
                .child(
                    div()
                        .w(px(320.0))
                        .max_w_full()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .bg(cx.theme().secondary)
                        .child(
                            Input::new(search_input)
                                .appearance(false)
                                .bordered(false)
                                .cleanable(true),
                        ),
                ),
        )
}

fn mode_button(mode: RouteMapMode, current: RouteMapMode, cx: &mut Context<WgApp>) -> Button {
    let id = match mode {
        RouteMapMode::Flow => "route-map-mode-flow",
        RouteMapMode::Routes => "route-map-mode-routes",
        RouteMapMode::Explain => "route-map-mode-explain",
        RouteMapMode::Events => "route-map-mode-events",
    };

    Button::new(id)
        .label(mode.label())
        .selected(current == mode)
        .on_click(cx.listener(move |this, _, _, cx| {
            this.set_route_map_mode(mode, cx);
        }))
}

fn family_button(
    family: RouteFamilyFilter,
    current: RouteFamilyFilter,
    cx: &mut Context<WgApp>,
) -> Button {
    let id = match family {
        RouteFamilyFilter::All => "route-map-family-all",
        RouteFamilyFilter::Ipv4 => "route-map-family-v4",
        RouteFamilyFilter::Ipv6 => "route-map-family-v6",
    };

    Button::new(id)
        .label(family.label())
        .selected(current == family)
        .on_click(cx.listener(move |this, _, _, cx| {
            this.set_route_map_family_filter(family, cx);
        }))
}

pub(super) fn summary_chip(chip: &RouteMapChip) -> Tag {
    match chip.tone {
        RouteMapTone::Secondary => Tag::secondary()
            .small()
            .rounded_full()
            .child(chip.label.clone()),
        RouteMapTone::Info => Tag::info().small().rounded_full().child(chip.label.clone()),
        RouteMapTone::Success => Tag::success()
            .small()
            .rounded_full()
            .child(chip.label.clone()),
        RouteMapTone::Warning => Tag::warning()
            .small()
            .rounded_full()
            .child(chip.label.clone()),
    }
}

pub(super) fn status_chip(status: RouteMapItemStatus) -> Tag {
    match status {
        RouteMapItemStatus::Planned => Tag::secondary()
            .small()
            .rounded_full()
            .child(status.label()),
        RouteMapItemStatus::Applied => Tag::success().small().rounded_full().child(status.label()),
        RouteMapItemStatus::Skipped => Tag::warning().small().rounded_full().child(status.label()),
        RouteMapItemStatus::Failed => Tag::danger().small().rounded_full().child(status.label()),
        RouteMapItemStatus::Warning => Tag::warning().small().rounded_full().child(status.label()),
    }
}

pub(super) fn empty_group(
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
