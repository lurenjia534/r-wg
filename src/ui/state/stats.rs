use std::collections::VecDeque;
use std::time::Instant;

use gpui::SharedString;
use r_wg::backend::wg::PeerStats;

use super::{TrafficStore, SPARKLINE_SAMPLES};

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

fn init_rate_history() -> VecDeque<f32> {
    // 预填充 0，保持曲线长度稳定。
    let mut history = VecDeque::with_capacity(SPARKLINE_SAMPLES);
    for _ in 0..SPARKLINE_SAMPLES {
        history.push_back(0.0);
    }
    history
}
