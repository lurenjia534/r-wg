use std::collections::{HashMap, HashSet, VecDeque};
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use gpui::{Entity, SharedString};
use gpui_component::input::InputState;
use gpui_component::theme::ThemeMode;
use r_wg::backend::wg::{DaitaMode, EphemeralFailureKind, PeerStats, QuantumMode};
use r_wg::core::route_plan::RouteApplyReport;
use r_wg::dns::{DnsMode, DnsPreset};

use crate::ui::features::configs::state::{ConfigsWorkspace, LoadedConfigState};
use crate::ui::features::themes::AppearancePolicy;
use crate::ui::persistence::{self, StoragePaths};

use super::{
    BackendDiagnostic, BackendHealth, ConfigInspectorTab, DaitaResourcesDiagnostic, PendingStart,
    ProxiesViewMode, ProxyRunningFilter, RouteFamilyFilter, RouteMapMode, SidebarItem,
    ToolsWorkspace, TrafficPeriod, TrafficStore, TunnelConfig, DEFAULT_CONFIGS_INSPECTOR_WIDTH,
    DEFAULT_CONFIGS_LIBRARY_WIDTH, DEFAULT_ROUTE_MAP_INSPECTOR_WIDTH,
    DEFAULT_ROUTE_MAP_INVENTORY_WIDTH, RESTART_COOLDOWN,
};

// App state containers excluding the WgApp facade.

pub(crate) struct ConfigsState {
    /// 全部隧道配置。
    pub(crate) configs: Vec<TunnelConfig>,
    /// 配置持久化目录与 state.json 路径。
    pub(crate) storage: Option<StoragePaths>,
    /// 下一个配置 ID（用于内部文件名）。
    pub(crate) next_config_id: u64,
}

impl ConfigsState {
    pub(super) fn new() -> Self {
        Self {
            configs: Vec::new(),
            storage: None,
            next_config_id: 1,
        }
    }

    pub(crate) fn ensure_storage(&mut self) -> Result<StoragePaths, String> {
        if let Some(storage) = &self.storage {
            return Ok(storage.clone());
        }
        let storage = persistence::ensure_storage_dirs()?;
        self.storage = Some(storage.clone());
        Ok(storage)
    }

    pub(crate) fn alloc_config_id(&mut self) -> u64 {
        let id = self.next_config_id.max(1);
        self.next_config_id = id.saturating_add(1);
        id
    }

    pub(crate) fn next_config_id(&self) -> u64 {
        self.next_config_id.max(1)
    }

    pub(crate) fn find_by_id(&self, config_id: u64) -> Option<TunnelConfig> {
        self.get_by_id(config_id).cloned()
    }

    pub(crate) fn find_index_by_id(&self, config_id: u64) -> Option<usize> {
        self.iter().position(|config| config.id == config_id)
    }

    pub(crate) fn get_by_id(&self, config_id: u64) -> Option<&TunnelConfig> {
        self.iter().find(|config| config.id == config_id)
    }

    pub(crate) fn get_mut_by_id(&mut self, config_id: u64) -> Option<&mut TunnelConfig> {
        self.iter_mut().find(|config| config.id == config_id)
    }
}

impl Deref for ConfigsState {
    type Target = Vec<TunnelConfig>;

    fn deref(&self) -> &Self::Target {
        &self.configs
    }
}

impl DerefMut for ConfigsState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.configs
    }
}

impl<'a> IntoIterator for &'a ConfigsState {
    type Item = &'a TunnelConfig;
    type IntoIter = std::slice::Iter<'a, TunnelConfig>;

    fn into_iter(self) -> Self::IntoIter {
        self.configs.iter()
    }
}

impl<'a> IntoIterator for &'a mut ConfigsState {
    type Item = &'a mut TunnelConfig;
    type IntoIter = std::slice::IterMut<'a, TunnelConfig>;

    fn into_iter(self) -> Self::IntoIter {
        self.configs.iter_mut()
    }
}

