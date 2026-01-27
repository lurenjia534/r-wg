// DNS 相关事件日志（scope = dns）。
use std::fmt;
use std::net::IpAddr;
use std::path::Path;

use crate::log_info;

pub fn backend_order(order: &str) {
    log_info!("dns", "dns backend order: {order}");
}

pub fn backend_not_found(name: &str) {
    log_info!("dns", "dns {name} not found");
}

pub fn backend_selected(name: &str, path: &Path) {
    log_info!("dns", "dns backend: {name} ({})", path.display());
}

pub fn backend_failed(name: &str, err: &impl fmt::Display) {
    log_info!("dns", "dns {name} failed: {err}");
}

pub fn resolv_conf_failed(err: &impl fmt::Display) {
    log_info!("dns", "dns resolv.conf failed: {err}");
}

pub fn apply_summary(servers: usize, search: usize) {
    log_info!("dns", "dns: servers={} search={}", servers, search);
}

pub fn apply_failed(err: &impl fmt::Display) {
    log_info!("dns", "dns apply failed: {err}");
}

pub fn revert_start() {
    log_info!("dns", "dns revert");
}

pub fn revert_failed(err: &impl fmt::Display) {
    log_info!("dns", "dns revert failed: {err}");
}

pub fn resolvectl_revert(path: &Path) {
    log_info!("dns", "dns revert: resolvectl ({})", path.display());
}

pub fn resolvconf_revert(path: &Path) {
    log_info!("dns", "dns revert: resolvconf ({})", path.display());
}

pub fn nmcli_revert(connection: &str) {
    log_info!("dns", "dns revert: nmcli connection={}", connection);
}

pub fn nmcli_revert_failed(err: &impl fmt::Display) {
    log_info!("dns", "dns nmcli revert failed: {err}");
}

pub fn resolv_conf_revert(path: &Path) {
    log_info!("dns", "dns revert: resolv.conf ({})", path.display());
}

pub fn exec(command: &str) {
    log_info!("dns", "exec: {command}");
}

pub fn resolv_conf_symlink(target: Option<&Path>) {
    match target {
        Some(path) => {
            log_info!("dns", "dns resolv.conf: symlink -> {}", path.display());
        }
        None => {
            log_info!("dns", "dns resolv.conf: symlink (target unknown)");
        }
    }
}

pub fn resolv_conf_regular() {
    log_info!("dns", "dns resolv.conf: regular file");
}

pub fn nmcli_verify_failed() {
    log_info!("dns", "dns nmcli verify failed, attempting reconnect");
}

pub fn nmcli_reconnect_failed(err: &impl fmt::Display) {
    log_info!("dns", "dns nmcli reconnect failed: {err}");
}

pub fn resolv_conf_skipped_symlink() {
    log_info!("dns", "dns resolv.conf skipped: symlink");
}

pub fn nmcli_snapshot_empty(label: &str, device: &str) {
    log_info!("dns", "dns nmcli {label}: device={} (empty)", device);
}

pub fn nmcli_snapshot(label: &str, device: &str, output: &str) {
    log_info!("dns", "dns nmcli {label}: device={}\n{}", device, output);
}

pub fn nmcli_snapshot_failed(label: &str, device: &str, err: &impl fmt::Display) {
    log_info!(
        "dns",
        "dns nmcli {label} failed: device={} err={err}",
        device
    );
}

pub fn resolv_conf_snapshot(label: &str, v4: &[IpAddr], v6: &[IpAddr]) {
    log_info!(
        "dns",
        "dns resolv.conf {label}: v4=[{}] v6=[{}]",
        format_ip_list(v4),
        format_ip_list(v6)
    );
}

pub fn resolv_conf_snapshot_failed(label: &str, err: &impl fmt::Display) {
    log_info!("dns", "dns resolv.conf {label} read failed: {err}");
}

pub fn apply_retry_fallback_guid() {
    log_info!("dns", "dns apply retry with fallback guid");
}

pub fn settings_not_found() {
    log_info!(
        "dns",
        "dns settings not found for interface, assuming empty"
    );
}

fn format_ip_list(values: &[IpAddr]) -> String {
    values
        .iter()
        .map(|ip| ip.to_string())
        .collect::<Vec<_>>()
        .join(",")
}
