use gpui::{actions, App, Context, KeyBinding, Window};

use crate::ui::state::{SidebarItem, WgApp};

actions!(
    rwg,
    [
        OpenOverview,
        OpenConfigs,
        OpenProxies,
        OpenDns,
        OpenLogs,
        OpenRouteMap,
        OpenTools,
        OpenAdvanced,
        OpenAbout,
        ImportConfig,
        SaveConfig,
        ToggleTunnel,
    ]
);

pub(crate) fn install_keybindings(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-1", OpenOverview, None),
        KeyBinding::new("cmd-2", OpenConfigs, None),
        KeyBinding::new("cmd-,", OpenAdvanced, None),
        KeyBinding::new("cmd-o", ImportConfig, Some("Configs")),
        KeyBinding::new("cmd-s", SaveConfig, Some("Configs")),
    ]);
}

impl WgApp {
    pub(crate) fn toggle_tunnel_action(
        &mut self,
        _: &ToggleTunnel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.handle_start_stop(window, cx);
    }

    pub(crate) fn open_overview_action(
        &mut self,
        _: &OpenOverview,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_sidebar_active(SidebarItem::Overview, window, cx);
    }

    pub(crate) fn open_configs_action(
        &mut self,
        _: &OpenConfigs,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_sidebar_active(SidebarItem::Configs, window, cx);
    }

    pub(crate) fn open_advanced_action(
        &mut self,
        _: &OpenAdvanced,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_sidebar_active(SidebarItem::Advanced, window, cx);
    }

    pub(crate) fn open_proxies_action(
        &mut self,
        _: &OpenProxies,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_sidebar_active(SidebarItem::Proxies, window, cx);
    }

    pub(crate) fn open_dns_action(
        &mut self,
        _: &OpenDns,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_sidebar_active(SidebarItem::Dns, window, cx);
    }

    pub(crate) fn open_logs_action(
        &mut self,
        _: &OpenLogs,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_sidebar_active(SidebarItem::Logs, window, cx);
    }

    pub(crate) fn open_route_map_action(
        &mut self,
        _: &OpenRouteMap,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_sidebar_active(SidebarItem::RouteMap, window, cx);
    }

    pub(crate) fn open_tools_action(
        &mut self,
        _: &OpenTools,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_sidebar_active(SidebarItem::Tools, window, cx);
    }

    pub(crate) fn open_about_action(
        &mut self,
        _: &OpenAbout,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_sidebar_active(SidebarItem::About, window, cx);
    }

    pub(crate) fn import_config_action(
        &mut self,
        _: &ImportConfig,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.handle_import_click(window, cx);
    }

    pub(crate) fn save_config_action(
        &mut self,
        _: &SaveConfig,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.handle_save_click(window, cx);
    }
}
