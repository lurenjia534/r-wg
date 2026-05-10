use gpui::SharedString;
use gpui_component::theme::ThemeMode;
use r_wg::backend::wg::{DaitaMode, QuantumMode, WireGuardBackendPreference};
use r_wg::dns::{DnsMode, DnsPreset};

use crate::ui::features::themes::AppearancePolicy;
use crate::ui::i18n::LanguagePreference;

use super::{
    ConfigInspectorTab, ProxiesViewMode, RouteFamilyFilter, RouteMapMode, SidebarItem,
    TrafficPeriod, DEFAULT_CONFIGS_INSPECTOR_WIDTH, DEFAULT_CONFIGS_LIBRARY_WIDTH,
    DEFAULT_ROUTE_MAP_INSPECTOR_WIDTH, DEFAULT_ROUTE_MAP_INVENTORY_WIDTH,
};

pub(crate) struct UiPrefsState {
    pub(crate) log_viewer_enabled: bool,
    pub(crate) log_auto_follow: bool,
    pub(crate) require_connect_password: bool,
    pub(crate) kill_switch_enabled: bool,
    pub(crate) preferred_inspector_tab: ConfigInspectorTab,
    pub(crate) preferred_traffic_period: TrafficPeriod,
    pub(crate) configs_library_width: f32,
    pub(crate) configs_inspector_width: f32,
    pub(crate) route_map_inventory_width: f32,
    pub(crate) route_map_inspector_width: f32,
    pub(crate) proxies_view_mode: ProxiesViewMode,
    pub(crate) appearance_policy: AppearancePolicy,
    pub(crate) resolved_theme_mode: ThemeMode,
    pub(crate) theme_light_key: Option<SharedString>,
    pub(crate) theme_dark_key: Option<SharedString>,
    pub(crate) theme_light_name: Option<SharedString>,
    pub(crate) theme_dark_name: Option<SharedString>,
    pub(crate) language_preference: LanguagePreference,
    pub(crate) dns_mode: DnsMode,
    pub(crate) dns_preset: DnsPreset,
    pub(crate) quantum_mode: QuantumMode,
    pub(crate) daita_mode: DaitaMode,
    pub(crate) wireguard_backend_preference: WireGuardBackendPreference,
}

impl UiPrefsState {
    pub(super) fn new(
        appearance_policy: AppearancePolicy,
        resolved_theme_mode: ThemeMode,
        theme_light_key: Option<SharedString>,
        theme_dark_key: Option<SharedString>,
        theme_light_name: Option<SharedString>,
        theme_dark_name: Option<SharedString>,
        language_preference: LanguagePreference,
    ) -> Self {
        Self {
            log_viewer_enabled: true,
            log_auto_follow: true,
            require_connect_password: false,
            kill_switch_enabled: true,
            preferred_inspector_tab: ConfigInspectorTab::Preview,
            preferred_traffic_period: TrafficPeriod::Today,
            configs_library_width: DEFAULT_CONFIGS_LIBRARY_WIDTH,
            configs_inspector_width: DEFAULT_CONFIGS_INSPECTOR_WIDTH,
            route_map_inventory_width: DEFAULT_ROUTE_MAP_INVENTORY_WIDTH,
            route_map_inspector_width: DEFAULT_ROUTE_MAP_INSPECTOR_WIDTH,
            proxies_view_mode: ProxiesViewMode::List,
            appearance_policy,
            resolved_theme_mode,
            theme_light_key,
            theme_dark_key,
            theme_light_name,
            theme_dark_name,
            language_preference,
            dns_mode: DnsMode::FollowConfig,
            dns_preset: DnsPreset::CloudflareStandard,
            quantum_mode: QuantumMode::Off,
            daita_mode: DaitaMode::Off,
            wireguard_backend_preference: WireGuardBackendPreference::default(),
        }
    }

    pub(crate) fn theme_palette_name(&self, mode: ThemeMode) -> Option<&SharedString> {
        match mode {
            ThemeMode::Light => self.theme_light_name.as_ref(),
            ThemeMode::Dark => self.theme_dark_name.as_ref(),
        }
    }

    pub(crate) fn theme_palette_key(&self, mode: ThemeMode) -> Option<&SharedString> {
        match mode {
            ThemeMode::Light => self.theme_light_key.as_ref(),
            ThemeMode::Dark => self.theme_dark_key.as_ref(),
        }
    }
}

pub(crate) struct UiSessionState {
    pub(crate) traffic_period: TrafficPeriod,
    pub(crate) sidebar_active: SidebarItem,
    pub(crate) sidebar_collapsed: bool,
    pub(crate) sidebar_overlay_open: bool,
    pub(crate) show_alternate_theme_preview: bool,
    pub(crate) route_map_mode: RouteMapMode,
    pub(crate) route_map_family_filter: RouteFamilyFilter,
    pub(crate) route_map_selected_item: Option<SharedString>,
    pub(crate) route_map_glossary_open: bool,
}

impl UiSessionState {
    pub(super) fn from_prefs(prefs: &UiPrefsState) -> Self {
        Self {
            traffic_period: prefs.preferred_traffic_period,
            sidebar_active: SidebarItem::Overview,
            sidebar_collapsed: false,
            sidebar_overlay_open: false,
            show_alternate_theme_preview: false,
            route_map_mode: RouteMapMode::Flow,
            route_map_family_filter: RouteFamilyFilter::All,
            route_map_selected_item: None,
            route_map_glossary_open: false,
        }
    }

    pub(crate) fn sync_from_prefs(&mut self, prefs: &UiPrefsState) {
        self.traffic_period = prefs.preferred_traffic_period;
    }
}
