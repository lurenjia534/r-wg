use std::collections::{HashMap, HashSet, VecDeque};
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::{Entity, SharedString};
use gpui_component::theme::ThemeMode;
use gpui_component::{input::InputState, IconName};
use r_wg::backend::wg::{config, Engine, PeerStats};
use r_wg::dns::{DnsMode, DnsPreset};
use serde::{Deserialize, Serialize};

use super::persistence::{self, StoragePaths};

/// 速度曲线采样点数量（固定窗口）。
pub(crate) const SPARKLINE_SAMPLES: usize = 24;
/// 7 日流量趋势展示天数。
pub(crate) const TRAFFIC_TREND_DAYS: usize = 7;
/// 持久化的流量历史天数（限制 state.json 体积）。
pub(crate) const TRAFFIC_HISTORY_DAYS: usize = 30;
/// Traffic Summary 的滚动天数（过去 30 天 + 前 30 天）。
pub(crate) const TRAFFIC_ROLLING_DAYS: usize = 60;
/// Traffic Summary 的滚动小时数（过去 24 小时，预留 48 小时）。
pub(crate) const TRAFFIC_HOURLY_HISTORY: usize = 48;
/// stop -> start 的最短冷却时间。
pub(crate) const RESTART_COOLDOWN: Duration = Duration::from_millis(300);

/// 配置来源：文件或粘贴文本。
#[derive(Clone)]
pub(crate) enum ConfigSource {
    File { origin_path: Option<PathBuf> },
    Paste,
}

/// Endpoint 地址族标识（基于配置文本解析）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum EndpointFamily {
    V4,
    V6,
    Dual,
    Unknown,
}

/// 隧道配置条目：用于配置列表与编辑器。
#[derive(Clone)]
pub(crate) struct TunnelConfig {
    /// 持久化 ID（用于内部文件名）。
    pub(crate) id: u64,
    /// 配置名称（用于列表与启动）。
    pub(crate) name: String,
    /// 小写版本的名称，用于搜索过滤，避免每次渲染都重复分配/转换。
    pub(crate) name_lower: String,
    /// 配置文本：文件导入时懒加载，因此可能为空。
    pub(crate) text: Option<SharedString>,
    /// 配置来源：文件路径或粘贴内容。
    pub(crate) source: ConfigSource,
    /// 内部存储路径：用于持久化读写。
    pub(crate) storage_path: PathBuf,
    /// Endpoint 地址族 metadata，供 Proxies 页直接读取，避免渲染时派生。
    pub(crate) endpoint_family: EndpointFamily,
}

/// 延迟启动请求（用于 stop -> start 过渡期间）。
#[derive(Clone, Copy)]
pub(crate) struct PendingStart {
    pub(crate) config_id: u64,
}

impl TunnelConfig {
    pub(crate) fn label(&self) -> String {
        match &self.source {
            ConfigSource::File { origin_path } => {
                let file = origin_path
                    .as_ref()
                    .and_then(|path| path.file_name())
                    .and_then(|name| name.to_str())
                    .unwrap_or("file");
                format!("{} ({})", self.name, file)
            }
            ConfigSource::Paste => format!("{} (pasted)", self.name),
        }
    }
}

/// 选中配置的解析缓存，避免渲染时重复解析。
pub(crate) struct ParseCache {
    pub(crate) name: String,
    pub(crate) text_hash: u64,
    pub(crate) parsed: Option<config::WireGuardConfig>,
    pub(crate) error: Option<String>,
}

/// 最近一次载入到输入框的配置，避免重复 set_value。
pub(crate) struct LoadedConfigState {
    pub(crate) name: String,
    pub(crate) text_hash: u64,
}

/// 日流量统计（按本地日期汇总）。
#[derive(Clone)]
pub(crate) struct TrafficDay {
    pub(crate) date: String,
    pub(crate) bytes: u64,
}

/// 按天统计的 RX/TX 统计（用于 30 天窗口）。
#[derive(Clone)]
pub(crate) struct TrafficDayStats {
    pub(crate) date: String,
    pub(crate) rx_bytes: u64,
    pub(crate) tx_bytes: u64,
}

