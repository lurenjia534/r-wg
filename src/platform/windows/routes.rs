//! Windows 路由收集、解析与下发。
//!
//! 职责：
//! - 汇总 Peer 的 AllowedIPs；
//! - 判断是否为全隧道（0.0.0.0/0 或 ::/0）；
//! - 解析 Endpoint 主机名并生成 bypass route；
//! - 统一封装路由增删所需的 Win32 结构。

use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use tokio::net::lookup_host;
use windows::Win32::Foundation::{ERROR_NOT_FOUND, NO_ERROR};
use windows::Win32::NetworkManagement::IpHelper::{
    CreateIpForwardEntry2, DeleteIpForwardEntry2, GetBestRoute2, InitializeIpForwardEntry,
    IP_ADDRESS_PREFIX, MIB_IPFORWARD_ROW2,
};
use windows::Win32::NetworkManagement::Ndis::NET_LUID_LH;
use windows::Win32::Networking::WinSock::{MIB_IPPROTO_NETMGMT, SOCKADDR_INET};

use crate::backend::wg::config::{AllowedIp, PeerConfig};

use super::sockaddr::{ip_from_sockaddr_inet, sockaddr_inet_from_ip};
use super::{is_already_exists, NetworkError, TUNNEL_METRIC};

/// 单条路由定义（用于 add/delete）。
#[derive(Clone)]
pub(super) struct RouteEntry {
    /// 目标前缀地址。
    pub(super) dest: IpAddr,
    /// 前缀长度。
    pub(super) prefix: u8,
    /// 下一跳（`None` 表示 on-link）。
    pub(super) next_hop: Option<IpAddr>,
    /// 绑定接口索引。
    pub(super) if_index: u32,
    /// 绑定接口 LUID。
    pub(super) luid: NET_LUID_LH,
}

/// 添加路由；如果已存在，按成功处理。
pub(super) fn add_route(entry: &RouteEntry) -> Result<(), NetworkError> {
    let row = build_route_row(entry);
    let result = unsafe { CreateIpForwardEntry2(&row) };
    if result == NO_ERROR || is_already_exists(result) {
        Ok(())
    } else {
        Err(NetworkError::Win32 {
            context: "CreateIpForwardEntry2",
            code: result,
        })
    }
}

/// 删除路由；如果不存在，按成功处理。
pub(super) fn delete_route(entry: &RouteEntry) -> Result<(), NetworkError> {
    let row = build_route_row(entry);
    let result = unsafe { DeleteIpForwardEntry2(&row) };
    if result == NO_ERROR || result == ERROR_NOT_FOUND {
        Ok(())
    } else {
        Err(NetworkError::Win32 {
            context: "DeleteIpForwardEntry2",
            code: result,
        })
    }
}

/// 汇总并去重所有 Peer 的 AllowedIPs。
pub(super) fn collect_allowed_routes(peers: &[PeerConfig]) -> Vec<AllowedIp> {
    let mut seen = HashSet::new();
    let mut routes = Vec::new();
    for peer in peers {
        for allowed in &peer.allowed_ips {
            if seen.insert((allowed.addr, allowed.cidr)) {
                routes.push(AllowedIp {
                    addr: allowed.addr,
                    cidr: allowed.cidr,
                });
            }
        }
    }
    routes
}

/// 判断是否包含 IPv4 / IPv6 全隧道路由。
pub(super) fn detect_full_tunnel(routes: &[AllowedIp]) -> (bool, bool) {
    let mut v4 = false;
    let mut v6 = false;
    for route in routes {
        match route.addr {
            IpAddr::V4(addr) if addr.is_unspecified() && route.cidr == 0 => v4 = true,
            IpAddr::V6(addr) if addr.is_unspecified() && route.cidr == 0 => v6 = true,
            _ => {}
        }
    }
    (v4, v6)
}

/// 解析所有 Endpoint 主机名，返回去重后的 IP 列表。
pub(super) async fn resolve_endpoint_ips(
    peers: &[PeerConfig],
) -> Result<Vec<IpAddr>, NetworkError> {
    let mut seen = HashSet::new();
    for peer in peers {
        let Some(endpoint) = &peer.endpoint else {
            continue;
        };
        let host = endpoint.host.trim();
        if host.is_empty() {
            continue;
        }
        let lookup = lookup_host((host, endpoint.port))
            .await
            .map_err(|_| NetworkError::EndpointResolve(format!("failed to resolve {host}")))?;
        for addr in lookup {
            seen.insert(addr.ip());
        }
    }
    Ok(seen.into_iter().collect())
}

/// 查询系统当前到指定 IP 的最佳路径（用于生成 bypass route）。
pub(super) fn best_route_to(ip: IpAddr) -> Result<RouteEntry, NetworkError> {
    let dest = sockaddr_inet_from_ip(ip);
    let mut row: MIB_IPFORWARD_ROW2 = unsafe { std::mem::zeroed() };
    let mut best_source: SOCKADDR_INET = unsafe { std::mem::zeroed() };
    let result = unsafe { GetBestRoute2(None, 0, None, &dest, 0, &mut row, &mut best_source) };
    if result != NO_ERROR {
        return Err(NetworkError::Win32 {
            context: "GetBestRoute2",
            code: result,
        });
    }

    let next_hop = ip_from_sockaddr_inet(&row.NextHop);
    Ok(RouteEntry {
        dest: ip,
        prefix: if ip.is_ipv4() { 32 } else { 128 },
        next_hop,
        if_index: row.InterfaceIndex,
        luid: row.InterfaceLuid,
    })
}

/// 构造 Win32 路由结构。
fn build_route_row(entry: &RouteEntry) -> MIB_IPFORWARD_ROW2 {
    let mut row: MIB_IPFORWARD_ROW2 = unsafe { std::mem::zeroed() };
    unsafe {
        InitializeIpForwardEntry(&mut row);
    }
    row.InterfaceIndex = entry.if_index;
    row.InterfaceLuid = entry.luid;
    row.DestinationPrefix = IP_ADDRESS_PREFIX {
        Prefix: sockaddr_inet_from_ip(entry.dest),
        PrefixLength: entry.prefix,
    };
    let next_hop = entry.next_hop.unwrap_or_else(|| match entry.dest {
        IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
    });
    row.NextHop = sockaddr_inet_from_ip(next_hop);
    row.Metric = TUNNEL_METRIC;
    row.Protocol = MIB_IPPROTO_NETMGMT;
    row
}
