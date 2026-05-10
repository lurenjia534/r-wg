use gpui::{Context, SharedString};
use gpui_component::theme::ThemeMode;
use r_wg::backend::wg::{DaitaMode, QuantumMode, WireGuardBackendPreference};
use r_wg::dns::{DnsMode, DnsPreset};

use crate::ui::features::themes::{self, AppearancePolicy};
use crate::ui::i18n::LanguagePreference;
use crate::ui::persistence::{PersistedState, StoragePaths};
use crate::ui::state::{ConfigInspectorTab, ProxiesViewMode, TrafficPeriod, UiPrefsState, WgApp};

pub(super) struct RestoredPrefs {
    appearance_policy: Option<AppearancePolicy>,
    resolved_theme_mode: Option<ThemeMode>,
    theme_light_key: Option<SharedString>,
    theme_dark_key: Option<SharedString>,
    theme_light_name: Option<SharedString>,
    theme_dark_name: Option<SharedString>,
    language_preference: Option<LanguagePreference>,
    log_viewer_enabled: Option<bool>,
    log_auto_follow: Option<bool>,
    require_connect_password: Option<bool>,
    kill_switch_enabled: Option<bool>,
    preferred_inspector_tab: Option<ConfigInspectorTab>,
    preferred_traffic_period: Option<TrafficPeriod>,
    configs_library_width: Option<f32>,
    configs_inspector_width: Option<f32>,
    route_map_inventory_width: Option<f32>,
    route_map_inspector_width: Option<f32>,
    proxies_view_mode: Option<ProxiesViewMode>,
    dns_mode: Option<DnsMode>,
    dns_preset: Option<DnsPreset>,
    quantum_mode: Option<QuantumMode>,
    daita_mode: Option<DaitaMode>,
    wireguard_backend_preference: Option<WireGuardBackendPreference>,
    pub(super) theme_notice: Option<SharedString>,
    pub(super) theme_prefs_migrated: bool,
}

pub(super) fn restore_prefs(
    state: &PersistedState,
    storage: &StoragePaths,
    cx: &Context<WgApp>,
) -> RestoredPrefs {
    let appearance_policy = state
        .theme_policy
        .or_else(|| state.theme_mode.map(Into::into));
    let light = themes::resolve_theme_preference(
        ThemeMode::Light,
        state.theme_light_key.as_deref(),
        state.theme_light_name.as_deref(),
        Some(storage),
        cx,
    );
    let dark = themes::resolve_theme_preference(
        ThemeMode::Dark,
        state.theme_dark_key.as_deref(),
        state.theme_dark_name.as_deref(),
        Some(storage),
        cx,
    );
    let mut notices = Vec::new();
    if let Some(notice) = light.notice.clone() {
        notices.push(notice);
    }
    if let Some(notice) = dark.notice.clone() {
        notices.push(notice);
    }
    let theme_notice = (!notices.is_empty()).then(|| notices.join(" • ").into());
    let theme_prefs_migrated = light.migrated
        || dark.migrated
        || state.theme_light_key.is_none()
        || state.theme_dark_key.is_none();

    RestoredPrefs {
        appearance_policy,
        resolved_theme_mode: state.theme_mode,
        theme_light_key: Some(light.entry.key.clone()),
        theme_dark_key: Some(dark.entry.key.clone()),
        theme_light_name: Some(light.entry.name.clone()),
        theme_dark_name: Some(dark.entry.name.clone()),
        language_preference: state.language_preference,
        log_viewer_enabled: state.log_viewer_enabled,
        log_auto_follow: state.log_auto_follow,
        require_connect_password: state.require_connect_password,
        kill_switch_enabled: state.kill_switch_enabled,
        preferred_inspector_tab: state.preferred_inspector_tab,
        preferred_traffic_period: state.preferred_traffic_period,
        configs_library_width: state.configs_library_width,
        configs_inspector_width: state.configs_inspector_width,
        route_map_inventory_width: state.route_map_inventory_width,
        route_map_inspector_width: state.route_map_inspector_width,
        proxies_view_mode: state.proxies_view_mode,
        dns_mode: state.dns_mode,
        dns_preset: state.dns_preset,
        quantum_mode: state.quantum_mode,
        daita_mode: state.daita_mode,
        wireguard_backend_preference: state.wireguard_backend_preference,
        theme_notice,
        theme_prefs_migrated,
    }
}

