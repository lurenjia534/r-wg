use std::time::Duration;

use gpui::SharedString;
use gpui::{AppContext, Context, Window};
use gpui_component::input::InputState;

use crate::ui::state::{RouteFamilyFilter, RouteMapMode, WgApp};

pub(crate) fn ensure_route_map_search_input(
    app: &mut WgApp,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) {
    if app.ui.route_map_search_input.is_some() {
        return;
    }

    let input =
        cx.new(|cx| InputState::new(window, cx).placeholder("Search IP, CIDR, or endpoint host"));
    app.ui.route_map_search_input = Some(input);
}

impl WgApp {
    pub(crate) fn set_route_map_mode(&mut self, value: RouteMapMode, cx: &mut gpui::Context<Self>) {
        if self.ui_session.route_map_mode != value {
            self.ui_session.route_map_mode = value;
            cx.notify();
        }
    }

    pub(crate) fn set_route_map_family_filter(
        &mut self,
        value: RouteFamilyFilter,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.ui_session.route_map_family_filter != value {
            self.ui_session.route_map_family_filter = value;
            cx.notify();
        }
    }

    pub(crate) fn set_route_map_selected_item(
        &mut self,
        value: Option<SharedString>,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.ui_session.route_map_selected_item != value {
            self.ui_session.route_map_selected_item = value;
            cx.notify();
        }
    }

    pub(crate) fn set_route_map_glossary_open(
        &mut self,
        value: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.ui_session.route_map_glossary_open != value {
            self.ui_session.route_map_glossary_open = value;
            cx.notify();
        }
    }

    pub(crate) fn sync_route_map_search_query(
        &mut self,
        value: impl Into<SharedString>,
        cx: &mut gpui::Context<Self>,
    ) {
        let value = value.into();
        if self.ui.route_map_search.raw_query == value {
            return;
        }
        self.ui.route_map_search.raw_query = value;
        self.ui.route_map_search.enqueue();
        if self.ui.route_map_search.worker_active {
            return;
        }
        self.ui.route_map_search.worker_active = true;

        cx.spawn(async move |view, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(150))
                .await;

            let Some(query) = view
                .update(cx, |this, _| {
                    match this.ui.route_map_search.take_queued_revision() {
                        Some(_) => Some(this.ui.route_map_search.raw_query.clone()),
                        None => {
                            this.ui.route_map_search.worker_active = false;
                            None
                        }
                    }
                })
                .ok()
                .flatten()
            else {
                break;
            };

            let _ = view.update(cx, |this, cx| {
                if this.ui.route_map_search.debounced_query != query {
                    this.ui.route_map_search.debounced_query = query;
                    cx.notify();
                }
            });
        })
        .detach();
    }
}
