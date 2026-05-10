// 引擎生命周期事件日志（scope = engine）。
use crate::{log_error, log_info, log_warn};

pub fn start(tun_name: &str, config_len: usize) {
    log_info!(
        "engine",
        "start: tun={} config_len={}",
        tun_name,
        config_len
    );
}

pub fn tunnel_started() {
    log_info!("engine", "tunnel started");
}

pub fn tunnel_start_failed(err: &impl std::fmt::Display) {
    log_warn!("engine", "tunnel start failed: {err}");
}

pub fn auto_fwmark(fwmark: u32) {
    log_info!("engine", "auto fwmark: 0x{fwmark:x}");
}

pub fn config_parsed() {
    log_info!("engine", "config parsed");
}

pub fn wireguard_backend_preference(preference: &str) {
    log_info!("engine", "WireGuard backend preference: {preference}");
}

pub fn wireguard_backend_resolved(backend: &str) {
    log_info!("engine", "Resolved WireGuard backend: {backend}");
}

pub fn wireguard_backend_fallback(reason: &str) {
    log_info!(
        "engine",
        "Kernel WireGuard unavailable: {reason}; falling back to userspace GotaTun"
    );
}

pub fn device_created() {
    log_info!("engine", "device created");
}

pub fn kernel_device_created(tun_name: &str) {
    log_info!("engine", "Created kernel WireGuard interface: {tun_name}");
}

pub fn device_configured() {
    log_info!("engine", "device configured");
}

pub fn network_configured() {
    log_info!("engine", "network configured");
}

pub fn ephemeral_negotiation_requested(quantum: bool, daita: bool) {
    log_info!(
        "engine",
        "ephemeral negotiation requested: quantum={} daita={}",
        quantum,
        daita
    );
}

pub fn ephemeral_negotiation_completed(quantum: bool, daita: bool) {
    log_info!(
        "engine",
        "ephemeral negotiation completed: quantum={} daita={}",
        quantum,
        daita
    );
}

pub fn ephemeral_negotiation_failed(message: &str) {
    log_info!("engine", "ephemeral negotiation failed: {}", message);
}

pub fn stop_requested() {
    log_info!("engine", "stop requested");
}

pub fn stop_failed(err: &impl std::fmt::Display) {
    log_warn!("engine", "stop failed: {err}");
}

pub fn device_stopped() {
    log_info!("engine", "device stopped");
}

pub fn worker_panic(message: &str) {
    log_error!("engine", "{message}");
}

pub fn panic_cleanup_failed(err: &impl std::fmt::Display) {
    log_warn!(
        "engine",
        "failed to clean up network state after backend panic: {err}"
    );
}
