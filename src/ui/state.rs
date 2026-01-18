use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::time::Instant;

use gpui::{Entity, SharedString};
use gpui_component::{IconName, input::InputState};
use r_wg::backend::wg::{config, Engine, PeerStats};
use r_wg::dns::{DnsMode, DnsPreset};

/// 速度曲线采样点数量（固定窗口）。
pub(crate) const SPARKLINE_SAMPLES: usize = 24;

/// 配置来源：文件或粘贴文本。
#[derive(Clone)]
pub(crate) enum ConfigSource {
    File(PathBuf),
    Paste,
}

/// 隧道配置条目：用于配置列表与编辑器。
#[derive(Clone)]
pub(crate) struct TunnelConfig {
    /// 配置名称（用于列表与启动）。
    pub(crate) name: String,
    /// 小写版本的名称，用于搜索过滤，避免每次渲染都重复分配/转换。
    pub(crate) name_lower: String,
    /// 配置文本：文件导入时懒加载，因此可能为空。
    pub(crate) text: Option<SharedString>,
    /// 配置来源：文件路径或粘贴内容。
    pub(crate) source: ConfigSource,
}

impl TunnelConfig {
    pub(crate) fn label(&self) -> String {
        match &self.source {
            ConfigSource::File(path) => {
                let file = path
                    .file_name()
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RightTab {
    Status,
    Logs,
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
        }
    }
}

pub(crate) struct WgApp {
    // 后端与配置列表。
    pub(crate) engine: Engine,
    pub(crate) configs: Vec<TunnelConfig>,
    pub(crate) selected: Option<usize>,
    /// 正在异步加载的配置索引（用于防止 UI 写入旧数据）。
    pub(crate) loading_config: Option<usize>,
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
    /// 代理/节点过滤：上一次查询字符串。
    pub(crate) proxy_filter_query: String,
    /// 代理/节点过滤：上一次的总条目数（用于检测列表变化）。
    pub(crate) proxy_filter_total: usize,
    /// 代理/节点过滤：缓存过滤后的索引列表，避免每帧全量扫描。
    pub(crate) proxy_filtered_indices: Vec<usize>,
    // 输入控件句柄（懒创建，避免提前绑定窗口上下文）。
    pub(crate) name_input: Option<Entity<InputState>>,
    pub(crate) config_input: Option<Entity<InputState>>,
    pub(crate) log_input: Option<Entity<InputState>>,
    pub(crate) proxy_search_input: Option<Entity<InputState>>,
    // 日志状态与提示。
    pub(crate) log_auto_follow: bool,
    pub(crate) status: SharedString,
    pub(crate) last_error: Option<SharedString>,
    // 运行与连接状态。
    pub(crate) running: bool,
    pub(crate) busy: bool,
    pub(crate) running_name: Option<String>,
    pub(crate) peer_stats: Vec<PeerStats>,
    // 统计展示（用于右侧面板与图表）。
    pub(crate) stats_note: SharedString,
    pub(crate) stats_generation: u64,
    // 页面选择与模式开关。
    pub(crate) right_tab: RightTab,
    pub(crate) dns_mode: DnsMode,
    pub(crate) dns_preset: DnsPreset,
    pub(crate) sidebar_active: SidebarItem,
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
}

impl WgApp {
    pub(crate) fn new(engine: Engine) -> Self {
        Self {
            engine,
            configs: Vec::new(),
            selected: None,
            loading_config: None,
            loading_config_path: None,
            parse_cache: None,
            loaded_config: None,
            config_text_cache: HashMap::new(),
            config_text_cache_order: VecDeque::new(),
            proxy_filter_query: String::new(),
            proxy_filter_total: 0,
            proxy_filtered_indices: Vec::new(),
            name_input: None,
            config_input: None,
            log_input: None,
            proxy_search_input: None,
            log_auto_follow: true,
            status: "Ready".into(),
            last_error: None,
            running: false,
            busy: false,
            running_name: None,
            peer_stats: Vec::new(),
            stats_note: "Peer stats unavailable".into(),
            stats_generation: 0,
            right_tab: RightTab::Status,
            dns_mode: DnsMode::FollowConfig,
            dns_preset: DnsPreset::CloudflareStandard,
            sidebar_active: SidebarItem::Overview,
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
