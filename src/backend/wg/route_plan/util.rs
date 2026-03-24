use std::net::IpAddr;

use super::super::config::{AllowedIp, RouteTable};
use super::{FullTunnelStatus, RoutePlanPlatform};

pub(super) fn effective_route_table_label(
    platform: RoutePlanPlatform,
    table: Option<RouteTable>,
    linux_policy_table_id: Option<u32>,
    route: &AllowedIp,
    full_tunnel: FullTunnelStatus,
) -> String {
    if platform == RoutePlanPlatform::Linux {
        if let Some(policy_table_id) = linux_policy_table_id {
            match route.addr {
                IpAddr::V4(_) if full_tunnel.ipv4 => return format!("table {policy_table_id}"),
                IpAddr::V6(_) if full_tunnel.ipv6 => return format!("table {policy_table_id}"),
                _ => {}
            }
        }
        match table {
            Some(RouteTable::Id(id)) => format!("table {id}"),
            Some(RouteTable::Off) => "off".to_string(),
            _ => "main".to_string(),
        }
    } else if platform == RoutePlanPlatform::Windows {
        match table {
            Some(RouteTable::Off) => "off".to_string(),
            _ => "tunnel adapter".to_string(),
        }
    } else {
        format_route_table(table)
    }
}

pub(super) fn route_table_id(table: Option<RouteTable>) -> Option<u32> {
    match table {
        Some(RouteTable::Id(id)) => Some(id),
        _ => None,
    }
}

pub(super) fn format_route_table(table: Option<RouteTable>) -> String {
    match table {
        Some(RouteTable::Auto) => "auto".to_string(),
        Some(RouteTable::Off) => "off".to_string(),
        Some(RouteTable::Id(id)) => format!("id:{id}"),
        None => "main".to_string(),
    }
}

pub(super) fn route_text(addr: IpAddr, cidr: u8) -> String {
    format!("{addr}/{cidr}")
}

pub(super) fn item_id(prefix: &str, suffix: &str) -> String {
    format!("{prefix}:{}", suffix.to_ascii_lowercase())
}
