use std::net::IpAddr;
use std::path::Path;
use std::time::Duration;

use super::super::super::NetworkError;
use super::super::command::{run_cmd, run_cmd_capture};
use super::super::types::{DnsBackend, DnsState, NmConnectionState};
use super::super::verify::{log_resolv_conf_snapshot, wait_for_resolv_conf_servers};
use crate::log::events::dns as log_dns;

pub(in crate::platform::linux::network::dns) async fn apply_network_manager(
    nmcli: &Path,
    servers: &[IpAddr],
    search: &[String],
) -> Result<DnsState, NetworkError> {
    // 只处理 NM 管理的“活动”连接；无活动连接则认为不可用。
    let connections = nmcli_active_connections(nmcli).await?;
    if connections.is_empty() {
        return Err(NetworkError::DnsNotSupported);
    }

    let (v4_dns, v6_dns) = split_dns_servers(servers);
    let v4_dns = v4_dns.join(",");
    let v6_dns = v6_dns.join(",");
    let search_value = search.join(",");

    // 逐个连接应用 DNS，并保存原状态用于回滚。
    let mut touched = Vec::new();
    for conn in connections {
        let state = read_nm_connection_state(nmcli, &conn.name, &conn.device).await?;
        if let Err(err) = apply_nm_connection(
            nmcli,
            &conn.name,
            &conn.device,
            &v4_dns,
            &v6_dns,
            &search_value,
        )
        .await
        {
            for restored in touched.iter().rev() {
                let _ = restore_nm_connection(nmcli, restored).await;
            }
            return Err(err);
        }
        touched.push(state);
    }

    // 记录 NM 与 resolv.conf 的即时状态，便于判断是否成功写入。
    log_nmcli_dns_snapshot(nmcli, &touched, "post-apply").await;
    log_resolv_conf_snapshot(Path::new("/etc/resolv.conf"), "post-apply");
    tokio::time::sleep(Duration::from_millis(800)).await;
    log_nmcli_dns_snapshot(nmcli, &touched, "after-wait").await;
    log_resolv_conf_snapshot(Path::new("/etc/resolv.conf"), "after-wait");

    if let Err(err) = wait_for_resolv_conf_servers(Path::new("/etc/resolv.conf"), servers).await {
        let mut final_err = err;
        if matches!(final_err, NetworkError::DnsVerifyFailed(_)) {
            // reapply 有时无法清掉 RDNSS 注入（如 fe80::1），尝试 down/up 强制刷新。
            log_dns::nmcli_verify_failed();
            if let Err(err) = nmcli_reconnect(nmcli, &touched).await {
                log_dns::nmcli_reconnect_failed(&err);
            } else {
                // 重新连接后再次采样并验证，确认是否仍残留意外 DNS。
                log_nmcli_dns_snapshot(nmcli, &touched, "post-reconnect").await;
                log_resolv_conf_snapshot(Path::new("/etc/resolv.conf"), "post-reconnect");
                tokio::time::sleep(Duration::from_millis(1200)).await;
                log_nmcli_dns_snapshot(nmcli, &touched, "after-reconnect-wait").await;
                log_resolv_conf_snapshot(Path::new("/etc/resolv.conf"), "after-reconnect-wait");
                match wait_for_resolv_conf_servers(Path::new("/etc/resolv.conf"), servers).await {
                    Ok(()) => {
                        return Ok(DnsState {
                            backend: DnsBackend::NetworkManager {
                                connections: touched,
                            },
                        });
                    }
                    Err(err) => {
                        final_err = err;
                    }
                }
            }
        }
        cleanup_network_manager(nmcli, &touched).await;
        return Err(final_err);
    }

    Ok(DnsState {
        backend: DnsBackend::NetworkManager {
            connections: touched,
        },
    })
}

pub(in crate::platform::linux::network::dns) async fn cleanup_network_manager(
    nmcli: &Path,
    connections: &[NmConnectionState],
) {
    for conn in connections {
        log_dns::nmcli_revert(&conn.name);
        if let Err(err) = restore_nm_connection(nmcli, conn).await {
            log_dns::nmcli_revert_failed(&err);
        }
    }
}