pub(crate) struct SelectionState {
    /// 是否已触发持久化加载，避免重复启动加载任务。
    pub(crate) persistence_loaded: bool,
    /// 当前单选中的配置 ID。
    pub(crate) selected_id: Option<u64>,
    /// 正在异步加载的配置 ID（用于防止 UI 写入旧数据）。
    pub(crate) loading_config_id: Option<u64>,
    /// 正在异步加载的配置路径（用于防止索引复用带来的错写）。
    pub(crate) loading_config_path: Option<PathBuf>,
    /// 最近一次写入输入框的配置标记，用于跳过重复 set_value。
    pub(crate) loaded_config: Option<LoadedConfigState>,
    /// 文本缓存：按路径缓存最近读取的配置文本，减少重复 IO。
    pub(crate) config_text_cache: HashMap<PathBuf, SharedString>,
    /// 文本缓存顺序：用于简易 LRU 淘汰。
    pub(crate) config_text_cache_order: VecDeque<PathBuf>,
    /// Endpoint metadata 正在后台计算中的配置 ID。
    pub(crate) endpoint_family_loading: HashSet<u64>,
    /// 代理列表结构化筛选：国家。
    pub(crate) proxy_country_filter: Option<String>,
    /// 代理列表结构化筛选：城市。
    pub(crate) proxy_city_filter: Option<String>,
    /// 代理列表结构化筛选：协议类型。
    pub(crate) proxy_protocol_filter: Option<String>,
    /// 代理列表结构化筛选：运行状态。
    pub(crate) proxy_running_filter: ProxyRunningFilter,
    /// 代理/节点多选模式开关。
    pub(crate) proxy_select_mode: bool,
    /// 代理/节点多选：选中的配置 ID 列表。
    pub(crate) proxy_selected_ids: HashSet<u64>,
    pub(crate) selection_revision: u64,
}

impl SelectionState {
    pub(super) fn new() -> Self {
        Self {
            persistence_loaded: false,
            selected_id: None,
            loading_config_id: None,
            loading_config_path: None,
            loaded_config: None,
            config_text_cache: HashMap::new(),
            config_text_cache_order: VecDeque::new(),
            endpoint_family_loading: HashSet::new(),
            proxy_country_filter: None,
            proxy_city_filter: None,
            proxy_protocol_filter: None,
            proxy_running_filter: ProxyRunningFilter::All,
            proxy_select_mode: false,
            proxy_selected_ids: HashSet::new(),
            selection_revision: 0,
        }
    }

    pub(crate) fn begin_persistence_load(&mut self) -> bool {
        if self.persistence_loaded {
            return false;
        }
        self.persistence_loaded = true;
        true
    }

    pub(crate) fn build_pending_start(
        &self,
        configs: &ConfigsState,
        runtime: &RuntimeState,
    ) -> Option<PendingStart> {
        if let Some(config_id) = self.selected_id {
            return configs.get_by_id(config_id).map(|config| PendingStart {
                config_id: config.id,
                password_authorized: false,
            });
        }
        runtime.running_id.map(|id| PendingStart {
            config_id: id,
            password_authorized: false,
        })
    }

    pub(crate) fn restore_after_persist(
        &mut self,
        selected_id: Option<u64>,
        configs: &ConfigsState,
    ) {
        self.selected_id = selected_id.filter(|id| configs.get_by_id(*id).is_some());
        self.loaded_config = None;
        self.loading_config_id = None;
        self.loading_config_path = None;
        self.selection_revision = self.selection_revision.wrapping_add(1);
    }
}

pub(crate) struct RuntimeState {
    /// 是否处于运行中。
    pub(crate) running: bool,
    /// 是否有异步流程正在执行。
    pub(crate) busy: bool,
    /// 停止过程中记录的“待启动”请求。
    pub(crate) pending_start: Option<PendingStart>,
    /// 最近一次停止完成的时间（用于冷却启动）。
    pub(crate) last_stop_at: Option<Instant>,
    pub(crate) running_name: Option<String>,
    pub(crate) running_id: Option<u64>,
    pub(crate) quantum_protected: bool,
    pub(crate) last_quantum_failure: Option<EphemeralFailureKind>,
    pub(crate) daita_active: bool,
    pub(crate) last_daita_failure: Option<EphemeralFailureKind>,
    pub(crate) last_apply_report: Option<RouteApplyReport>,
    pub(crate) runtime_revision: u64,
}

static LAST_APPLY_REPORT: OnceLock<Mutex<Option<RouteApplyReport>>> = OnceLock::new();

fn last_apply_report_cell() -> &'static Mutex<Option<RouteApplyReport>> {
    LAST_APPLY_REPORT.get_or_init(|| Mutex::new(None))
}

fn set_current_apply_report(report: Option<RouteApplyReport>) {
    if let Ok(mut slot) = last_apply_report_cell().lock() {
        *slot = report;
    }
}