/// 按小时统计的 RX/TX 统计（用于 24 小时窗口）。
#[derive(Clone)]
pub(crate) struct TrafficHour {
    pub(crate) hour: i64,
    pub(crate) rx_bytes: u64,
    pub(crate) tx_bytes: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RightTab {
    Status,
    Logs,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrafficPeriod {
    Today,
    ThisMonth,
    LastMonth,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProxiesViewMode {
    List,
    Gallery,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProxyRunningFilter {
    All,
    Running,
    Idle,
}

/// 左侧导航栏的选中项。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SidebarItem {
    Overview,
    TrafficStats,
    Connections,
    Logs,
    Proxies,
    Rules,
    Dns,
    Providers,
    Configs,
    Advanced,
    Topology,
    RouteMap,
    About,
}

impl SidebarItem {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::TrafficStats => "Traffic Stats",
            Self::Connections => "Connections",
            Self::Logs => "Logs",
            Self::Proxies => "Proxies",
            Self::Rules => "Rules",
            Self::Dns => "DNS",
            Self::Providers => "Providers",
            Self::Configs => "Configs",
            Self::Advanced => "Advanced",
            Self::Topology => "Topology",
            Self::RouteMap => "Route Map",
            Self::About => "About",
        }
    }

    pub(crate) fn icon(self) -> IconName {
        match self {
            Self::Overview => IconName::LayoutDashboard,
            Self::TrafficStats => IconName::ChartPie,
            Self::Connections => IconName::Globe,
            Self::Logs => IconName::SquareTerminal,
            Self::Proxies => IconName::Globe,
            Self::Rules => IconName::Menu,
            Self::Dns => IconName::Search,
            Self::Providers => IconName::Building2,
            Self::Configs => IconName::File,
            Self::Advanced => IconName::Settings2,
            Self::Topology => IconName::Frame,
            Self::RouteMap => IconName::Map,
            Self::About => IconName::Info,
        }
    }
}

pub(crate) struct ConfigsState {
    /// 全部隧道配置。
    pub(crate) configs: Vec<TunnelConfig>,
    /// 配置持久化目录与 state.json 路径。
    pub(crate) storage: Option<StoragePaths>,
    /// 下一个配置 ID（用于内部文件名）。
    pub(crate) next_config_id: u64,
}

impl ConfigsState {
    fn new() -> Self {
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
    /// 解析缓存：只缓存“当前选中”的解析结果，避免每次渲染都解析。
    pub(crate) parse_cache: Option<ParseCache>,
    /// 最近一次写入输入框的配置标记，用于跳过重复 set_value。
    pub(crate) loaded_config: Option<LoadedConfigState>,
    /// 文本缓存：按路径缓存最近读取的配置文本，减少重复 IO。
    pub(crate) config_text_cache: HashMap<PathBuf, SharedString>,
    /// 文本缓存顺序：用于简易 LRU 淘汰。
    pub(crate) config_text_cache_order: VecDeque<PathBuf>,
    /// 代理/节点过滤：上一次的总条目数（用于检测列表变化）。
    pub(crate) proxy_filter_total: usize,
    /// 代理/节点过滤：缓存过滤后的配置 ID 列表，避免每帧全量扫描。
    pub(crate) proxy_filtered_ids: Vec<u64>,
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
}

impl SelectionState {
    fn new() -> Self {
        Self {
            persistence_loaded: false,
            selected_id: None,
            loading_config_id: None,
            loading_config_path: None,
            parse_cache: None,
            loaded_config: None,
            config_text_cache: HashMap::new(),
            config_text_cache_order: VecDeque::new(),
            proxy_filter_total: 0,
            proxy_filtered_ids: Vec::new(),
            endpoint_family_loading: HashSet::new(),
            proxy_country_filter: None,
            proxy_city_filter: None,
            proxy_protocol_filter: None,
            proxy_running_filter: ProxyRunningFilter::All,
            proxy_select_mode: false,
            proxy_selected_ids: HashSet::new(),
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
            });
        }
        runtime.running_id.map(|id| PendingStart { config_id: id })
    }