async fn nmcli_active_connections(nmcli: &Path) -> Result<Vec<NmConnection>, NetworkError> {
    // 只筛选 activated 且非 loopback/tun/vpn 的连接，避免误改隧道自身。
    let args = vec![
        "-t".to_string(),
        "-f".to_string(),
        "NAME,DEVICE,TYPE,STATE".to_string(),
        "connection".to_string(),
        "show".to_string(),
        "--active".to_string(),
    ];
    let output = run_cmd_capture(nmcli, &args).await?;
    let mut connections = Vec::new();
    for line in output.lines() {
        let mut parts = line.splitn(4, ':');
        let name = parts.next().unwrap_or("").trim();
        let device = parts.next().unwrap_or("").trim();
        let kind = parts.next().unwrap_or("").trim();
        let state = parts.next().unwrap_or("").trim();
        if name.is_empty() || device.is_empty() || device == "--" {
            continue;
        }
        if state != "activated" {
            continue;
        }
        let kind_lower = kind.to_ascii_lowercase();
        if kind_lower.contains("loopback")
            || kind_lower.contains("tun")
            || kind_lower.contains("wireguard")
            || kind_lower.contains("vpn")
        {
            continue;
        }
        connections.push(NmConnection {
            name: name.to_string(),
            device: device.to_string(),
        });
    }
    Ok(connections)
}

struct NmConnection {
    name: String,
    device: String,
}

async fn read_nm_connection_state(
    nmcli: &Path,
    name: &str,
    device: &str,
) -> Result<NmConnectionState, NetworkError> {
    // 读取连接现有 DNS 设置，供失败时回滚。
    let ipv4_dns = nmcli_get(nmcli, name, "ipv4.dns").await?;
    let ipv4_ignore_auto =
        normalize_nmcli_bool(nmcli_get(nmcli, name, "ipv4.ignore-auto-dns").await?);
    let ipv4_search = nmcli_get(nmcli, name, "ipv4.dns-search").await?;
    let ipv4_priority = nmcli_get(nmcli, name, "ipv4.dns-priority").await?;
    let ipv6_dns = nmcli_get(nmcli, name, "ipv6.dns").await?;
    let ipv6_ignore_auto =
        normalize_nmcli_bool(nmcli_get(nmcli, name, "ipv6.ignore-auto-dns").await?);
    let ipv6_search = nmcli_get(nmcli, name, "ipv6.dns-search").await?;
    let ipv6_priority = nmcli_get(nmcli, name, "ipv6.dns-priority").await?;

    Ok(NmConnectionState {
        name: name.to_string(),
        device: device.to_string(),
        ipv4_dns,
        ipv4_ignore_auto,
        ipv4_search,
        ipv4_priority,
        ipv6_dns,
        ipv6_ignore_auto,
        ipv6_search,
        ipv6_priority,
    })
}

async fn nmcli_get(nmcli: &Path, name: &str, field: &str) -> Result<String, NetworkError> {
    let args = vec![
        "-g".to_string(),
        field.to_string(),
        "connection".to_string(),
        "show".to_string(),
        name.to_string(),
    ];
    let output = run_cmd_capture(nmcli, &args).await?;
    Ok(output.trim().to_string())
}

fn normalize_nmcli_bool(value: String) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "no".to_string()
    } else {
        trimmed.to_string()
    }
}

async fn apply_nm_connection(
    nmcli: &Path,
    name: &str,
    device: &str,
    v4_dns: &str,
    v6_dns: &str,
    search: &str,
) -> Result<(), NetworkError> {
    // ignore-auto-dns=yes + dns-priority=-42 让 NM 以手动 DNS 为“独占”优先级。
    let args = vec![
        "connection".to_string(),
        "modify".to_string(),
        name.to_string(),
        "ipv4.dns".to_string(),
        v4_dns.to_string(),
        "ipv4.ignore-auto-dns".to_string(),
        "yes".to_string(),
        "ipv4.dns-search".to_string(),
        search.to_string(),
        "ipv4.dns-priority".to_string(),
        "-42".to_string(),
        "ipv6.dns".to_string(),
        v6_dns.to_string(),
        "ipv6.ignore-auto-dns".to_string(),
        "yes".to_string(),
        "ipv6.dns-search".to_string(),
        search.to_string(),
        "ipv6.dns-priority".to_string(),
        "-42".to_string(),
    ];
    run_cmd(nmcli, &args).await?;
    nmcli_reapply(nmcli, device).await
}