pub(crate) fn current_apply_report() -> Option<RouteApplyReport> {
    last_apply_report_cell()
        .lock()
        .ok()
        .and_then(|slot| slot.clone())
}

impl RuntimeState {
    pub(super) fn new() -> Self {
        Self {
            running: false,
            busy: false,
            pending_start: None,
            last_stop_at: None,
            running_name: None,
            running_id: None,
            quantum_protected: false,
            last_quantum_failure: None,
            daita_active: false,
            last_daita_failure: None,
            last_apply_report: None,
            runtime_revision: 0,
        }
    }

    pub(crate) fn restart_delay(&self) -> Option<Duration> {
        let last_stop = self.last_stop_at?;
        let elapsed = last_stop.elapsed();
        if elapsed >= RESTART_COOLDOWN {
            None
        } else {
            Some(RESTART_COOLDOWN - elapsed)
        }
    }

    pub(crate) fn queue_pending_start(&mut self, pending: Option<PendingStart>) -> bool {
        let Some(pending) = pending else {
            return false;
        };
        self.pending_start = Some(pending);
        true
    }

    pub(crate) fn begin_stop(&mut self) {
        self.busy = true;
        self.runtime_revision = self.runtime_revision.wrapping_add(1);
    }

    pub(crate) fn finish_stop_success(&mut self) {
        self.busy = false;
        self.running = false;
        self.running_name = None;
        self.running_id = None;
        self.quantum_protected = false;
        self.last_quantum_failure = None;
        self.daita_active = false;
        self.last_daita_failure = None;
        self.clear_last_apply_report();
        self.last_stop_at = Some(Instant::now());
        self.runtime_revision = self.runtime_revision.wrapping_add(1);
    }

    pub(crate) fn finish_stop_failure(&mut self) {
        self.busy = false;
        self.pending_start = None;
        self.runtime_revision = self.runtime_revision.wrapping_add(1);
    }

    pub(crate) fn begin_start(&mut self) {
        self.busy = true;
        self.quantum_protected = false;
        self.last_quantum_failure = None;
        self.daita_active = false;
        self.last_daita_failure = None;
        self.runtime_revision = self.runtime_revision.wrapping_add(1);
    }

    pub(crate) fn finish_start_attempt(&mut self) {
        self.busy = false;
        self.runtime_revision = self.runtime_revision.wrapping_add(1);
    }

    pub(crate) fn mark_started(&mut self, selected: &TunnelConfig) {
        self.running = true;
        self.running_name = Some(selected.name.clone());
        self.running_id = Some(selected.id);
        self.runtime_revision = self.runtime_revision.wrapping_add(1);
    }

    pub(crate) fn set_quantum_status(
        &mut self,
        protected: bool,
        failure: Option<EphemeralFailureKind>,
    ) {
        self.quantum_protected = protected;
        self.last_quantum_failure = failure;
        self.runtime_revision = self.runtime_revision.wrapping_add(1);
    }

    pub(crate) fn set_daita_status(
        &mut self,
        active: bool,
        failure: Option<EphemeralFailureKind>,
    ) {
        self.daita_active = active;
        self.last_daita_failure = failure;
        self.runtime_revision = self.runtime_revision.wrapping_add(1);
    }

    pub(crate) fn set_last_apply_report(&mut self, report: Option<RouteApplyReport>) {
        self.last_apply_report = report.clone();
        set_current_apply_report(report);
        self.runtime_revision = self.runtime_revision.wrapping_add(1);
    }

    pub(crate) fn clear_last_apply_report(&mut self) {
        self.last_apply_report = None;
        set_current_apply_report(None);
    }
}

pub(crate) struct StatsState {
    /// 最近一次拉取到的 Peer 统计。
    pub(crate) peer_stats: Vec<PeerStats>,
    // 统计展示（用于右侧面板与图表）。
    pub(crate) stats_note: SharedString,
    pub(crate) stats_generation: u64,
    // 速率/流量采样窗口。
    pub(crate) started_at: Option<Instant>,
    pub(crate) last_stats_at: Option<Instant>,
    pub(crate) last_rx_bytes: u64,
    pub(crate) last_tx_bytes: u64,
    pub(crate) rx_rate_bps: f64,
    pub(crate) tx_rate_bps: f64,
    pub(crate) rx_rate_history: VecDeque<f32>,
    pub(crate) tx_rate_history: VecDeque<f32>,
    pub(crate) stats_idle_samples: u8,
    pub(crate) last_iface_rx_bytes: u64,
    pub(crate) last_iface_tx_bytes: u64,
    pub(crate) iface_rx_rate_bps: f64,
    pub(crate) iface_tx_rate_bps: f64,
    pub(crate) process_rss_bytes: Option<u64>,
    pub(crate) traffic: TrafficStore,
    pub(crate) stats_revision: u64,
}

