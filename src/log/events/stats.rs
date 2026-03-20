// 统计快照事件日志（scope = stats）。
use crate::log::{self, LogLevel};
use crate::log_debug;

pub struct SnapshotArgs<'a> {
    pub tun_name: Option<&'a str>,
    pub elapsed_secs: Option<f64>,
    pub total_rx: u64,
    pub total_tx: u64,
    pub rx_delta: u64,
    pub tx_delta: u64,
    pub rx_rate_bps: f64,
    pub tx_rate_bps: f64,
    pub iface_rx: Option<u64>,
    pub iface_tx: Option<u64>,
    pub iface_rx_rate_bps: f64,
    pub iface_tx_rate_bps: f64,
}

pub fn snapshot(args: SnapshotArgs<'_>) {
    if !log::enabled_for(LogLevel::Debug, "stats") {
        return;
    }
    let name = args.tun_name.unwrap_or("-");
    let elapsed_text = args
        .elapsed_secs
        .map(|value| format!("{value:.2}s"))
        .unwrap_or_else(|| "-".to_string());
    let iface_text = match (args.iface_rx, args.iface_tx) {
        (Some(rx), Some(tx)) => format!(
            "iface_rx={rx} iface_tx={tx} iface_rx_rate={:.0}B/s iface_tx_rate={:.0}B/s",
            args.iface_rx_rate_bps, args.iface_tx_rate_bps
        ),
        _ => "iface_stats=unavailable".to_string(),
    };

    log_debug!(
        "stats",
        "tun={name} elapsed={elapsed_text} rx_total={} tx_total={} rx_delta={} tx_delta={} rx_rate={:.0}B/s tx_rate={:.0}B/s {iface_text}",
        args.total_rx,
        args.total_tx,
        args.rx_delta,
        args.tx_delta,
        args.rx_rate_bps,
        args.tx_rate_bps
    );
}
