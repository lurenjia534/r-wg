use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Instant;

use gpui::{Entity, SharedString};
use gpui_component::{IconName, input::InputState};
use r_wg::backend::wg::{Engine, PeerStats};

pub(crate) const SPARKLINE_SAMPLES: usize = 24;

#[derive(Clone)]
pub(crate) enum ConfigSource {
    File(PathBuf),
    Paste,
}

#[derive(Clone)]
pub(crate) struct TunnelConfig {
    pub(crate) name: String,
    pub(crate) text: String,
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
            Self::Providers => IconName::Building2,
            Self::Configs => IconName::File,
            Self::Advanced => IconName::Settings2,
            Self::Topology => IconName::Frame,
            Self::RouteMap => IconName::Map,
        }
    }
}

pub(crate) struct WgApp {
    pub(crate) engine: Engine,
    pub(crate) configs: Vec<TunnelConfig>,
    pub(crate) selected: Option<usize>,
    pub(crate) name_input: Option<Entity<InputState>>,
    pub(crate) config_input: Option<Entity<InputState>>,
    pub(crate) log_input: Option<Entity<InputState>>,
    pub(crate) log_auto_follow: bool,
    pub(crate) status: SharedString,
    pub(crate) last_error: Option<SharedString>,
    pub(crate) running: bool,
    pub(crate) busy: bool,
    pub(crate) running_name: Option<String>,
    pub(crate) peer_stats: Vec<PeerStats>,
    pub(crate) stats_note: SharedString,
    pub(crate) stats_generation: u64,
    pub(crate) right_tab: RightTab,
    pub(crate) sidebar_active: SidebarItem,
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
            name_input: None,
            config_input: None,
            log_input: None,
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
    let mut history = VecDeque::with_capacity(SPARKLINE_SAMPLES);
    for _ in 0..SPARKLINE_SAMPLES {
        history.push_back(0.0);
    }
    history
}
