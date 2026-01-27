//! 网络相关日志与诊断输出。
//!
//! 说明：
//! - 日志仅在 `log::enabled_for(LogLevel::Debug, "net")` 时生效，避免无谓 IO。
//! - 默认路由与权限信息用于现场排障，不影响主逻辑。

use std::net::{Ipv4Addr, Ipv6Addr};

use crate::log::events::net as log_net;
use crate::log::{self, LogLevel};

/// 打印系统默认路由（IPv4 / IPv6）。
///
/// 说明：直接读取 /proc，避免依赖外部命令，便于在最小环境下排查问题。
pub(super) fn log_default_routes() {
    if !log::enabled_for(LogLevel::Debug, "net") {
        return;
    }

    match std::fs::read_to_string("/proc/net/route") {
        Ok(contents) => {
            let mut found = false;
            for (idx, line) in contents.lines().enumerate() {
                if idx == 0 {
                    continue;
                }
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() < 8 {
                    continue;
                }
                let iface = parts[0];
                let destination = parts[1];
                let gateway = parts[2];
                let metric = parts[6];
                if destination == "00000000" {
                    let gw = parse_ipv4_hex_le(gateway)
                        .map(|addr| addr.to_string())
                        .unwrap_or_else(|| "-".to_string());
                    log_net::default_route_v4(iface, &gw, metric);
                    found = true;
                }
            }
            if !found {
                log_net::default_route_v4_not_found();
            }
        }
        Err(err) => {
            log_net::default_route_v4_read_failed(&err);
        }
    }

    match std::fs::read_to_string("/proc/net/ipv6_route") {
        Ok(contents) => {
            let mut found = false;
            for line in contents.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() < 10 {
                    continue;
                }
                let destination = parts[0];
                let prefix = parts[1];
                let gateway = parts[4];
                let metric = parts[5];
                let iface = parts[9];
                if destination == "00000000000000000000000000000000" && prefix == "00000000" {
                    let gw = parse_ipv6_hex(gateway)
                        .map(|addr| addr.to_string())
                        .unwrap_or_else(|| "-".to_string());
                    let metric = u32::from_str_radix(metric, 16)
                        .map(|value| value.to_string())
                        .unwrap_or_else(|_| metric.to_string());
                    log_net::default_route_v6(iface, &gw, &metric);
                    found = true;
                }
            }
            if !found {
                log_net::default_route_v6_not_found();
            }
        }
        Err(err) => {
            log_net::default_route_v6_read_failed(&err);
        }
    }
}

pub(super) fn log_privileges() {
    // 读取 /proc/self/status 判断是否具备 CAP_NET_ADMIN。
    if !log::enabled_for(LogLevel::Debug, "net") {
        return;
    }

    let status = match std::fs::read_to_string("/proc/self/status") {
        Ok(status) => status,
        Err(err) => {
            log_net::proc_status_read_failed(&err);
            return;
        }
    };

    let euid = parse_status_uid(&status);
    let cap_eff = parse_status_cap_eff(&status);
    match (euid, cap_eff) {
        (Some(euid), Some(cap_eff)) => {
            let cap_net_admin = 1u64 << 12;
            let has_net_admin = (cap_eff & cap_net_admin) != 0;
            log_net::proc_status_capabilities(euid, cap_eff, has_net_admin);
        }
        _ => {
            log_net::proc_status_parse_failed();
        }
    }
}

fn parse_ipv4_hex_le(hex: &str) -> Option<Ipv4Addr> {
    // /proc/net/route 使用小端十六进制表示网关地址。
    let value = u32::from_str_radix(hex, 16).ok()?;
    Some(Ipv4Addr::from(value.to_le_bytes()))
}

fn parse_ipv6_hex(hex: &str) -> Option<Ipv6Addr> {
    // /proc/net/ipv6_route 使用 32 字节十六进制表示地址。
    if hex.len() != 32 {
        return None;
    }
    let mut bytes = [0u8; 16];
    for idx in 0..16 {
        let start = idx * 2;
        let chunk = &hex[start..start + 2];
        bytes[idx] = u8::from_str_radix(chunk, 16).ok()?;
    }
    Some(Ipv6Addr::from(bytes))
}

fn parse_status_uid(status: &str) -> Option<u32> {
    status
        .lines()
        .find(|line| line.starts_with("Uid:"))
        .and_then(|line| line.split_whitespace().nth(2))
        .and_then(|value| value.parse().ok())
}

fn parse_status_cap_eff(status: &str) -> Option<u64> {
    status
        .lines()
        .find(|line| line.starts_with("CapEff:"))
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| u64::from_str_radix(value, 16).ok())
}
