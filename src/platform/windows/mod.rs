//! Windows 网络配置入口。
//!
//! 主要职责：
//! - 配置 TUN 接口地址与 AllowedIPs 路由；
//! - 在全隧道时生成并下发 Endpoint bypass route，避免握手被自身默认路由截断；
//! - 应用 DNS、NRPT 与 DNS 防泄露规则；
//! - 在失败或断开时按顺序回滚。

mod adapter;
mod addresses;
mod dns;
mod firewall;
mod metrics;
mod nrpt;
mod recovery;
mod routes;
mod sockaddr;

use std::fmt;

use windows::core::PWSTR;
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, ERROR_OBJECT_ALREADY_EXISTS, WIN32_ERROR};
use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6};

use crate::backend::wg::config::{InterfaceAddress, InterfaceConfig, PeerConfig, RouteTable};
use crate::log::events::{dns as log_dns, net as log_net};

use adapter::AdapterInfo;
use addresses::{add_unicast_address, cleanup_stale_unicast_addresses, delete_unicast_address};
use dns::{apply_dns, cleanup_dns, DnsState};
use firewall::{apply_dns_guard, cleanup_dns_guard, DnsGuardState};
use metrics::{restore_interface_metric, set_interface_metric, InterfaceMetricState};
use nrpt::{apply_nrpt_guard, cleanup_nrpt_guard, NrptState};
use recovery::RecoveryGuard;
use routes::{
    add_route, best_route_to, collect_allowed_routes, delete_route, detect_full_tunnel,
    resolve_endpoint_ips, RouteEntry,
};

/// 隧道接口 metric。值越小优先级越高。
const TUNNEL_METRIC: u32 = 0;

/// 一次网络配置应用后的状态，用于后续清理回滚。
pub struct AppliedNetworkState {
    /// 隧道接口名（用于日志和清理）。
    tun_name: String,
    /// 目标网卡关键信息。
    adapter: AdapterInfo,
    /// 本次添加的接口地址。
    addresses: Vec<InterfaceAddress>,
    /// 本次添加的普通路由（AllowedIPs + DNS host route）。
    routes: Vec<RouteEntry>,
    /// 本次添加的 Endpoint bypass 路由。
    bypass_routes: Vec<RouteEntry>,
    /// metric 调整前快照（用于恢复）。
    iface_metrics: Vec<InterfaceMetricState>,
    /// DNS 变更状态（用于回滚）。
    dns: Option<DnsState>,
    /// NRPT 变更状态（用于回滚）。
    nrpt: Option<NrptState>,
    /// DNS Guard 状态（用于回滚）。
    dns_guard: Option<DnsGuardState>,
    /// 持久化恢复日志。
    recovery: Option<RecoveryGuard>,
}

