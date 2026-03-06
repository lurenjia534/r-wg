use std::collections::{BTreeMap, HashMap, HashSet};

use gpui::{AppContext, Context, Window};
use gpui_component::theme::Theme;

use super::super::persistence::{
    self, PersistedConfig, PersistedConfigTrafficDay, PersistedConfigTrafficHour, PersistedSource,
    PersistedState, PersistedTrafficDayStats, PersistedTrafficHour, StoragePaths, STATE_VERSION,
};
use super::super::state::{
    ConfigSource, TrafficDay, TrafficDayStats, TrafficHour, TunnelConfig, WgApp,
    TRAFFIC_HOURLY_HISTORY, TRAFFIC_ROLLING_DAYS,
};

impl WgApp {
    pub(crate) fn ensure_storage(&mut self) -> Result<StoragePaths, String> {
        if let Some(storage) = &self.configs.storage {
            return Ok(storage.clone());
        }
        let storage = persistence::ensure_storage_dirs()?;
        self.configs.storage = Some(storage.clone());
        Ok(storage)
    }

    pub(crate) fn alloc_config_id(&mut self) -> u64 {
        let id = self.configs.next_config_id.max(1);
        self.configs.next_config_id = id.saturating_add(1);
        id
    }

    pub(crate) fn start_load_persisted_state(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selection.persistence_loaded {
            return;
        }
        self.selection.persistence_loaded = true;

        let storage = match self.ensure_storage() {
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
                        Ok(Some(state)) => {
                            this.apply_persisted_state(state, &storage, window, cx);
                        }
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
        // 异步写盘：避免阻塞 UI；失败只提示错误，不影响当前交互。
        let storage = match self.ensure_storage() {
            Ok(storage) => storage,
            Err(err) => {
                self.set_error(err);
                cx.notify();
                return;
            }
        };
        let state = self.build_persisted_state();

        cx.spawn(async move |view, cx| {
            let save_task =
                cx.background_spawn(async move { persistence::save_state(&storage, &state) });
            if let Err(err) = save_task.await {
                view.update(cx, |this, cx| {
                    this.set_error(err);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    fn apply_persisted_state(
        &mut self,
        state: PersistedState,
        storage: &StoragePaths,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if state.version != STATE_VERSION {
            self.set_error(format!("Unsupported state version: {}", state.version));
            return;
        }

        // 尽早应用主题，避免启动时闪烁。
        if let Some(theme_mode) = state.theme_mode {
            self.ui_prefs.theme_mode = theme_mode;
            Theme::change(theme_mode, Some(window), cx);
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
                // 清理逻辑说明：
                // - 元数据里存在条目，但内部 configs/<id>.conf 丢失，说明用户手动删了文件
                //   或者上一次写盘失败导致文件缺失。
                // - 这种情况下继续保留条目会导致启动/编辑/运行时反复报错，
                //   用户看见但无法使用，体验更差。
                // - 因此我们在加载阶段直接跳过该条目，并在后续保存时把 state.json
                //   里的“悬空记录”清理掉，让元数据与磁盘一致。
                // - missing_files 计数用于状态提示，便于用户意识到有条目被清理。
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
            });
        }

        self.configs.configs = configs;
        // 合并同一天的流量记录，并按日期排序，避免重复日期导致统计偏差。
        let mut traffic_days = BTreeMap::<String, u64>::new();
        for day in state.traffic_days {
            let entry = traffic_days.entry(day.date).or_insert(0);
            *entry = entry.saturating_add(day.bytes);
        }
        self.stats.traffic_days = traffic_days
            .into_iter()
            .map(|(date, bytes)| TrafficDay { date, bytes })
            .collect();
        // 新版（按天/按小时）流量统计。
        self.stats.traffic_days_v2 = merge_day_stats(state.traffic_days_v2);
        self.stats.traffic_hours = merge_hour_stats(state.traffic_hours);
        let config_ids: HashSet<u64> = self.configs.iter().map(|cfg| cfg.id).collect();
        self.stats.config_traffic_days =
            merge_config_day_stats(state.config_traffic_days, &config_ids);
        self.stats.config_traffic_hours =
            merge_config_hour_stats(state.config_traffic_hours, &config_ids);
        // 刚加载的数据视为“干净”，只有新流量产生时才标记 dirty。
        self.stats.traffic_dirty = false;
        self.stats.traffic_last_persist_at = None;
        self.selection.selected = state
            .selected_id
            .and_then(|id| self.configs.iter().position(|cfg| cfg.id == id));
        self.configs.next_config_id = state.next_id.max(max_id.saturating_add(1));
        self.selection.proxy_filter_total = 0;
        self.selection.parse_cache = None;
        self.selection.loaded_config = None;
        self.selection.loading_config = None;
        self.selection.loading_config_path = None;

        if let Some(idx) = self.selection.selected {
            self.load_config_into_inputs(idx, window, cx);
        }
        if missing_files > 0 {
            // 如果发现缺失文件，马上落盘一次：
            // - 这会把刚才跳过的条目从 state.json 中移除；
            // - 下次启动不会再遇到同样的“幽灵配置”；
            // - 这属于修复性写入，不改变其他配置内容。
            self.set_status(format!(
                "Loaded {} configs, {} missing",
                self.configs.len(),
                missing_files
            ));
            self.persist_state_async(cx);
        } else if self.configs.is_empty() {
            self.set_status("Ready");
        } else {
            self.set_status(format!("Loaded {} configs", self.configs.len()));
        }
    }

    fn build_persisted_state(&self) -> PersistedState {
        let selected_id = self
            .selection
            .selected
            .and_then(|idx| self.configs.get(idx))
            .map(|cfg| cfg.id);
        PersistedState {
            version: STATE_VERSION,
            next_id: self.configs.next_config_id,
            selected_id,
            // 保存当前主题，便于下次启动恢复。
            theme_mode: Some(self.ui_prefs.theme_mode),
            // 按天流量持久化，便于下次启动继续累计与绘制趋势。
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
