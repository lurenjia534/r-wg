//! 路由收集、创建与删除。
//!
//! 主要职责：
//! - 从 peers 的 AllowedIPs 汇总路由；
//! - 判断全隧道与默认路由；
//! - 解析 Endpoint 以便生成绕过路由（bypass route）。

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

#[derive(Clone)]
pub(super) struct RouteEntry {
    /// 目标地址。
    pub(super) dest: IpAddr,
    /// 前缀长度。
    pub(super) prefix: u8,
    /// 下一跳（None 表示 on-link）。
    pub(super) next_hop: Option<IpAddr>,
    /// 绑定的接口索引。
    pub(super) if_index: u32,
    /// 绑定的接口 LUID。
    pub(super) luid: NET_LUID_LH,
}

pub(super) fn add_route(entry: &RouteEntry) -> Result<(), NetworkError> {
    // CreateIpForwardEntry2：添加路由，允许“已存在”作为成功。
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

pub(super) fn delete_route(entry: &RouteEntry) -> Result<(), NetworkError> {
    // DeleteIpForwardEntry2：删除路由，不存在视为成功。
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

pub(super) fn collect_allowed_routes(peers: &[PeerConfig]) -> Vec<AllowedIp> {
    // 去重合并所有 Peer 的 AllowedIPs。
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

pub(super) fn detect_full_tunnel(routes: &[AllowedIp]) -> (bool, bool) {
    // 判断是否包含 0.0.0.0/0 或 ::/0。
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

pub(super) fn is_default_route(route: &AllowedIp) -> bool {
    // 是否为默认路由（全 0 目标 + /0）。
    route.addr.is_unspecified() && route.cidr == 0
}

pub(super) async fn resolve_endpoint_ips(
    peers: &[PeerConfig],
) -> Result<Vec<IpAddr>, NetworkError> {
    // 解析 Endpoint 主机名为 IP，用于生成绕过路由。
    let mut seen = HashSet::new();
    for peer in peers {
        let Some(endpoint) = &peer.endpoint else {
            continue;
        };
        let host = endpoint.host.trim();
        if host.is_empty() {
            continue;
        }
        let lookup = lookup_host((host, endpoint.port)).await.map_err(|_| {
            NetworkError::EndpointResolve(format!("failed to resolve {host}"))
        })?;
        for addr in lookup {
            seen.insert(addr.ip());
        }
    }
    Ok(seen.into_iter().collect())
}

pub(super) fn best_route_to(ip: IpAddr) -> Result<RouteEntry, NetworkError> {
    // 通过 GetBestRoute2 获取系统最优路由，为 bypass route 提供下一跳。
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

fn build_route_row(entry: &RouteEntry) -> MIB_IPFORWARD_ROW2 {
    // 构造路由结构体，包含目标前缀、下一跳与 metric。
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
