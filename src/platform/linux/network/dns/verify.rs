use std::collections::HashSet;
use std::fs;
use std::net::IpAddr;
use std::path::Path;
use std::time::Duration;

use super::super::NetworkError;
use crate::log::events::dns as log_dns;

/// 严格模式：不允许出现“额外的” DNS，避免泄漏或旁路解析。
pub(super) fn verify_resolv_conf_servers(
    path: &Path,
    servers: &[IpAddr],
) -> Result<(), NetworkError> {
    let expected_v4: HashSet<IpAddr> = servers
        .iter()
        .filter(|addr| matches!(addr, IpAddr::V4(_)))
        .copied()
        .collect();
    let expected_v6: HashSet<IpAddr> = servers
        .iter()
        .filter(|addr| matches!(addr, IpAddr::V6(_)))
        .copied()
        .collect();

    let (found_v4, found_v6) = read_resolv_conf_servers(path)?;

    if expected_v4.is_empty() {
        if !found_v4.is_empty() {
            return Err(NetworkError::DnsVerifyFailed(format!(
                "unexpected IPv4 DNS: {}",
                format_ip_list(&found_v4)
            )));
        }
    } else {
        if found_v4.is_empty() {
            return Err(NetworkError::DnsVerifyFailed(
                "missing IPv4 DNS".to_string(),
            ));
        }
        let extras: Vec<IpAddr> = found_v4
            .iter()
            .filter(|addr| !expected_v4.contains(addr))
            .copied()
            .collect();
        if !extras.is_empty() {
            return Err(NetworkError::DnsVerifyFailed(format!(
                "unexpected IPv4 DNS: {}",
                format_ip_list(&extras)
            )));
        }
    }

    if expected_v6.is_empty() {
        if !found_v6.is_empty() {
            return Err(NetworkError::DnsVerifyFailed(format!(
                "unexpected IPv6 DNS: {}",
                format_ip_list(&found_v6)
            )));
        }
    } else {
        if found_v6.is_empty() {
            return Err(NetworkError::DnsVerifyFailed(
                "missing IPv6 DNS".to_string(),
            ));
        }
        let extras: Vec<IpAddr> = found_v6
            .iter()
            .filter(|addr| !expected_v6.contains(addr))
            .copied()
            .collect();
        if !extras.is_empty() {
            return Err(NetworkError::DnsVerifyFailed(format!(
                "unexpected IPv6 DNS: {}",
                format_ip_list(&extras)
            )));
        }
    }

    Ok(())
}

/// 允许系统异步写入（如 NM reapply/RA），短暂重试等待稳定态。
pub(super) async fn wait_for_resolv_conf_servers(
    path: &Path,
    servers: &[IpAddr],
) -> Result<(), NetworkError> {
    let mut last_error: Option<NetworkError> = None;
    for _ in 0..5 {
        match verify_resolv_conf_servers(path, servers) {
            Ok(()) => return Ok(()),
            Err(err @ NetworkError::DnsVerifyFailed(_)) => {
                last_error = Some(err);
            }
            Err(err) => return Err(err),
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    Err(last_error.unwrap_or(NetworkError::DnsVerifyFailed(
        "dns verify timeout".to_string(),
    )))
}

pub(super) fn log_resolv_conf_snapshot(path: &Path, label: &str) {
    // resolv.conf 里的 nameserver 可能包含 zone index（%eth0），读时剥离。
    match read_resolv_conf_servers(path) {
        Ok((v4, v6)) => {
            log_dns::resolv_conf_snapshot(label, &v4, &v6);
        }
        Err(err) => {
            log_dns::resolv_conf_snapshot_failed(label, &err);
        }
    }
}

/// 只解析 nameserver 行；忽略注释与空行。
pub(super) fn read_resolv_conf_servers(
    path: &Path,
) -> Result<(Vec<IpAddr>, Vec<IpAddr>), NetworkError> {
    let contents = fs::read_to_string(path)?;
    let mut v4 = Vec::new();
    let mut v6 = Vec::new();

    for line in contents.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if !line.starts_with("nameserver") {
            continue;
        }
        let mut parts = line.split_whitespace();
        let _ = parts.next();
        let Some(value) = parts.next() else {
            continue;
        };
        // IPv6 可能带有 scope（如 fe80::1%enp0s3），需去掉 % 后再解析。
        let ip_str = value.split('%').next().unwrap_or(value);
        let Ok(addr) = ip_str.parse::<IpAddr>() else {
            return Err(NetworkError::DnsVerifyFailed(format!(
                "unparseable DNS entry: {value}"
            )));
        };
        match addr {
            IpAddr::V4(_) => v4.push(addr),
            IpAddr::V6(_) => v6.push(addr),
        }
    }

    Ok((v4, v6))
}

pub(super) fn format_ip_list(addrs: &[IpAddr]) -> String {
    addrs
        .iter()
        .map(|addr| addr.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}