#[derive(Debug)]
pub enum NetworkError {
    AdapterNotFound(String),
    EndpointResolve(String),
    /// 用于 fail-closed：检测到潜在泄露风险时主动失败。
    UnsafeRouting(String),
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
            NetworkError::UnsafeRouting(message) => {
                write!(f, "unsafe routing configuration: {message}")
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

/// 应用 Windows 侧网络配置。
///
/// 注意：任何关键步骤失败都会触发回滚，避免留下“半配置”状态。
pub async fn apply_network_config(
    tun_name: &str,
    interface: &InterfaceConfig,
    peers: &[PeerConfig],
) -> Result<AppliedNetworkState, NetworkError> {
    // 1) 记录本次配置参数，便于问题排查。
    log_net::apply_windows(
        tun_name,
        interface.addresses.len(),
        interface.dns_servers.len(),
        interface.dns_search.len(),
    );

    if let Some(RouteTable::Id(id)) = interface.table {
        log_net::route_table_id_ignored(id);
    }
    if interface.fwmark.is_some() {
        log_net::fwmark_ignored();
    }

    // 2) 查找目标 TUN 适配器。
    let adapter = adapter::find_adapter_with_retry(tun_name).await?;

    let mut state = AppliedNetworkState {
        tun_name: tun_name.to_string(),
        adapter,
        addresses: Vec::new(),
        routes: Vec::new(),
        bypass_routes: Vec::new(),
        iface_metrics: Vec::new(),
        dns: None,
        nrpt: None,
        dns_guard: None,
        recovery: Some(RecoveryGuard::begin(tun_name, adapter)?),
    };

    // 3) 先清理旧地址，避免历史残留影响路由决策。
    cleanup_stale_unicast_addresses(adapter, &interface.addresses)?;

    // 4) 配置接口地址。
    for address in &interface.addresses {
        log_net::address_add_windows(address.addr, address.cidr);
        if let Err(err) = add_unicast_address(adapter, address) {
            let _ = cleanup_network_config(state).await;
            return Err(err);
        }
        state.addresses.push(address.clone());
        if let Some(recovery) = state.recovery.as_mut() {
            recovery.record_address(address)?;
        }
    }

    // 5) 汇总路由并判断是否为全隧道。
    let routes = collect_allowed_routes(peers);
    let (full_v4, full_v6) = detect_full_tunnel(&routes);

    // 6) 全隧道时下调接口 metric。
    // 这里使用 fail-closed：设置失败则直接回滚并退出。
    if interface.table != Some(RouteTable::Off) {
        if full_v4 {
            match set_interface_metric(adapter, AF_INET, TUNNEL_METRIC) {
                Ok(metric_state) => {
                    log_net::interface_metric_set_v4(TUNNEL_METRIC);
                    state.iface_metrics.push(metric_state);
                    if let Some(recovery) = state.recovery.as_mut() {
                        recovery.record_metric(metric_state)?;
                    }
                }
                Err(err) => {
                    log_net::interface_metric_set_failed_v4(&err);
                    let _ = cleanup_network_config(state).await;
                    return Err(NetworkError::UnsafeRouting(format!(
                        "failed to set IPv4 interface metric for full tunnel: {err}"
                    )));
                }
            }
        }
        if full_v6 {
            match set_interface_metric(adapter, AF_INET6, TUNNEL_METRIC) {
                Ok(metric_state) => {
                    log_net::interface_metric_set_v6(TUNNEL_METRIC);
                    state.iface_metrics.push(metric_state);
                    if let Some(recovery) = state.recovery.as_mut() {
                        recovery.record_metric(metric_state)?;
                    }
                }
                Err(err) => {
                    log_net::interface_metric_set_failed_v6(&err);
                    let _ = cleanup_network_config(state).await;
                    return Err(NetworkError::UnsafeRouting(format!(
                        "failed to set IPv6 interface metric for full tunnel: {err}"
                    )));
                }
            }
        }
    }

    let mut bypass_routes = Vec::new();
    let mut endpoint_v4 = 0usize;
    let mut endpoint_v6 = 0usize;
    let mut bypass_v4 = 0usize;
    let mut bypass_v6 = 0usize;

    // 7) 全隧道时，为 endpoint 下发 bypass route。
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
                    log_net::bypass_route_add(route.dest, route.next_hop, route.if_index);
                    if ip.is_ipv4() {
                        bypass_v4 += 1;
                    } else {
                        bypass_v6 += 1;
                    }
                    bypass_routes.push(route);
                }
                Err(err) => log_net::bypass_route_failed(ip, &err),
            }
        }
    }

    // 8) 安全护栏：如果 endpoint 存在但 bypass 为 0，则拒绝继续。
    let missing_v4_bypass = full_v4 && endpoint_v4 > 0 && bypass_v4 == 0;
    let missing_v6_bypass = full_v6 && endpoint_v6 > 0 && bypass_v6 == 0;
    if missing_v4_bypass {
        log_net::skip_default_route_v4();
    }
    if missing_v6_bypass {
        log_net::skip_default_route_v6();
    }
    if missing_v4_bypass || missing_v6_bypass {
        let family = match (missing_v4_bypass, missing_v6_bypass) {
            (true, true) => "IPv4+IPv6",
            (true, false) => "IPv4",
            (false, true) => "IPv6",
            (false, false) => unreachable!(),
        };
        let _ = cleanup_network_config(state).await;
        return Err(NetworkError::UnsafeRouting(format!(
            "full-tunnel {family} endpoint bypass route missing; refusing to continue to avoid traffic/DNS leak"
        )));
    }

    // 9) 先下发 bypass route，再下发 AllowedIPs 路由。
    if interface.table != Some(RouteTable::Off) {
        for entry in bypass_routes {
            if let Err(err) = add_route(&entry) {
                let _ = cleanup_network_config(state).await;
                return Err(err);
            }
            if let Some(recovery) = state.recovery.as_mut() {
                recovery.record_bypass_route(&entry)?;
            }
            state.bypass_routes.push(entry);
        }

        for route in routes {
            let entry = RouteEntry {
                dest: route.addr,
                prefix: route.cidr,
                next_hop: None,
                if_index: adapter.if_index,
                luid: adapter.luid,
            };
            log_net::route_add_windows(
                entry.dest,
                entry.prefix,
                entry.next_hop,
                entry.if_index,
                TUNNEL_METRIC,
            );
            if let Err(err) = add_route(&entry) {
                let _ = cleanup_network_config(state).await;
                return Err(err);
            }
            if let Some(recovery) = state.recovery.as_mut() {
                recovery.record_route(&entry)?;
            }
            state.routes.push(entry);
        }
    }

    // 10) 为隧道 DNS 服务器添加 host route，避免被系统更具体路由抢占。
    if interface.table != Some(RouteTable::Off) && !interface.dns_servers.is_empty() {
        for dns_server in &interface.dns_servers {
            let applies = (dns_server.is_ipv4() && full_v4) || (dns_server.is_ipv6() && full_v6);
            if !applies {
                continue;
            }

            let entry = RouteEntry {
                dest: *dns_server,
                prefix: if dns_server.is_ipv4() { 32 } else { 128 },
                next_hop: None,
                if_index: adapter.if_index,
                luid: adapter.luid,
            };
            log_net::dns_route_add_windows(entry.dest, entry.prefix, entry.if_index, TUNNEL_METRIC);
            if let Err(err) = add_route(&entry) {
                let _ = cleanup_network_config(state).await;
                return Err(err);
            }
            if let Some(recovery) = state.recovery.as_mut() {
                recovery.record_route(&entry)?;
            }
            state.routes.push(entry);
        }
    }

    // 11) 应用 DNS。
    if !interface.dns_servers.is_empty() || !interface.dns_search.is_empty() {
        log_dns::apply_summary(interface.dns_servers.len(), interface.dns_search.len());
        match apply_dns(adapter, &interface.dns_servers, &interface.dns_search) {
            Ok(dns_state) => {
                if let Some(recovery) = state.recovery.as_mut() {
                    recovery.record_dns(&dns_state)?;
                }
                state.dns = Some(dns_state);
            }
            Err(err) => {
                log_dns::apply_failed(&err);
                let _ = cleanup_network_config(state).await;
                return Err(err);
            }
        }
    }

    // 12) 全隧道 + DNS 场景下，启用 NRPT 与 DNS Guard。
    if !interface.dns_servers.is_empty()
        && interface.table != Some(RouteTable::Off)
        && (full_v4 || full_v6)
    {
        match apply_nrpt_guard(adapter, &interface.dns_servers) {
            Ok(nrpt_state) => {
                if let (Some(recovery), Some(nrpt_state)) =
                    (state.recovery.as_mut(), nrpt_state.as_ref())
                {
                    recovery.record_nrpt(nrpt_state)?;
                }
                state.nrpt = nrpt_state;
            }
            Err(err) => {
                let _ = cleanup_network_config(state).await;
                return Err(err);
            }
        }

        match apply_dns_guard(adapter, full_v4, full_v6, &interface.dns_servers) {
            Ok(guard_state) => {
                if let (Some(recovery), Some(guard_state)) =
                    (state.recovery.as_mut(), guard_state.as_ref())
                {
                    recovery.record_dns_guard(guard_state)?;
                }
                state.dns_guard = guard_state;
            }
            Err(err) => {
                let _ = cleanup_network_config(state).await;
                return Err(err);
            }
        }
    }

    if let Some(recovery) = state.recovery.as_mut() {
        recovery.mark_running()?;
    }

    Ok(state)
}

