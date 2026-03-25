mod data;
mod events;
mod explain;
mod graph;
mod inspector;
mod inventory;
mod presenter;

use gpui::*;
use gpui_component::{
    button::{Button, ButtonGroup},
    group_box::{GroupBox, GroupBoxVariants},
    h_flex,
    input::Input,
    resizable::{h_resizable, resizable_panel, ResizableState},
    scroll::ScrollableElement as _,
    tab::{Tab, TabBar},
    tag::Tag,
    v_flex, ActiveTheme as _, PixelsExt as _, Selectable, Sizable as _, StyledExt as _,
};

use crate::ui::state::{RouteFamilyFilter, RouteMapMode, WgApp};
use crate::ui::view::shared::ViewData;
use crate::ui::view::widgets::PageShell;

use self::data::{
    EffectiveRoutePlan, RouteMapChip, RouteMapData, RouteMapEvidence, RouteMapItemStatus,
    RouteMapTone,
};

const ROUTE_MAP_STACK_BREAKPOINT: f32 = 1360.0;
const ROUTE_MAP_COMPACT_INVENTORY_HEIGHT: f32 = 280.0;
const ROUTE_MAP_COMPACT_GRAPH_HEIGHT: f32 = 460.0;
const ROUTE_MAP_COMPACT_INSPECTOR_HEIGHT: f32 = 320.0;
const ROUTE_MAP_COMPACT_EVENTS_HEIGHT: f32 = 520.0;

#[derive(Default)]
struct RouteMapWorkspaceState {
    plan: Option<EffectiveRoutePlan>,
    evidence: Option<RouteMapEvidence>,
}

impl RouteMapWorkspaceState {
    fn refresh(&mut self, app: &WgApp, data: &ViewData) {
        let plan_key = RouteMapData::plan_key(app, data);
        if self.plan.as_ref().map(|plan| plan.cache_key) != Some(plan_key) {
            self.plan = Some(RouteMapData::build_plan(app, data));
        }

        let evidence = RouteMapData::build_evidence();
        if self.evidence.as_ref().map(|cached| cached.cache_key) != Some(evidence.cache_key) {
            self.evidence = Some(evidence);
        }
    }

    fn model(&self, app: &WgApp, query: &str) -> RouteMapData {
        RouteMapData::from_cached(
            app,
            query,
            self.plan.as_ref().expect("route map plan should be cached"),
            self.evidence
                .as_ref()
                .expect("route map evidence should be cached"),
        )
    }
}

pub(crate) fn render_route_map(
    app: &mut WgApp,
    data: &ViewData,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) -> impl IntoElement {
    app.ensure_route_map_search_input(window, cx);
    let app_handle = cx.entity();
    let search_input = app
        .ui
        .route_map_search_input
        .clone()
        .expect("route map search input should be initialized");
    let raw_query = search_input.read(cx).value().to_string();
    app.sync_route_map_search_query(raw_query, cx);
    let query = app.ui.route_map_search.debounced_query.to_string();
    let inventory_width = app.ui_prefs.route_map_inventory_width;
    let inspector_width = app.ui_prefs.route_map_inspector_width;
    let compact_layout = window.viewport_size().width < px(ROUTE_MAP_STACK_BREAKPOINT);
    let workspace = window.use_keyed_state("route-map-workspace", cx, |_, _| {
        RouteMapWorkspaceState::default()
    });
    workspace.update(cx, |state, _| {
        state.refresh(app, data);
    });
    let model = workspace.read(cx).model(app, &query);
    let mode = app.ui_session.route_map_mode;
    let key_search_input = search_input.clone();
    let key_explain_match_id = model.explain_match_id.clone();
    let key_selected_item_id = model.selected_item_id.clone();

    PageShell::custom_header(
        render_header(&model, cx),
        div()
            .flex()
            .flex_col()
            .flex_1()
            .w_full()
            .min_h(px(0.0))
            .child(if compact_layout && mode == RouteMapMode::Events {
                render_compact_events_layout(app, &model, window, cx)
            } else if compact_layout {
                render_compact_layout(app, &model, mode, window, cx)
            } else if mode == RouteMapMode::Events {
                render_events_layout(app, &model, inventory_width, app_handle.clone(), window, cx)
            } else {
                render_standard_layout(
                    app,
                    &model,
                    inventory_width,
                    inspector_width,
                    mode,
                    app_handle.clone(),
                    window,
                    cx,
                )
            }),
    )
    .toolbar(render_toolbar(app, &search_input, cx))
    .render(cx)
    .id("route-map-root")
    .key_context("RouteMap")
    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
        handle_route_map_keydown(
            this,
            key_explain_match_id.as_ref(),
            key_selected_item_id.as_ref(),
            &key_search_input,
            event,
            window,
            cx,
        );
    }))
}