    pub(crate) fn restore_after_persist(
        &mut self,
        selected_id: Option<u64>,
        configs: &ConfigsState,
    ) {
        self.selected_id = selected_id.filter(|id| configs.get_by_id(*id).is_some());
        self.proxy_filter_total = 0;
        self.parse_cache = None;
        self.loaded_config = None;
        self.loading_config_id = None;
        self.loading_config_path = None;
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
}

impl RuntimeState {
    fn new() -> Self {
        Self {
            running: false,
            busy: false,
            pending_start: None,
            last_stop_at: None,
            running_name: None,
            running_id: None,
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
    }

    pub(crate) fn finish_stop_success(&mut self) {
        self.busy = false;
        self.running = false;
        self.running_name = None;
        self.running_id = None;
        self.last_stop_at = Some(Instant::now());
    }

    pub(crate) fn finish_stop_failure(&mut self) {
        self.busy = false;
        self.pending_start = None;
    }

    pub(crate) fn begin_start(&mut self) {
        self.busy = true;
    }

    pub(crate) fn finish_start_attempt(&mut self) {
        self.busy = false;
    }

    pub(crate) fn mark_started(&mut self, selected: &TunnelConfig) {
        self.running = true;
        self.running_name = Some(selected.name.clone());
        self.running_id = Some(selected.id);
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
    // 7 日流量趋势（按天累计）。
    pub(crate) traffic_days: Vec<TrafficDay>,
    pub(crate) traffic_days_v2: Vec<TrafficDayStats>,
    pub(crate) traffic_hours: Vec<TrafficHour>,
    pub(crate) config_traffic_days: HashMap<u64, Vec<TrafficDayStats>>,
    pub(crate) config_traffic_hours: HashMap<u64, Vec<TrafficHour>>,
    pub(crate) traffic_dirty: bool,
    pub(crate) traffic_last_persist_at: Option<Instant>,
}

impl StatsState {
    fn new() -> Self {
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
            traffic_days: Vec::new(),
            traffic_days_v2: Vec::new(),
            traffic_hours: Vec::new(),
            config_traffic_days: HashMap::new(),
            config_traffic_hours: HashMap::new(),
            traffic_dirty: false,
            traffic_last_persist_at: None,
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
        self.stats_note = "Fetching peer stats...".into();
    }

    pub(crate) fn set_stats_error(&mut self, message: impl Into<SharedString>) {
        self.stats_note = message.into();
    }
}

pub(crate) struct UiPrefsState {
    pub(crate) log_auto_follow: bool,
    pub(crate) right_tab: RightTab,
    pub(crate) traffic_period: TrafficPeriod,
    pub(crate) proxies_view_mode: ProxiesViewMode,
    pub(crate) theme_mode: ThemeMode,
    pub(crate) dns_mode: DnsMode,
    pub(crate) dns_preset: DnsPreset,
    pub(crate) sidebar_active: SidebarItem,
}

impl UiPrefsState {
    fn new(theme_mode: ThemeMode) -> Self {
        Self {
            log_auto_follow: true,
            right_tab: RightTab::Status,
            traffic_period: TrafficPeriod::Today,
            proxies_view_mode: ProxiesViewMode::List,
            theme_mode,
            dns_mode: DnsMode::FollowConfig,
            dns_preset: DnsPreset::CloudflareStandard,
            sidebar_active: SidebarItem::Overview,
        }
    }
}

pub(crate) struct UiState {
    // 输入控件句柄（懒创建，避免提前绑定窗口上下文）。
    pub(crate) name_input: Option<Entity<InputState>>,
    pub(crate) config_input: Option<Entity<InputState>>,
    pub(crate) log_input: Option<Entity<InputState>>,
    pub(crate) proxy_search_input: Option<Entity<InputState>>,
    // 日志状态与提示。
    pub(crate) status: SharedString,
    pub(crate) last_error: Option<SharedString>,
    pub(crate) backend_status: SharedString,
    pub(crate) backend_detail: SharedString,
    pub(crate) backend_available: bool,
}

impl UiState {
    fn new() -> Self {
        Self {
            name_input: None,
            config_input: None,
            log_input: None,
            proxy_search_input: None,
            status: "Ready".into(),
            last_error: None,
            backend_status: "Unknown".into(),
            backend_detail: "Refresh to probe the Linux privileged backend.".into(),
            backend_available: false,
        }
    }

    pub(crate) fn set_status(&mut self, message: impl Into<SharedString>) {
        self.status = message.into();
    }

    pub(crate) fn set_error(&mut self, message: impl Into<SharedString>) {
        let message = message.into();
        self.status = message.clone();
        self.last_error = Some(message);
    }

    pub(crate) fn set_backend_status(
        &mut self,
        status: impl Into<SharedString>,
        detail: impl Into<SharedString>,
        available: bool,
    ) {
        self.backend_status = status.into();
        self.backend_detail = detail.into();
        self.backend_available = available;
    }
}

pub(crate) struct WgApp {
    pub(crate) engine: Engine,
    pub(crate) configs: ConfigsState,
    pub(crate) selection: SelectionState,
    pub(crate) runtime: RuntimeState,
    pub(crate) stats: StatsState,
    pub(crate) ui_prefs: UiPrefsState,
    pub(crate) ui: UiState,
}

impl WgApp {
    pub(crate) fn new(engine: Engine, theme_mode: ThemeMode) -> Self {
        Self {
            engine,
            configs: ConfigsState::new(),
            selection: SelectionState::new(),
            runtime: RuntimeState::new(),
            stats: StatsState::new(),
            ui_prefs: UiPrefsState::new(theme_mode),
            ui: UiState::new(),
        }
    }
}

fn init_rate_history() -> VecDeque<f32> {
    // 预填充 0，保持曲线长度稳定。
    let mut history = VecDeque::with_capacity(SPARKLINE_SAMPLES);
    for _ in 0..SPARKLINE_SAMPLES {
        history.push_back(0.0);
    }
    history
}