/// 回滚 Windows 网络配置。
///
/// 顺序：bypass route -> 普通 route -> 地址 -> metric -> DNS/NRPT/guard。
pub async fn cleanup_network_config(state: AppliedNetworkState) -> Result<(), NetworkError> {
    let AppliedNetworkState {
        tun_name,
        adapter,
        addresses,
        routes,
        bypass_routes,
        iface_metrics,
        dns,
        nrpt,
        dns_guard,
        mut recovery,
    } = state;

    if let Some(guard) = recovery.as_mut() {
        let _ = guard.mark_stopping();
    }

    log_net::cleanup_windows(
        &tun_name,
        addresses.len(),
        routes.len(),
        bypass_routes.len(),
    );

    for entry in bypass_routes.iter().rev() {
        if let Err(err) = delete_route(entry) {
            log_net::bypass_route_del_failed(&err);
        }
    }

    for entry in routes.iter().rev() {
        if let Err(err) = delete_route(entry) {
            log_net::route_del_failed(&err);
        }
    }

    for address in &addresses {
        if let Err(err) = delete_unicast_address(adapter, address) {
            log_net::address_del_failed(&err);
        }
    }

    for iface in iface_metrics.iter().rev() {
        if let Err(err) = restore_interface_metric(adapter, *iface) {
            log_net::interface_metric_restore_failed(&err);
        }
    }

    if let Some(dns) = dns {
        log_dns::revert_start();
        if let Err(err) = cleanup_dns(dns) {
            log_dns::revert_failed(&err);
        }
    }

    if let Some(nrpt) = nrpt {
        if let Err(err) = cleanup_nrpt_guard(nrpt) {
            log_net::nrpt_cleanup_failed(&err);
        }
    }

    if let Some(guard) = dns_guard {
        if let Err(err) = cleanup_dns_guard(guard) {
            log_net::dns_guard_cleanup_failed(&err);
        }
    }

    if let Some(guard) = recovery {
        guard.clear()?;
    }

    Ok(())
}

/// Windows service 启动期修复占位。
///
/// Phase 1 先切换 owner/transport，后续阶段在这里接入 durable journal。
pub fn attempt_startup_repair() -> Result<(), NetworkError> {
    recovery::attempt_startup_repair()
}

/// 将 Windows 宽字符串指针转换为 Rust String。
fn pwstr_to_string(ptr: PWSTR) -> String {
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

/// 判断 Win32 错误是否属于“已存在”。
fn is_already_exists(code: WIN32_ERROR) -> bool {
    code == ERROR_OBJECT_ALREADY_EXISTS || code == ERROR_ALREADY_EXISTS
}