#[allow(clippy::too_many_arguments)]
fn render_standard_layout(
    app: &mut WgApp,
    model: &RouteMapData,
    inventory_width: f32,
    inspector_width: f32,
    mode: RouteMapMode,
    app_handle: Entity<WgApp>,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) -> AnyElement {
    h_resizable("route-map-layout")
        .on_resize(move |state: &Entity<ResizableState>, _window, cx| {
            let sizes = state.read(cx).sizes().clone();
            if sizes.len() < 3 {
                return;
            }
            app_handle.update(cx, |app, cx| {
                let changed =
                    app.persist_route_map_panel_widths(sizes[0].as_f32(), sizes[2].as_f32(), cx);
                if changed {
                    cx.notify();
                }
            });
        })
        .child(
            resizable_panel()
                .size(px(inventory_width))
                .size_range(px(240.0)..px(360.0))
                .child(panel_shell(
                    inventory::render_inventory(app, model, window, cx).into_any_element(),
                )),
        )
        .child(
            resizable_panel()
                .size_range(px(420.0)..Pixels::MAX)
                .child(panel_shell(
                    graph::render_graph(model, mode, window, cx).into_any_element(),
                )),
        )
        .child(
            resizable_panel()
                .size(px(inspector_width))
                .size_range(px(280.0)..px(420.0))
                .child(panel_shell(
                    inspector::render_inspector(app, model, cx).into_any_element(),
                )),
        )
        .into_any_element()
}

fn render_compact_layout(
    app: &mut WgApp,
    model: &RouteMapData,
    mode: RouteMapMode,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .w_full()
        .min_h(px(0.0))
        .overflow_hidden()
        .child(
            div()
                .w_full()
                .flex_1()
                .min_h(px(0.0))
                .overflow_y_scrollbar()
                .child(
                    v_flex()
                        .w_full()
                        .gap_3()
                        .p_3()
                        .child(compact_panel(
                            px(ROUTE_MAP_COMPACT_INVENTORY_HEIGHT),
                            inventory::render_inventory(app, model, window, cx).into_any_element(),
                        ))
                        .child(compact_panel(
                            px(ROUTE_MAP_COMPACT_GRAPH_HEIGHT),
                            graph::render_graph(model, mode, window, cx).into_any_element(),
                        ))
                        .child(compact_panel(
                            px(ROUTE_MAP_COMPACT_INSPECTOR_HEIGHT),
                            inspector::render_inspector(app, model, cx).into_any_element(),
                        )),
                ),
        )
        .into_any_element()
}

fn render_compact_events_layout(
    app: &mut WgApp,
    model: &RouteMapData,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .w_full()
        .min_h(px(0.0))
        .overflow_hidden()
        .child(
            div()
                .w_full()
                .flex_1()
                .min_h(px(0.0))
                .overflow_y_scrollbar()
                .child(
                    v_flex()
                        .w_full()
                        .gap_3()
                        .p_3()
                        .child(compact_panel(
                            px(ROUTE_MAP_COMPACT_INVENTORY_HEIGHT),
                            inventory::render_inventory(app, model, window, cx).into_any_element(),
                        ))
                        .child(compact_panel(
                            px(ROUTE_MAP_COMPACT_EVENTS_HEIGHT),
                            events::render_events_workspace(model, window, cx).into_any_element(),
                        )),
                ),
        )
        .into_any_element()
}

fn compact_panel(height: Pixels, child: AnyElement) -> Div {
    div()
        .w_full()
        .h(height)
        .min_h(height)
        .child(panel_shell(child))
}

fn render_events_layout(
    app: &mut WgApp,
    model: &RouteMapData,
    inventory_width: f32,
    app_handle: Entity<WgApp>,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) -> AnyElement {
    h_resizable("route-map-layout-events")
        .on_resize(move |state: &Entity<ResizableState>, _window, cx| {
            let sizes = state.read(cx).sizes().clone();
            let Some(inventory_size) = sizes.first() else {
                return;
            };
            app_handle.update(cx, |app, cx| {
                let changed = app.persist_route_map_panel_widths(
                    inventory_size.as_f32(),
                    app.ui_prefs.route_map_inspector_width,
                    cx,
                );
                if changed {
                    cx.notify();
                }
            });
        })
        .child(
            resizable_panel()
                .size(px(inventory_width))
                .size_range(px(240.0)..px(360.0))
                .child(panel_shell(
                    inventory::render_inventory(app, model, window, cx).into_any_element(),
                )),
        )
        .child(
            resizable_panel()
                .size_range(px(560.0)..Pixels::MAX)
                .child(panel_shell(
                    events::render_events_workspace(model, window, cx).into_any_element(),
                )),
        )
        .into_any_element()
}

fn panel_shell(child: AnyElement) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .w_full()
        .h_full()
        .min_h(px(0.0))
        .p_3()
        .child(child)
}

