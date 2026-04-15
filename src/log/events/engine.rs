// 引擎生命周期事件日志（scope = engine）。
use crate::log_info;

pub fn start(tun_name: &str, config_len: usize) {
    log_info!(
        "engine",
        "start: tun={} config_len={}",
        tun_name,
        config_len
    );
}

pub fn auto_fwmark(fwmark: u32) {
    log_info!("engine", "auto fwmark: 0x{fwmark:x}");
}

pub fn config_parsed() {
    log_info!("engine", "config parsed");
}

pub fn device_created() {
    log_info!("engine", "device created");
}

pub fn device_configured() {
    log_info!("engine", "device configured");
}

pub fn network_configured() {
    log_info!("engine", "network configured");
}

pub fn quantum_upgrade_requested() {
    log_info!("engine", "quantum upgrade requested");
}

pub fn quantum_upgrade_completed() {
    log_info!("engine", "quantum upgrade completed");
}

pub fn quantum_upgrade_failed(message: &str) {
    log_info!("engine", "quantum upgrade failed: {}", message);
}

pub fn stop_requested() {
    log_info!("engine", "stop requested");
}

pub fn device_stopped() {
    log_info!("engine", "device stopped");
}
