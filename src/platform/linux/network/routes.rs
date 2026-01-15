//! 路由收集与路由表选择逻辑。
//!
//! 目标：
//! - 从 peer 配置中抽取 AllowedIPs 并去重。
//! - 判断是否为全隧道（0.0.0.0/0 或 ::/0）。
//! - 在全隧道 + policy routing 时，强制写入策略表。

use std::collections::HashSet;
use std::net::IpAddr;

use crate::backend::wg::config::{AllowedIp, PeerConfig, RouteTable};

use super::policy::PolicyRoutingState;

/// 从所有 peer 收集 AllowedIPs 并去重。
pub(super) fn collect_allowed_routes(peers: &[PeerConfig]) -> Vec<AllowedIp> {
    let mut seen = HashSet::new();
    let mut routes = Vec::new();
    for peer in peers {
        for allowed in &peer.allowed_ips {
            // 保留首次出现的顺序，便于日志与排障对齐配置。
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
    // 0/0 即全量路由，分别检测 IPv4 与 IPv6。
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

pub(super) fn route_table_for(
    route: &AllowedIp,
    table: Option<RouteTable>,
    policy: Option<&PolicyRoutingState>,
    full_v4: bool,
    full_v6: bool,
) -> Option<u32> {
    // 全隧道 + policy routing 时，统一走策略表，避免走回 main。
    if let Some(policy) = policy {
        match route.addr {
            IpAddr::V4(_) if full_v4 => return Some(policy.table_id),
            IpAddr::V6(_) if full_v6 => return Some(policy.table_id),
            _ => {}
        }
    }
    // 否则使用用户显式配置的 table（或走默认 main）。
    route_table_id(table)
}

/// 从 RouteTable 生成 netlink 路由表 ID。
fn route_table_id(table: Option<RouteTable>) -> Option<u32> {
    match table {
        Some(RouteTable::Id(value)) => Some(value),
        _ => None,
    }
}