async fn restore_nm_connection(
    nmcli: &Path,
    state: &NmConnectionState,
) -> Result<(), NetworkError> {
    // 用之前保存的值还原，尽量避免破坏用户网络配置。
    let args = vec![
        "connection".to_string(),
        "modify".to_string(),
        state.name.to_string(),
        "ipv4.dns".to_string(),
        state.ipv4_dns.clone(),
        "ipv4.ignore-auto-dns".to_string(),
        state.ipv4_ignore_auto.clone(),
        "ipv4.dns-search".to_string(),
        state.ipv4_search.clone(),
        "ipv4.dns-priority".to_string(),
        state.ipv4_priority.clone(),
        "ipv6.dns".to_string(),
        state.ipv6_dns.clone(),
        "ipv6.ignore-auto-dns".to_string(),
        state.ipv6_ignore_auto.clone(),
        "ipv6.dns-search".to_string(),
        state.ipv6_search.clone(),
        "ipv6.dns-priority".to_string(),
        state.ipv6_priority.clone(),
    ];
    run_cmd(nmcli, &args).await?;
    nmcli_reapply(nmcli, &state.device).await
}

async fn nmcli_reapply(nmcli: &Path, device: &str) -> Result<(), NetworkError> {
    // reapply 不会完全断网，但某些情况下不会清掉 RA 注入的 DNS。
    let args = vec![
        "device".to_string(),
        "reapply".to_string(),
        device.to_string(),
    ];
    run_cmd(nmcli, &args).await
}

async fn nmcli_reconnect(
    nmcli: &Path,
    connections: &[NmConnectionState],
) -> Result<(), NetworkError> {
    // down/up 会短暂断网，但可强制 NM 重新应用连接设置。
    for conn in connections {
        let args = vec![
            "connection".to_string(),
            "down".to_string(),
            conn.name.clone(),
        ];
        run_cmd(nmcli, &args).await?;
    }
    for conn in connections {
        let args = vec![
            "connection".to_string(),
            "up".to_string(),
            conn.name.clone(),
            "ifname".to_string(),
            conn.device.clone(),
        ];
        run_cmd(nmcli, &args).await?;
    }
    Ok(())
}

fn split_dns_servers(servers: &[IpAddr]) -> (Vec<String>, Vec<String>) {
    // 分离 IPv4/IPv6，便于 nmcli 字段写入。
    let mut v4 = Vec::new();
    let mut v6 = Vec::new();
    for server in servers {
        match server {
            IpAddr::V4(addr) => v4.push(addr.to_string()),
            IpAddr::V6(addr) => v6.push(addr.to_string()),
        }
    }
    (v4, v6)
}

async fn log_nmcli_dns_snapshot(nmcli: &Path, connections: &[NmConnectionState], label: &str) {
    // 辅助日志：用于判断 NM 是否已写入期望 DNS。
    for state in connections {
        let args = vec![
            "-f".to_string(),
            "IP4.DNS,IP6.DNS,IP4.DOMAIN,IP6.DOMAIN".to_string(),
            "device".to_string(),
            "show".to_string(),
            state.device.clone(),
        ];
        match run_cmd_capture(nmcli, &args).await {
            Ok(output) => {
                let output = output.trim();
                if output.is_empty() {
                    log_dns::nmcli_snapshot_empty(label, &state.device);
                } else {
                    log_dns::nmcli_snapshot(label, &state.device, output);
                }
            }
            Err(err) => {
                log_dns::nmcli_snapshot_failed(label, &state.device, &err);
            }
        }
    }
}