impl RestoredPrefs {
    pub(super) fn apply(self, ui_prefs: &mut UiPrefsState) {
        if let Some(appearance_policy) = self.appearance_policy {
            ui_prefs.appearance_policy = appearance_policy;
        }
        if let Some(resolved_theme_mode) = self.resolved_theme_mode {
            ui_prefs.resolved_theme_mode = resolved_theme_mode;
        }
        if let Some(theme_light_key) = self.theme_light_key {
            ui_prefs.theme_light_key = Some(theme_light_key);
        }
        if let Some(theme_dark_key) = self.theme_dark_key {
            ui_prefs.theme_dark_key = Some(theme_dark_key);
        }
        if let Some(theme_light_name) = self.theme_light_name {
            ui_prefs.theme_light_name = Some(theme_light_name);
        }
        if let Some(theme_dark_name) = self.theme_dark_name {
            ui_prefs.theme_dark_name = Some(theme_dark_name);
        }
        if let Some(language_preference) = self.language_preference {
            ui_prefs.language_preference = language_preference;
        }
        if let Some(log_viewer_enabled) = self.log_viewer_enabled {
            ui_prefs.log_viewer_enabled = log_viewer_enabled;
            r_wg::log::set_buffer_enabled(log_viewer_enabled);
        }
        if let Some(log_auto_follow) = self.log_auto_follow {
            ui_prefs.log_auto_follow = log_auto_follow;
        }
        if let Some(require_connect_password) = self.require_connect_password {
            ui_prefs.require_connect_password = require_connect_password;
        }
        if let Some(kill_switch_enabled) = self.kill_switch_enabled {
            ui_prefs.kill_switch_enabled = kill_switch_enabled;
        }
        if let Some(preferred_inspector_tab) = self.preferred_inspector_tab {
            ui_prefs.preferred_inspector_tab = preferred_inspector_tab;
        }
        if let Some(preferred_traffic_period) = self.preferred_traffic_period {
            ui_prefs.preferred_traffic_period = preferred_traffic_period;
        }
        if let Some(configs_library_width) = self.configs_library_width {
            ui_prefs.configs_library_width = configs_library_width.clamp(240.0, 420.0);
        }
        if let Some(configs_inspector_width) = self.configs_inspector_width {
            ui_prefs.configs_inspector_width = configs_inspector_width.clamp(280.0, 440.0);
        }
        if let Some(route_map_inventory_width) = self.route_map_inventory_width {
            ui_prefs.route_map_inventory_width = route_map_inventory_width.clamp(240.0, 360.0);
        }
        if let Some(route_map_inspector_width) = self.route_map_inspector_width {
            ui_prefs.route_map_inspector_width = route_map_inspector_width.clamp(280.0, 420.0);
        }
        if let Some(proxies_view_mode) = self.proxies_view_mode {
            ui_prefs.proxies_view_mode = proxies_view_mode;
        }
        if let Some(dns_mode) = self.dns_mode {
            ui_prefs.dns_mode = dns_mode;
        }
        if let Some(dns_preset) = self.dns_preset {
            ui_prefs.dns_preset = dns_preset;
        }
        if let Some(quantum_mode) = self.quantum_mode {
            ui_prefs.quantum_mode = quantum_mode;
        }
        if let Some(daita_mode) = self.daita_mode {
            ui_prefs.daita_mode = daita_mode;
        }
        if let Some(wireguard_backend_preference) = self.wireguard_backend_preference {
            ui_prefs.wireguard_backend_preference = wireguard_backend_preference;
        }
    }
}
