// 统计快照事件日志（scope = stats）。
use crate::log::{self, LogLevel};
use crate::log_debug;

pub fn snapshot(
    tun_name: Option<&str>,
    elapsed_secs: Option<f64>,
    total_rx: u64,
    total_tx: u64,
    rx_delta: u64,
    tx_delta: u64,
    rx_rate_bps: f64,
    tx_rate_bps: f64,
    iface_rx: Option<u64>,
    iface_tx: Option<u64>,
    iface_rx_rate_bps: f64,
    iface_tx_rate_bps: f64,
) {
    if !log::enabled_for(LogLevel::Debug, "stats") {
        return;
    }
    let name = tun_name.unwrap_or("-");
    let elapsed_text = elapsed_secs
        .map(|value| format!("{value:.2}s"))
        .unwrap_or_else(|| "-".to_string());
    let iface_text = match (iface_rx, iface_tx) {
        (Some(rx), Some(tx)) => format!(
            "iface_rx={rx} iface_tx={tx} iface_rx_rate={:.0}B/s iface_tx_rate={:.0}B/s",
            iface_rx_rate_bps, iface_tx_rate_bps
        ),
        _ => "iface_stats=unavailable".to_string(),
    };

    log_debug!(
        "stats",
        "tun={name} elapsed={elapsed_text} rx_total={total_rx} tx_total={total_tx} rx_delta={rx_delta} tx_delta={tx_delta} rx_rate={:.0}B/s tx_rate={:.0}B/s {iface_text}",
        rx_rate_bps,
        tx_rate_bps
    );
}
