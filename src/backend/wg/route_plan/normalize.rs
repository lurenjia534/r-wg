use std::collections::HashSet;
use std::net::IpAddr;

use crate::dns::{apply_dns_selection, DnsSelection};

use super::super::config::{AllowedIp, PeerConfig, RouteTable, WireGuardConfig};
use super::{FullTunnelStatus, DEFAULT_FULL_TUNNEL_FWMARK, LINUX_DEFAULT_POLICY_TABLE_ID, RoutePlanPlatform};

pub fn normalize_config_for_runtime(
    mut config: WireGuardConfig,
    dns_selection: DnsSelection,
) -> WireGuardConfig {
    apply_dns_selection(
        &mut config.interface.dns_servers,
        &mut config.interface.dns_search,
        dns_selection,
    );
    if wants_full_tunnel(&config.peers) && config.interface.fwmark.is_none() {
        config.interface.fwmark = Some(DEFAULT_FULL_TUNNEL_FWMARK);
    }
    config
}

pub fn collect_allowed_routes(peers: &[PeerConfig]) -> Vec<AllowedIp> {
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

pub fn detect_full_tunnel(routes: &[AllowedIp]) -> FullTunnelStatus {
    let mut full_tunnel = FullTunnelStatus::default();
    for route in routes {
        match route.addr {
            IpAddr::V4(addr) if addr.is_unspecified() && route.cidr == 0 => {
                full_tunnel.ipv4 = true;
            }
            IpAddr::V6(addr) if addr.is_unspecified() && route.cidr == 0 => {
                full_tunnel.ipv6 = true;
            }
            _ => {}
        }
    }
    full_tunnel
}

pub fn linux_default_policy_table_id() -> u32 {
    LINUX_DEFAULT_POLICY_TABLE_ID
}

pub fn linux_policy_table_id(
    platform: RoutePlanPlatform,
    table: Option<RouteTable>,
    full_tunnel: FullTunnelStatus,
) -> Option<u32> {
    if platform != RoutePlanPlatform::Linux || table == Some(RouteTable::Off) || !full_tunnel.any()
    {
        return None;
    }

    Some(match table {
        Some(RouteTable::Id(id)) => id,
        _ => LINUX_DEFAULT_POLICY_TABLE_ID,
    })
}

pub fn linux_route_table_for(
    platform: RoutePlanPlatform,
    table: Option<RouteTable>,
    policy_table_id: Option<u32>,
    full_tunnel: FullTunnelStatus,
    route: &AllowedIp,
) -> Option<u32> {
    if platform == RoutePlanPlatform::Linux {
        if let Some(policy_table_id) = policy_table_id {
            match route.addr {
                IpAddr::V4(_) if full_tunnel.ipv4 => return Some(policy_table_id),
                IpAddr::V6(_) if full_tunnel.ipv6 => return Some(policy_table_id),
                _ => {}
            }
        }
    }

    super::util::route_table_id(table)
}

fn wants_full_tunnel(peers: &[PeerConfig]) -> bool {
    peers.iter().any(|peer| {
        peer.allowed_ips
            .iter()
            .any(|allowed| allowed.addr.is_unspecified() && allowed.cidr == 0)
    })
}