fn handle_route_map_keydown(
    app: &mut WgApp,
    explain_match_id: Option<&SharedString>,
    selected_item_id: Option<&SharedString>,
    search_input: &Entity<gpui_component::input::InputState>,
    event: &KeyDownEvent,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) {
    if event.is_held {
        return;
    }

    let modifiers = event.keystroke.modifiers;
    if modifiers.control || modifiers.alt || modifiers.platform || modifiers.function {
        return;
    }

    let search_focused = search_input.read(cx).focus_handle(cx).is_focused(window);
    let key = event.keystroke.key.as_str();

    if key == "/" && !search_focused {
        search_input.update(cx, |input, cx| {
            input.focus(window, cx);
        });
        cx.stop_propagation();
        return;
    }

    if search_focused {
        return;
    }

    let next_mode = match key {
        "1" => Some(RouteMapMode::Flow),
        "2" => Some(RouteMapMode::Routes),
        "3" => Some(RouteMapMode::Explain),
        "4" => Some(RouteMapMode::Events),
        _ => None,
    };
    if let Some(next_mode) = next_mode {
        app.set_route_map_mode(next_mode, cx);
        cx.stop_propagation();
        return;
    }

    if matches!(key, "enter" | "return") {
        if let Some(matched_id) = explain_match_id {
            if selected_item_id != Some(matched_id) {
                app.set_route_map_selected_item(Some(matched_id.clone()), cx);
                cx.stop_propagation();
            }
        }
    }
}

fn render_header(model: &RouteMapData, cx: &mut Context<WgApp>) -> Div {
    let (context_chips, metric_chips): (Vec<_>, Vec<_>) = model
        .summary_chips
        .iter()
        .partition(|chip| !is_metric_chip(chip));

    div()
        .px_6()
        .py_4()
        .border_b_1()
        .border_color(cx.theme().border)
        .bg(linear_gradient(
            135.0,
            linear_color_stop(cx.theme().background.alpha(0.98), 0.0),
            linear_color_stop(cx.theme().muted.alpha(0.72), 1.0),
        ))
        .child(
            v_flex()
                .gap_3()
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
                                .child(
                                    h_flex()
                                        .items_center()
                                        .gap_2()
                                        .flex_wrap()
                                        .child(div().text_xl().font_semibold().child("Route Map"))
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
                                        ),
                                ),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .gap_2()
                                .flex_wrap()
                                .children(metric_chips.into_iter().map(summary_chip)),
                        ),
                )
                .child(
                    h_flex()
                        .items_start()
                        .justify_between()
                        .gap_3()
                        .flex_wrap()
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(model.plan_status.clone()),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .gap_2()
                                .flex_wrap()
                                .children(context_chips.into_iter().map(summary_chip)),
                        ),
                ),
        )
}

fn render_toolbar(
    app: &mut WgApp,
    search_input: &Entity<gpui_component::input::InputState>,
    cx: &mut Context<WgApp>,
) -> Div {
    let app_handle = cx.entity();
    let mode_group = TabBar::new("route-map-mode")
        .underline()
        .small()
        .selected_index(match app.ui_session.route_map_mode {
            RouteMapMode::Flow => 0,
            RouteMapMode::Routes => 1,
            RouteMapMode::Explain => 2,
            RouteMapMode::Events => 3,
        })
        .on_click(move |index, _window, app| {
            let next_mode = match *index {
                0 => RouteMapMode::Flow,
                1 => RouteMapMode::Routes,
                2 => RouteMapMode::Explain,
                3 => RouteMapMode::Events,
                _ => return,
            };

            app.update_entity(&app_handle, |this, cx| {
                this.set_route_map_mode(next_mode, cx);
            });
        })
        .child(Tab::new().label(RouteMapMode::Flow.label()).small())
        .child(Tab::new().label(RouteMapMode::Routes.label()).small())
        .child(Tab::new().label(RouteMapMode::Explain.label()).small())
        .child(Tab::new().label(RouteMapMode::Events.label()).small());

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
        .py_2()
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
                        .rounded_lg()
                        .bg(cx.theme().secondary.alpha(0.88))
                        .child(
                            Input::new(search_input)
                                .appearance(false)
                                .bordered(false)
                                .cleanable(true),
                        ),
                ),
        )
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
        .tooltip(family_tooltip(family))
        .selected(current == family)
        .on_click(cx.listener(move |this, _, _, cx| {
            this.set_route_map_family_filter(family, cx);
        }))
}

fn family_tooltip(family: RouteFamilyFilter) -> &'static str {
    match family {
        RouteFamilyFilter::All => "Show both IPv4 and IPv6 plan items.",
        RouteFamilyFilter::Ipv4 => "Restrict the route plan to IPv4 items.",
        RouteFamilyFilter::Ipv6 => "Restrict the route plan to IPv6 items.",
    }
}

pub(super) fn summary_chip(chip: &RouteMapChip) -> Tag {
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

pub(super) fn status_chip(status: RouteMapItemStatus) -> Tag {
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

fn is_metric_chip(chip: &RouteMapChip) -> bool {
    let label = chip.label.as_ref();
    label.starts_with("Guardrails ")
        || label.starts_with("Bypass ")
        || label.starts_with("Evidence ")
        || label.starts_with("Routes ")
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
