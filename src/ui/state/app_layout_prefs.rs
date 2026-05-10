use super::{ConfigInspectorTab, ProxiesViewMode, TrafficPeriod, WgApp};

impl WgApp {
    pub(crate) fn current_configs_inspector_tab(
        &self,
        cx: &mut gpui::Context<Self>,
    ) -> ConfigInspectorTab {
        self.ui
            .configs_workspace
            .as_ref()
            .map(|workspace| workspace.read(cx).inspector_tab)
            .unwrap_or(self.ui_prefs.preferred_inspector_tab)
    }

    pub(crate) fn persist_preferred_inspector_tab(
        &mut self,
        value: ConfigInspectorTab,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        if self.ui_prefs.preferred_inspector_tab == value {
            return false;
        }
        self.ui_prefs.preferred_inspector_tab = value;
        self.persist_state_async(cx);
        true
    }

    pub(crate) fn persist_configs_panel_widths(
        &mut self,
        library_width: f32,
        inspector_width: f32,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        let library_width = library_width.clamp(240.0, 420.0);
        let inspector_width = inspector_width.clamp(280.0, 440.0);
        if self.ui_prefs.configs_library_width == library_width
            && self.ui_prefs.configs_inspector_width == inspector_width
        {
            return false;
        }
        self.ui_prefs.configs_library_width = library_width;
        self.ui_prefs.configs_inspector_width = inspector_width;
        self.persist_state_async(cx);
        true
    }

    pub(crate) fn persist_route_map_panel_widths(
        &mut self,
        inventory_width: f32,
        inspector_width: f32,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        let inventory_width = inventory_width.clamp(240.0, 360.0);
        let inspector_width = inspector_width.clamp(280.0, 420.0);
        if self.ui_prefs.route_map_inventory_width == inventory_width
            && self.ui_prefs.route_map_inspector_width == inspector_width
        {
            return false;
        }
        self.ui_prefs.route_map_inventory_width = inventory_width;
        self.ui_prefs.route_map_inspector_width = inspector_width;
        self.persist_state_async(cx);
        true
    }

    pub(crate) fn set_preferred_inspector_tab(
        &mut self,
        value: ConfigInspectorTab,
        cx: &mut gpui::Context<Self>,
    ) {
        self.persist_preferred_inspector_tab(value, cx);
        if let Some(workspace) = self.ui.configs_workspace.clone() {
            workspace.update(cx, |workspace, cx| {
                if workspace.set_inspector_tab(value) {
                    cx.notify();
                }
            });
        }
        cx.notify();
    }

    pub(crate) fn set_preferred_traffic_period(
        &mut self,
        value: TrafficPeriod,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.ui_prefs.preferred_traffic_period != value {
            self.ui_prefs.preferred_traffic_period = value;
            self.persist_state_async(cx);
        }
        self.ui_session.traffic_period = value;
        cx.notify();
    }

    pub(crate) fn set_session_traffic_period(
        &mut self,
        value: TrafficPeriod,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.ui_session.traffic_period != value {
            self.ui_session.traffic_period = value;
            cx.notify();
        }
    }

    pub(crate) fn set_proxies_view_mode_pref(
        &mut self,
        value: ProxiesViewMode,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.ui_prefs.proxies_view_mode != value {
            self.ui_prefs.proxies_view_mode = value;
            self.persist_state_async(cx);
        }
        cx.notify();
    }
}
