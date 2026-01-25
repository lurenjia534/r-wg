//! Windows 平台网络配置入口。
//!
//! 职责：
//! - TUN 接口地址与路由配置；
//! - Endpoint 绕过路由（避免全隧道阻断握手）；
//! - DNS 配置与回滚（按接口级设置）。

mod adapter;
mod addresses;
mod dns;
mod metrics;
mod routes;
mod sockaddr;

use std::fmt;

use windows::core::PWSTR;
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, ERROR_OBJECT_ALREADY_EXISTS, WIN32_ERROR};
use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6};

use crate::backend::wg::config::{InterfaceAddress, InterfaceConfig, PeerConfig, RouteTable};
use crate::log;

use adapter::AdapterInfo;
use addresses::{add_unicast_address, cleanup_stale_unicast_addresses, delete_unicast_address};
use dns::{apply_dns, cleanup_dns, DnsState};
use metrics::{restore_interface_metric, set_interface_metric, InterfaceMetricState};
use routes::{
    add_route, best_route_to, collect_allowed_routes, delete_route, detect_full_tunnel,
    is_default_route, resolve_endpoint_ips, RouteEntry,
};

/// 隧道接口默认 metric，值越小优先级越高。
const TUNNEL_METRIC: u32 = 5;

pub struct AppliedNetworkState {
    /// 本次应用的接口名称（用于日志与清理）。
    tun_name: String,
    /// 适配器信息（ifIndex/LUID/GUID）。
    adapter: AdapterInfo,
    /// 本次写入的地址列表。
    addresses: Vec<InterfaceAddress>,
    /// 本次写入的路由列表（AllowedIPs）。
    routes: Vec<RouteEntry>,
    /// Endpoint 绕过路由。
    bypass_routes: Vec<RouteEntry>,
    /// 接口 metric 的原始状态，用于恢复。
    iface_metrics: Vec<InterfaceMetricState>,
    /// DNS 修改状态（用于回滚）。
    dns: Option<DnsState>,
}

#[derive(Debug)]
pub enum NetworkError {
    AdapterNotFound(String),
    EndpointResolve(String),
    Io(std::io::Error),
    Win32 {
        context: &'static str,
        code: WIN32_ERROR,
    },
}

impl fmt::Display for NetworkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetworkError::AdapterNotFound(name) => {
                write!(f, "adapter not found: {name}")
            }
            NetworkError::EndpointResolve(message) => {
                write!(f, "endpoint resolve failed: {message}")
            }
            NetworkError::Io(err) => write!(f, "io error: {err}"),
            NetworkError::Win32 { context, code } => {
                let err = std::io::Error::from_raw_os_error(code.0 as i32);
                write!(f, "{context}: {err} (code={})", code.0)
            }
        }
    }
}

impl std::error::Error for NetworkError {}

impl From<std::io::Error> for NetworkError {
    fn from(err: std::io::Error) -> Self {
        NetworkError::Io(err)
    }
}