impl StatsState {
    pub(super) fn new() -> Self {
        Self {
            peer_stats: Vec::new(),
            stats_note: "Peer stats unavailable".into(),
            stats_generation: 0,
            started_at: None,
            last_stats_at: None,
            last_rx_bytes: 0,
            last_tx_bytes: 0,
            rx_rate_bps: 0.0,
            tx_rate_bps: 0.0,
            rx_rate_history: init_rate_history(),
            tx_rate_history: init_rate_history(),
            stats_idle_samples: 0,
            last_iface_rx_bytes: 0,
            last_iface_tx_bytes: 0,
            iface_rx_rate_bps: 0.0,
            iface_tx_rate_bps: 0.0,
            process_rss_bytes: None,
            traffic: TrafficStore::new(),
            stats_revision: 0,
        }
    }

    pub(crate) fn reset_rate_history(&mut self) {
        self.rx_rate_history = init_rate_history();
        self.tx_rate_history = init_rate_history();
    }

    pub(crate) fn clear_runtime_metrics(&mut self) {
        self.peer_stats.clear();
        self.stats_note = "Peer stats unavailable".into();
        self.started_at = None;
        self.last_stats_at = None;
        self.last_rx_bytes = 0;
        self.last_tx_bytes = 0;
        self.rx_rate_bps = 0.0;
        self.tx_rate_bps = 0.0;
        self.reset_rate_history();
        self.stats_idle_samples = 0;
        self.last_iface_rx_bytes = 0;
        self.last_iface_tx_bytes = 0;
        self.iface_rx_rate_bps = 0.0;
        self.iface_tx_rate_bps = 0.0;
        self.process_rss_bytes = None;
        self.stats_revision = self.stats_revision.wrapping_add(1);
    }

    pub(crate) fn reset_for_start(&mut self) {
        self.started_at = Some(Instant::now());
        self.last_stats_at = None;
        self.last_rx_bytes = 0;
        self.last_tx_bytes = 0;
        self.rx_rate_bps = 0.0;
        self.tx_rate_bps = 0.0;
        self.reset_rate_history();
        self.stats_idle_samples = 0;
        self.last_iface_rx_bytes = 0;
        self.last_iface_tx_bytes = 0;
        self.iface_rx_rate_bps = 0.0;
        self.iface_tx_rate_bps = 0.0;
        self.process_rss_bytes = None;
        self.stats_note = "Fetching peer stats...".into();
        self.stats_revision = self.stats_revision.wrapping_add(1);
    }

    pub(crate) fn set_stats_error(&mut self, message: impl Into<SharedString>) -> bool {
        let message = message.into();
        if self.stats_note == message {
            return false;
        }
        self.stats_note = message;
        true
    }
}

pub(crate) struct UiPrefsState {
    pub(crate) log_auto_follow: bool,
    pub(crate) require_connect_password: bool,
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
    pub(crate) dns_mode: DnsMode,
    pub(crate) dns_preset: DnsPreset,
    pub(crate) quantum_mode: QuantumMode,
    pub(crate) daita_mode: DaitaMode,
}

