use crate::ui::persistence::{
    PersistedConfig, PersistedConfigTrafficDayBucket, PersistedConfigTrafficHourBucket,
    PersistedSource, PersistedState, PersistedTrafficDayBucket, PersistedTrafficHourBucket,
    STATE_VERSION,
};
use crate::ui::state::{ConfigsState, SelectionState, StatsState, UiPrefsState, WgApp};

pub(super) struct PersistedStateSnapshot<'a> {
    configs: &'a ConfigsState,
    selection: &'a SelectionState,
    stats: &'a StatsState,
    ui_prefs: &'a UiPrefsState,
}

impl<'a> PersistedStateSnapshot<'a> {
    pub(super) fn capture(app: &'a WgApp) -> Self {
        Self {
            configs: &app.configs,
            selection: &app.selection,
            stats: &app.stats,
            ui_prefs: &app.ui_prefs,
        }
    }

    pub(super) fn build(&self) -> PersistedState {
        let selected_id = self.selection.selected_id;

        PersistedState {
            version: STATE_VERSION,
            next_id: self.configs.next_config_id,
            selected_id,
            theme_mode: Some(self.ui_prefs.resolved_theme_mode),
            theme_policy: Some(self.ui_prefs.appearance_policy),
            theme_light_key: self
                .ui_prefs
                .theme_light_key
                .as_ref()
                .map(ToString::to_string),
            theme_dark_key: self
                .ui_prefs
                .theme_dark_key
                .as_ref()
                .map(ToString::to_string),
            theme_light_name: self
                .ui_prefs
                .theme_light_name
                .as_ref()
                .map(ToString::to_string),
            theme_dark_name: self
                .ui_prefs
                .theme_dark_name
                .as_ref()
                .map(ToString::to_string),
            language_preference: Some(self.ui_prefs.language_preference),
            log_viewer_enabled: Some(self.ui_prefs.log_viewer_enabled),
            log_auto_follow: Some(self.ui_prefs.log_auto_follow),
            require_connect_password: Some(self.ui_prefs.require_connect_password),
            kill_switch_enabled: Some(self.ui_prefs.kill_switch_enabled),
            preferred_inspector_tab: Some(self.ui_prefs.preferred_inspector_tab),
            preferred_traffic_period: Some(self.ui_prefs.preferred_traffic_period),
            configs_library_width: Some(self.ui_prefs.configs_library_width),
            configs_inspector_width: Some(self.ui_prefs.configs_inspector_width),
            route_map_inventory_width: Some(self.ui_prefs.route_map_inventory_width),
            route_map_inspector_width: Some(self.ui_prefs.route_map_inspector_width),
            proxies_view_mode: Some(self.ui_prefs.proxies_view_mode),
            dns_mode: Some(self.ui_prefs.dns_mode),
            dns_preset: Some(self.ui_prefs.dns_preset),
            quantum_mode: Some(self.ui_prefs.quantum_mode),
            daita_mode: Some(self.ui_prefs.daita_mode),
            wireguard_backend_preference: Some(self.ui_prefs.wireguard_backend_preference),
            traffic_global_days: self
                .stats
                .traffic
                .global_days
                .iter()
                .map(|day| PersistedTrafficDayBucket {
                    day_key: day.day_key,
                    rx_bytes: day.rx_bytes,
                    tx_bytes: day.tx_bytes,
                })
                .collect(),
            traffic_global_hours: self
                .stats
                .traffic
                .global_hours
                .iter()
                .map(|hour| PersistedTrafficHourBucket {
                    hour_key: hour.hour_key,
                    rx_bytes: hour.rx_bytes,
                    tx_bytes: hour.tx_bytes,
                })
                .collect(),
            traffic_config_days: self
                .stats
                .traffic
                .config_days
                .iter()
                .flat_map(|(config_id, days)| {
                    days.iter().map(|day| PersistedConfigTrafficDayBucket {
                        config_id: *config_id,
                        day_key: day.day_key,
                        rx_bytes: day.rx_bytes,
                        tx_bytes: day.tx_bytes,
                    })
                })
                .collect(),
            traffic_config_hours: self
                .stats
                .traffic
                .config_hours
                .iter()
                .flat_map(|(config_id, hours)| {
                    hours.iter().map(|hour| PersistedConfigTrafficHourBucket {
                        config_id: *config_id,
                        hour_key: hour.hour_key,
                        rx_bytes: hour.rx_bytes,
                        tx_bytes: hour.tx_bytes,
                    })
                })
                .collect(),
            configs: self
                .configs
                .iter()
                .map(|cfg| PersistedConfig {
                    id: cfg.id,
                    name: cfg.name.clone(),
                    source: PersistedSource::from(&cfg.source),
                })
                .collect(),
        }
    }
}