pub async fn apply_network_config(
    tun_name: &str,
    interface: &InterfaceConfig,
    peers: &[PeerConfig],
) -> Result<AppliedNetworkState, NetworkError> {
    // 1) 记录基本参数，便于诊断。
    log_net(format!(
        "apply: tun={tun_name} addr_count={} dns_servers={} dns_search={}",
        interface.addresses.len(),
        interface.dns_servers.len(),
        interface.dns_search.len()
    ));

    if let Some(RouteTable::Id(id)) = interface.table {
        log_net(format!("route table id ignored on windows: {id}"));
    }
    if interface.fwmark.is_some() {
        log_net("fwmark ignored on windows".to_string());
    }
    // 2) 解析并定位目标适配器。
    let adapter = adapter::find_adapter_with_retry(tun_name).await?;

    let mut state = AppliedNetworkState {
        tun_name: tun_name.to_string(),
        adapter,
        addresses: Vec::new(),
        routes: Vec::new(),
        bypass_routes: Vec::new(),
        iface_metrics: Vec::new(),
        dns: None,
    };

    // 3) 清理历史残留地址，避免影响路由决策。
    cleanup_stale_unicast_addresses(adapter, &interface.addresses)?;

    // 4) 写入接口地址（IPv4/IPv6）。
    for address in &interface.addresses {
        log_net(format!("address add: {}/{}", address.addr, address.cidr));
        if let Err(err) = add_unicast_address(adapter, address) {
            let _ = cleanup_network_config(state).await;
            return Err(err);
        }
        state.addresses.push(address.clone());
    }

    // 5) 汇总 AllowedIPs，并判断是否为全隧道。
    let routes = collect_allowed_routes(peers);
    let (full_v4, full_v6) = detect_full_tunnel(&routes);

    // 6) 全隧道时降低接口 metric，以抢占默认路由优先级。
    if interface.table != Some(RouteTable::Off) {
        if full_v4 {
            match set_interface_metric(adapter, AF_INET, TUNNEL_METRIC) {
                Ok(metric_state) => {
                    log_net(format!("interface metric set: v4 metric={}", TUNNEL_METRIC));
                    state.iface_metrics.push(metric_state);
                }
                Err(err) => log_net(format!("interface metric set failed (v4): {err}")),
            }
        }
        if full_v6 {
            match set_interface_metric(adapter, AF_INET6, TUNNEL_METRIC) {
                Ok(metric_state) => {
                    log_net(format!("interface metric set: v6 metric={}", TUNNEL_METRIC));
                    state.iface_metrics.push(metric_state);
                }
                Err(err) => log_net(format!("interface metric set failed (v6): {err}")),
            }
        }
    }

    let mut bypass_routes = Vec::new();
    let mut endpoint_v4 = 0usize;
    let mut endpoint_v6 = 0usize;
    let mut bypass_v4 = 0usize;
    let mut bypass_v6 = 0usize;
    // 7) 全隧道场景下，为 Endpoint 生成绕过路由。
    if interface.table != Some(RouteTable::Off) && (full_v4 || full_v6) {
        let endpoint_ips = resolve_endpoint_ips(peers).await?;
        for ip in endpoint_ips {
            if ip.is_ipv4() {
                endpoint_v4 += 1;
                if !full_v4 {
                    continue;
                }
            } else {
                endpoint_v6 += 1;
                if !full_v6 {
                    continue;
                }
            }
            match best_route_to(ip) {
                Ok(route) => {
                    log_net(format!(
                        "bypass route add: {} via {:?} if_index={}",
                        route.dest, route.next_hop, route.if_index
                    ));
                    if ip.is_ipv4() {
                        bypass_v4 += 1;
                    } else {
                        bypass_v6 += 1;
                    }
                    bypass_routes.push(route);
                }
                Err(err) => log_net(format!("bypass route failed for {ip}: {err}")),
            }
        }
    }

    let allow_v4_default = !(full_v4 && endpoint_v4 > 0 && bypass_v4 == 0);
    let allow_v6_default = !(full_v6 && endpoint_v6 > 0 && bypass_v6 == 0);
    if !allow_v4_default {
        log_net("skip IPv4 default route: no bypass route for endpoint".to_string());
    }
    if !allow_v6_default {
        log_net("skip IPv6 default route: no bypass route for endpoint".to_string());
    }

    // 8) 写入 AllowedIPs 路由（必要时跳过默认路由以避免断网）。
    if interface.table != Some(RouteTable::Off) {
        for route in routes {
            if is_default_route(&route) {
                if route.addr.is_ipv4() && !allow_v4_default {
                    continue;
                }
                if route.addr.is_ipv6() && !allow_v6_default {
                    continue;
                }
            }
            let entry = RouteEntry {
                dest: route.addr,
                prefix: route.cidr,
                next_hop: None,
                if_index: adapter.if_index,
                luid: adapter.luid,
            };
            log_net(format!(
                "route add: {}/{} via {:?} if_index={} metric={}",
                entry.dest, entry.prefix, entry.next_hop, entry.if_index, TUNNEL_METRIC
            ));
            if let Err(err) = add_route(&entry) {
                let _ = cleanup_network_config(state).await;
                return Err(err);
            }
            state.routes.push(entry);
        }

        for entry in bypass_routes {
            match add_route(&entry) {
                Ok(()) => state.bypass_routes.push(entry),
                Err(err) => log_net(format!("bypass route add failed for {}: {err}", entry.dest)),
            }
        }
    }

    // 9) 写入 DNS 设置，失败视为致命错误并回滚。
    if !interface.dns_servers.is_empty() || !interface.dns_search.is_empty() {
        log_net(format!(
            "dns: servers={} search={}",
            interface.dns_servers.len(),
            interface.dns_search.len()
        ));
        match apply_dns(adapter, &interface.dns_servers, &interface.dns_search) {
            Ok(dns_state) => state.dns = Some(dns_state),
            Err(err) => {
                log_net(format!("dns apply failed: {err}"));
                let _ = cleanup_network_config(state).await;
                return Err(err);
            }
        }
    }

    Ok(state)
}

pub async fn cleanup_network_config(state: AppliedNetworkState) -> Result<(), NetworkError> {
    // 回滚顺序：先路由、后地址、再 metric/DNS，避免残留影响系统网络。
    log_net(format!(
        "cleanup: tun={} addr_count={} route_count={} bypass_count={}",
        state.tun_name,
        state.addresses.len(),
        state.routes.len(),
        state.bypass_routes.len()
    ));

    // 先删 bypass routes，避免后续默认路由清理影响出口。
    for entry in state.bypass_routes.iter().rev() {
        if let Err(err) = delete_route(entry) {
            log_net(format!("bypass route del failed: {err}"));
        }
    }

    // 再删普通路由。
    for entry in state.routes.iter().rev() {
        if let Err(err) = delete_route(entry) {
            log_net(format!("route del failed: {err}"));
        }
    }

    // 删除接口地址。
    for address in &state.addresses {
        if let Err(err) = delete_unicast_address(state.adapter, address) {
            log_net(format!("address del failed: {err}"));
        }
    }

    // 恢复接口 metric。
    for iface in state.iface_metrics.iter().rev() {
        if let Err(err) = restore_interface_metric(state.adapter, *iface) {
            log_net(format!("interface metric restore failed: {err}"));
        }
    }

    // 回滚 DNS 设置。
    if let Some(dns) = state.dns {
        log_net("dns revert".to_string());
        if let Err(err) = cleanup_dns(dns) {
            log_net(format!("dns revert failed: {err}"));
        }
    }

    Ok(())
}

fn log_net(message: String) {
    // 统一网络层日志出口，便于筛选分析。
    log::log("net", message);
}

fn pwstr_to_string(ptr: PWSTR) -> String {
    // 将 Windows 宽字符串指针转换为 Rust String。
    if ptr.0.is_null() {
        return String::new();
    }
    unsafe {
        let mut len = 0usize;
        let mut cursor = ptr.0;
        while *cursor != 0 {
            len += 1;
            cursor = cursor.add(1);
        }
        let slice = std::slice::from_raw_parts(ptr.0, len);
        String::from_utf16_lossy(slice)
    }
}

fn is_already_exists(code: WIN32_ERROR) -> bool {
    // Windows 对“已存在”错误有多个返回值。
    code == ERROR_OBJECT_ALREADY_EXISTS || code == ERROR_ALREADY_EXISTS
}