impl UiPrefsState {
    pub(super) fn new(
        appearance_policy: AppearancePolicy,
        resolved_theme_mode: ThemeMode,
        theme_light_key: Option<SharedString>,
        theme_dark_key: Option<SharedString>,
        theme_light_name: Option<SharedString>,
        theme_dark_name: Option<SharedString>,
    ) -> Self {
        Self {
            log_auto_follow: true,
            require_connect_password: false,
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
            dns_mode: DnsMode::FollowConfig,
            dns_preset: DnsPreset::CloudflareStandard,
            quantum_mode: QuantumMode::Off,
            daita_mode: DaitaMode::Off,
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

pub(crate) struct PersistenceState {
    next_revision: u64,
    queued_revision: Option<u64>,
    pub(crate) worker_active: bool,
}

impl PersistenceState {
    pub(super) fn new() -> Self {
        Self {
            next_revision: 0,
            queued_revision: None,
            worker_active: false,
        }
    }

    pub(crate) fn enqueue(&mut self) -> u64 {
        self.next_revision = self.next_revision.saturating_add(1);
        self.queued_revision = Some(self.next_revision);
        self.next_revision
    }

    pub(crate) fn take_queued_revision(&mut self) -> Option<u64> {
        self.queued_revision.take()
    }

    pub(crate) fn has_pending(&self) -> bool {
        self.queued_revision.is_some()
    }
}

pub(crate) struct RouteMapSearchState {
    pub(crate) raw_query: SharedString,
    pub(crate) debounced_query: SharedString,
    next_revision: u64,
    queued_revision: Option<u64>,
    pub(crate) worker_active: bool,
}

impl RouteMapSearchState {
    pub(super) fn new() -> Self {
        Self {
            raw_query: SharedString::default(),
            debounced_query: SharedString::default(),
            next_revision: 0,
            queued_revision: None,
            worker_active: false,
        }
    }

    pub(super) fn enqueue(&mut self) -> u64 {
        self.next_revision = self.next_revision.saturating_add(1);
        self.queued_revision = Some(self.next_revision);
        self.next_revision
    }

    pub(super) fn take_queued_revision(&mut self) -> Option<u64> {
        self.queued_revision.take()
    }
}

fn init_rate_history() -> VecDeque<f32> {
    // 预填充 0，保持曲线长度稳定。
    let mut history = VecDeque::with_capacity(crate::ui::state::SPARKLINE_SAMPLES);
    for _ in 0..crate::ui::state::SPARKLINE_SAMPLES {
        history.push_back(0.0);
    }
    history
}

pub(crate) struct UiState {
    pub(crate) log_input: Option<Entity<InputState>>,
    pub(crate) proxy_search_input: Option<Entity<InputState>>,
    pub(crate) route_map_search_input: Option<Entity<InputState>>,
    pub(crate) route_map_search: RouteMapSearchState,
    pub(crate) configs_workspace: Option<Entity<ConfigsWorkspace>>,
    pub(crate) tools_workspace: Option<Entity<ToolsWorkspace>>,
    // 日志状态与提示。
    pub(crate) status: SharedString,
    pub(crate) last_error: Option<SharedString>,
    pub(crate) backend: BackendDiagnostic,
    pub(crate) backend_last_error: Option<SharedString>,
    pub(crate) daita_resources: DaitaResourcesDiagnostic,
    pub(crate) theme_appearance_observer_ready: bool,
}

impl UiState {
    pub(super) fn new() -> Self {
        Self {
            log_input: None,
            proxy_search_input: None,
            route_map_search_input: None,
            route_map_search: RouteMapSearchState::new(),
            configs_workspace: None,
            tools_workspace: None,
            status: "Ready".into(),
            last_error: None,
            backend: BackendDiagnostic::default_for_platform(),
            backend_last_error: None,
            daita_resources: DaitaResourcesDiagnostic::default_state(),
            theme_appearance_observer_ready: false,
        }
    }

    pub(crate) fn set_status(&mut self, message: impl Into<SharedString>) -> bool {
        let message = message.into();
        if self.status == message {
            return false;
        }
        self.status = message;
        true
    }

    pub(crate) fn set_error(&mut self, message: impl Into<SharedString>) -> bool {
        let message = message.into();
        let changed = self.status != message || self.last_error.as_ref() != Some(&message);
        self.status = message.clone();
        self.last_error = Some(message);
        changed
    }

    pub(crate) fn set_backend_diagnostic(&mut self, diagnostic: BackendDiagnostic) {
        match diagnostic.health {
            BackendHealth::AccessDenied
            | BackendHealth::VersionMismatch { .. }
            | BackendHealth::Unreachable => {
                self.backend_last_error = Some(diagnostic.detail.clone());
            }
            BackendHealth::Running | BackendHealth::Installed | BackendHealth::NotInstalled => {
                self.backend_last_error = None;
            }
            BackendHealth::Unknown | BackendHealth::Checking | BackendHealth::Working { .. } => {}
            #[cfg(not(any(target_os = "linux", target_os = "windows")))]
            BackendHealth::Unsupported => {
                self.backend_last_error = None;
            }
        }
        self.backend = diagnostic;
    }

    pub(crate) fn set_backend_last_error(&mut self, message: impl Into<SharedString>) {
        self.backend_last_error = Some(message.into());
    }

    pub(crate) fn set_daita_resources_diagnostic(
        &mut self,
        diagnostic: DaitaResourcesDiagnostic,
    ) {
        self.daita_resources = diagnostic;
    }
}
