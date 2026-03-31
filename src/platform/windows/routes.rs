//! Windows 路由收集、解析与下发。
//!
//! 职责：
//! - 复用共享 route plan 的 AllowedIPs 汇总与全隧道判断；
//! - 解析 Endpoint 主机名并生成 bypass route；
//! - 统一封装路由增删所需的 Win32 结构。

use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use serde::{Deserialize, Serialize};
use tokio::net::lookup_host;
use windows::Win32::Foundation::{ERROR_NOT_FOUND, NO_ERROR};
use windows::Win32::NetworkManagement::IpHelper::{
    CreateIpForwardEntry2, DeleteIpForwardEntry2, GetBestRoute2, InitializeIpForwardEntry,
    IP_ADDRESS_PREFIX, MIB_IPFORWARD_ROW2,
};
use windows::Win32::NetworkManagement::Ndis::NET_LUID_LH;
use windows::Win32::Networking::WinSock::{MIB_IPPROTO_NETMGMT, SOCKADDR_INET};

use crate::core::route_plan::RoutePlanBypassOp;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RouteSnapshot {
    pub(super) dest: IpAddr,
    pub(super) prefix: u8,
    pub(super) next_hop: Option<IpAddr>,
    pub(super) if_index: u32,
    pub(super) luid_value: u64,
}

impl From<&RouteEntry> for RouteSnapshot {
    fn from(entry: &RouteEntry) -> Self {
        Self {
            dest: entry.dest,
            prefix: entry.prefix,
            next_hop: entry.next_hop,
            if_index: entry.if_index,
            luid_value: unsafe { entry.luid.Value },
        }
    }
}

impl RouteSnapshot {
    pub(super) fn to_route_entry(&self) -> RouteEntry {
        RouteEntry {
            dest: self.dest,
            prefix: self.prefix,
            next_hop: self.next_hop,
            if_index: self.if_index,
            luid: NET_LUID_LH {
                Value: self.luid_value,
            },
        }
    }
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

/// 解析单个 bypass 目标，返回去重后的 IP 列表。
pub(super) async fn resolve_bypass_ips_for_op(
    op: &RoutePlanBypassOp,
) -> Result<Vec<IpAddr>, NetworkError> {
    let host = op.host.trim();
    if host.is_empty() {
        return Ok(Vec::new());
    }

    let lookup = lookup_host((host, op.port))
        .await
        .map_err(|_| NetworkError::EndpointResolve(format!("failed to resolve {host}")))?;
    let mut seen = HashSet::new();
    for addr in lookup {
        seen.insert(addr.ip());
    }
    Ok(seen.into_iter().collect())
}

/// 解析所有计划中的 bypass 目标，返回去重后的 IP 列表。
pub(super) async fn resolve_bypass_ips(
    bypass_ops: &[RoutePlanBypassOp],
) -> Result<Vec<IpAddr>, NetworkError> {
    let mut seen = HashSet::new();
    for op in bypass_ops {
        for ip in resolve_bypass_ips_for_op(op).await? {
            seen.insert(ip);
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
