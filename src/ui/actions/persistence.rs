use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;

use gpui::SharedString;
use gpui::{AppContext, Context, Timer, Window};
use gpui_component::theme::ThemeMode;
use r_wg::dns::{DnsMode, DnsPreset};

use super::super::persistence::{
    self, PersistedConfig, PersistedConfigTrafficDay, PersistedConfigTrafficHour, PersistedSource,
    PersistedState, PersistedTrafficDayStats, PersistedTrafficHour, StoragePaths, STATE_VERSION,
};
use super::super::state::{
    build_configs_library_rows, ConfigSource, ConfigsState, EndpointFamily, SelectionState,
    StatsState, TrafficDay, TrafficDayStats, TrafficHour, TunnelConfig, UiPrefsState, WgApp,
    TRAFFIC_HOURLY_HISTORY, TRAFFIC_ROLLING_DAYS,
};
use super::super::themes::{self, AppearancePolicy};

impl WgApp {
    pub(crate) fn start_load_persisted_state(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.selection.begin_persistence_load() {
            return;
        }

        let storage = match self.configs.ensure_storage() {
            Ok(storage) => storage,
            Err(err) => {
                self.set_error(err);
                cx.notify();
                return;
            }
        };

        self.set_status("Loading configs...");
        cx.notify();

        let view = cx.weak_entity();
        window
            .spawn(cx, async move |cx| {
                let storage_for_task = storage.clone();
                let load_task =
                    cx.background_spawn(async move { persistence::load_state(&storage_for_task) });
                let result = load_task.await;
                view.update_in(cx, |this, window, cx| {
                    match result {
                        Ok(Some(state)) => match PersistedStateRestore::decode(state, &storage, cx)
                        {
                            Ok(restored) => {
                                let summary = restored.apply(
                                    &mut this.configs,
                                    &mut this.selection,
                                    &mut this.stats,
                                    &mut this.ui_prefs,
                                );
                                if let Some(workspace) = this.ui.configs_workspace.clone() {
                                    let rows = build_configs_library_rows(
                                        &this.configs,
                                        &this.runtime,
                                        &workspace.read(cx).draft,
                                    );
                                    let _ = workspace.update(cx, |workspace, cx| {
                                        if workspace.set_library_rows(rows) {
                                            cx.notify();
                                        }
                                    });
                                }
                                this.ui_session.sync_from_prefs(&this.ui_prefs);
                                this.apply_theme_prefs(Some(window), cx);
                                if let Some(selected_id) = summary.selected_id {
                                    this.load_config_into_inputs(selected_id, window, cx);
                                }
                                if let Some(theme_notice) = summary.theme_notice {
                                    this.push_success_toast(theme_notice, window, cx);
                                }
                                if summary.missing_files > 0 {
                                    this.set_status(format!(
                                        "Loaded {} configs, {} missing",
                                        summary.loaded_count, summary.missing_files
                                    ));
                                    this.persist_state_async(cx);
                                } else if summary.theme_prefs_migrated {
                                    if summary.loaded_count == 0 {
                                        this.set_status("Ready");
                                    } else {
                                        this.set_status(format!(
                                            "Loaded {} configs",
                                            summary.loaded_count
                                        ));
                                    }
                                    this.persist_state_async(cx);
                                } else if summary.loaded_count == 0 {
                                    this.set_status("Ready");
                                } else {
                                    this.set_status(format!(
                                        "Loaded {} configs",
                                        summary.loaded_count
                                    ));
                                }
                            }
                            Err(err) => {
                                this.set_error(err);
                            }
                        },
                        Ok(None) => {
                            this.set_status("Ready");
                        }
                        Err(err) => {
                            this.set_error(err);
                        }
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
    }

    pub(crate) fn persist_state_async(&mut self, cx: &mut Context<Self>) {
        // 单 writer + debounce：避免多次快速变更时旧快照后写覆盖新快照。
        let storage = match self.configs.ensure_storage() {
            Ok(storage) => storage,
            Err(err) => {
                self.set_error(err);
                cx.notify();
                return;
            }
        };
        self.persistence.enqueue();

        if self.persistence.worker_active {
            return;
        }
        self.persistence.worker_active = true;

        cx.spawn(async move |view, cx| loop {
            Timer::after(Duration::from_millis(200)).await;

            let Some(state) = view
                .update(cx, |this, _cx| {
                    match this.persistence.take_queued_revision() {
                        Some(_) => {}
                        None => {
                            this.persistence.worker_active = false;
                            return None;
                        }
                    }
                    Some(PersistedStateSnapshot::capture(this).build())
                })
                .ok()
                .flatten()
            else {
                break;
            };

            let save_task = cx.background_spawn({
                let storage = storage.clone();
                async move { persistence::save_state(&storage, &state) }
            });

            let result = save_task.await;

            let should_continue = view
                .update(cx, |this, cx| {
                    if let Err(err) = result {
                        this.set_error(err);
                        cx.notify();
                    }

                    if this.persistence.has_pending() {
                        true
                    } else {
                        this.persistence.worker_active = false;
                        false
                    }
                })
                .unwrap_or(false);

            if !should_continue {
                break;
            }
        })
        .detach();
    }
}

struct PersistedStateSnapshot<'a> {
    configs: &'a ConfigsState,
    selection: &'a SelectionState,
    stats: &'a StatsState,
    ui_prefs: &'a UiPrefsState,
}

impl<'a> PersistedStateSnapshot<'a> {
    fn capture(app: &'a WgApp) -> Self {
        Self {
            configs: &app.configs,
            selection: &app.selection,
            stats: &app.stats,
            ui_prefs: &app.ui_prefs,
        }
    }

    fn build(&self) -> PersistedState {
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
            log_auto_follow: Some(self.ui_prefs.log_auto_follow),
            preferred_inspector_tab: Some(self.ui_prefs.preferred_inspector_tab),
            preferred_traffic_period: Some(self.ui_prefs.preferred_traffic_period),
            configs_library_width: Some(self.ui_prefs.configs_library_width),
            configs_inspector_width: Some(self.ui_prefs.configs_inspector_width),
            route_map_inventory_width: Some(self.ui_prefs.route_map_inventory_width),
            route_map_inspector_width: Some(self.ui_prefs.route_map_inspector_width),
            proxies_view_mode: Some(self.ui_prefs.proxies_view_mode),
            dns_mode: Some(self.ui_prefs.dns_mode),
            dns_preset: Some(self.ui_prefs.dns_preset),
            traffic_days: self
                .stats
                .traffic_days
                .clone()
                .into_iter()
                .map(Into::into)
                .collect(),
            traffic_days_v2: self
                .stats
                .traffic_days_v2
                .iter()
                .map(|day| PersistedTrafficDayStats {
                    date: day.date.clone(),
                    rx_bytes: day.rx_bytes,
                    tx_bytes: day.tx_bytes,
                })
                .collect(),
            traffic_hours: self
                .stats
                .traffic_hours
                .iter()
                .map(|hour| PersistedTrafficHour {
                    hour: hour.hour,
                    rx_bytes: hour.rx_bytes,
                    tx_bytes: hour.tx_bytes,
                })
                .collect(),
            config_traffic_days: self
                .stats
                .config_traffic_days
                .iter()
                .flat_map(|(config_id, days)| {
                    days.iter().map(|day| PersistedConfigTrafficDay {
                        config_id: *config_id,
                        date: day.date.clone(),
                        rx_bytes: day.rx_bytes,
                        tx_bytes: day.tx_bytes,
                    })
                })
                .collect(),
            config_traffic_hours: self
                .stats
                .config_traffic_hours
                .iter()
                .flat_map(|(config_id, hours)| {
                    hours.iter().map(|hour| PersistedConfigTrafficHour {
                        config_id: *config_id,
                        hour: hour.hour,
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

struct PersistedStateRestore {
    appearance_policy: Option<AppearancePolicy>,
    resolved_theme_mode: Option<ThemeMode>,
    theme_light_key: Option<SharedString>,
    theme_dark_key: Option<SharedString>,
    theme_light_name: Option<SharedString>,
    theme_dark_name: Option<SharedString>,
    log_auto_follow: Option<bool>,
    preferred_inspector_tab: Option<super::super::state::ConfigInspectorTab>,
    preferred_traffic_period: Option<super::super::state::TrafficPeriod>,
    configs_library_width: Option<f32>,
    configs_inspector_width: Option<f32>,
    route_map_inventory_width: Option<f32>,
    route_map_inspector_width: Option<f32>,
    proxies_view_mode: Option<super::super::state::ProxiesViewMode>,
    dns_mode: Option<DnsMode>,
    dns_preset: Option<DnsPreset>,
    configs: Vec<TunnelConfig>,
    next_config_id: u64,
    selected_id: Option<u64>,
    traffic_days: Vec<TrafficDay>,
    traffic_days_v2: Vec<TrafficDayStats>,
    traffic_hours: Vec<TrafficHour>,
    config_traffic_days: HashMap<u64, Vec<TrafficDayStats>>,
    config_traffic_hours: HashMap<u64, Vec<TrafficHour>>,
    missing_files: usize,
    theme_notice: Option<SharedString>,
    theme_prefs_migrated: bool,
}

struct PersistedStateSummary {
    selected_id: Option<u64>,
    loaded_count: usize,
    missing_files: usize,
    theme_notice: Option<SharedString>,
    theme_prefs_migrated: bool,
}

impl PersistedStateRestore {
    fn decode(
        state: PersistedState,
        storage: &StoragePaths,
        cx: &Context<WgApp>,
    ) -> Result<Self, String> {
        if state.version != STATE_VERSION {
            return Err(format!("Unsupported state version: {}", state.version));
        }

        let mut configs = Vec::new();
        let mut max_id = 0u64;
        let mut missing_files = 0usize;
        for entry in state.configs {
            if entry.id == 0 || entry.name.trim().is_empty() {
                continue;
            }
            max_id = max_id.max(entry.id);
            let storage_path = persistence::config_path(storage, entry.id);
            if !storage_path.exists() {
                missing_files += 1;
                continue;
            }
            let source = ConfigSource::from(entry.source);
            configs.push(TunnelConfig {
                id: entry.id,
                name: entry.name.clone(),
                name_lower: entry.name.to_lowercase(),
                text: None,
                source,
                storage_path,
                endpoint_family: EndpointFamily::Unknown,
            });
        }

        let mut traffic_days = BTreeMap::<String, u64>::new();
        for day in state.traffic_days {
            let entry = traffic_days.entry(day.date).or_insert(0);
            *entry = entry.saturating_add(day.bytes);
        }
        let traffic_days = traffic_days
            .into_iter()
            .map(|(date, bytes)| TrafficDay { date, bytes })
            .collect();

        let config_ids: HashSet<u64> = configs.iter().map(|cfg| cfg.id).collect();
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

        Ok(Self {
            appearance_policy,
            resolved_theme_mode: state.theme_mode,
            theme_light_key: Some(light.entry.key.clone()),
            theme_dark_key: Some(dark.entry.key.clone()),
            theme_light_name: Some(light.entry.name.clone()),
            theme_dark_name: Some(dark.entry.name.clone()),
            log_auto_follow: state.log_auto_follow,
            preferred_inspector_tab: state.preferred_inspector_tab,
            preferred_traffic_period: state.preferred_traffic_period,
            configs_library_width: state.configs_library_width,
            configs_inspector_width: state.configs_inspector_width,
            route_map_inventory_width: state.route_map_inventory_width,
            route_map_inspector_width: state.route_map_inspector_width,
            proxies_view_mode: state.proxies_view_mode,
            dns_mode: state.dns_mode,
            dns_preset: state.dns_preset,
            next_config_id: state.next_id.max(max_id.saturating_add(1)),
            selected_id: state.selected_id,
            traffic_days,
            traffic_days_v2: merge_day_stats(state.traffic_days_v2),
            traffic_hours: merge_hour_stats(state.traffic_hours),
            config_traffic_days: merge_config_day_stats(state.config_traffic_days, &config_ids),
            config_traffic_hours: merge_config_hour_stats(state.config_traffic_hours, &config_ids),
            configs,
            missing_files,
            theme_notice,
            theme_prefs_migrated,
        })
    }

    fn apply(
        self,
        configs: &mut ConfigsState,
        selection: &mut SelectionState,
        stats: &mut StatsState,
        ui_prefs: &mut UiPrefsState,
    ) -> PersistedStateSummary {
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
        if let Some(log_auto_follow) = self.log_auto_follow {
            ui_prefs.log_auto_follow = log_auto_follow;
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

        configs.configs = self.configs;
        configs.next_config_id = self.next_config_id;

        stats.traffic_days = self.traffic_days;
        stats.traffic_days_v2 = self.traffic_days_v2;
        stats.traffic_hours = self.traffic_hours;
        stats.config_traffic_days = self.config_traffic_days;
        stats.config_traffic_hours = self.config_traffic_hours;
        stats.traffic_dirty = false;
        stats.traffic_last_persist_at = None;

        selection.restore_after_persist(self.selected_id, configs);

        PersistedStateSummary {
            selected_id: selection.selected_id,
            loaded_count: configs.len(),
            missing_files: self.missing_files,
            theme_notice: self.theme_notice,
            theme_prefs_migrated: self.theme_prefs_migrated,
        }
    }
}

fn merge_day_stats(items: Vec<PersistedTrafficDayStats>) -> Vec<TrafficDayStats> {
    let mut map = BTreeMap::<String, (u64, u64)>::new();
    for day in items {
        let entry = map.entry(day.date).or_insert((0, 0));
        entry.0 = entry.0.saturating_add(day.rx_bytes);
        entry.1 = entry.1.saturating_add(day.tx_bytes);
    }
    let mut days = map
        .into_iter()
        .map(|(date, (rx_bytes, tx_bytes))| TrafficDayStats {
            date,
            rx_bytes,
            tx_bytes,
        })
        .collect::<Vec<_>>();
    prune_day_stats(&mut days);
    days
}

fn merge_hour_stats(items: Vec<PersistedTrafficHour>) -> Vec<TrafficHour> {
    let mut map = BTreeMap::<i64, (u64, u64)>::new();
    for hour in items {
        let entry = map.entry(hour.hour).or_insert((0, 0));
        entry.0 = entry.0.saturating_add(hour.rx_bytes);
        entry.1 = entry.1.saturating_add(hour.tx_bytes);
    }
    let mut hours = map
        .into_iter()
        .map(|(hour, (rx_bytes, tx_bytes))| TrafficHour {
            hour,
            rx_bytes,
            tx_bytes,
        })
        .collect::<Vec<_>>();
    prune_hour_stats(&mut hours);
    hours
}

fn merge_config_day_stats(
    items: Vec<PersistedConfigTrafficDay>,
    config_ids: &HashSet<u64>,
) -> HashMap<u64, Vec<TrafficDayStats>> {
    let mut map = HashMap::<u64, BTreeMap<String, (u64, u64)>>::new();
    for day in items {
        if !config_ids.contains(&day.config_id) {
            continue;
        }
        let entry = map
            .entry(day.config_id)
            .or_insert_with(BTreeMap::new)
            .entry(day.date)
            .or_insert((0, 0));
        entry.0 = entry.0.saturating_add(day.rx_bytes);
        entry.1 = entry.1.saturating_add(day.tx_bytes);
    }
    let mut out = HashMap::new();
    for (config_id, days) in map {
        let mut days = days
            .into_iter()
            .map(|(date, (rx_bytes, tx_bytes))| TrafficDayStats {
                date,
                rx_bytes,
                tx_bytes,
            })
            .collect::<Vec<_>>();
        prune_day_stats(&mut days);
        out.insert(config_id, days);
    }
    out
}

fn merge_config_hour_stats(
    items: Vec<PersistedConfigTrafficHour>,
    config_ids: &HashSet<u64>,
) -> HashMap<u64, Vec<TrafficHour>> {
    let mut map = HashMap::<u64, BTreeMap<i64, (u64, u64)>>::new();
    for hour in items {
        if !config_ids.contains(&hour.config_id) {
            continue;
        }
        let entry = map
            .entry(hour.config_id)
            .or_insert_with(BTreeMap::new)
            .entry(hour.hour)
            .or_insert((0, 0));
        entry.0 = entry.0.saturating_add(hour.rx_bytes);
        entry.1 = entry.1.saturating_add(hour.tx_bytes);
    }
    let mut out = HashMap::new();
    for (config_id, hours) in map {
        let mut hours = hours
            .into_iter()
            .map(|(hour, (rx_bytes, tx_bytes))| TrafficHour {
                hour,
                rx_bytes,
                tx_bytes,
            })
            .collect::<Vec<_>>();
        prune_hour_stats(&mut hours);
        out.insert(config_id, hours);
    }
    out
}

fn prune_day_stats(days: &mut Vec<TrafficDayStats>) {
    days.sort_by(|a, b| a.date.cmp(&b.date));
    if days.len() > TRAFFIC_ROLLING_DAYS {
        let remove_count = days.len() - TRAFFIC_ROLLING_DAYS;
        days.drain(0..remove_count);
    }
}

fn prune_hour_stats(hours: &mut Vec<TrafficHour>) {
    hours.sort_by(|a, b| a.hour.cmp(&b.hour));
    if hours.len() > TRAFFIC_HOURLY_HISTORY {
        let remove_count = hours.len() - TRAFFIC_HOURLY_HISTORY;
        hours.drain(0..remove_count);
    }
}
