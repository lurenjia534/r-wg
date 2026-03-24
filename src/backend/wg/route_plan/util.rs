use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use super::super::config::{AllowedIp, RouteTable, WireGuardConfig};
use super::{
    FullTunnelStatus, RoutePlanChip, RoutePlanFamily, RoutePlanGraphStep, RoutePlanPlatform,
    RoutePlanStepKind, RoutePlanTone,
};

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

pub(super) fn dns_guard_label(
    platform: RoutePlanPlatform,
    full_tunnel: bool,
    dns_empty: bool,
) -> &'static str {
    if dns_empty {
        "No Tunnel DNS"
    } else if platform == RoutePlanPlatform::Windows && full_tunnel {
        "NRPT + Guard"
    } else if full_tunnel {
        "Tunnel DNS"
    } else {
        "Config DNS"
    }
}

pub(super) fn interface_label(parsed: &WireGuardConfig) -> String {
    if parsed.interface.addresses.is_empty() {
        "No interface address".to_string()
    } else {
        parsed
            .interface
            .addresses
            .iter()
            .map(|address| route_text(address.addr, address.cidr))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

pub(super) fn dns_label(parsed: &WireGuardConfig) -> String {
    if parsed.interface.dns_servers.is_empty() {
        "No tunnel DNS".to_string()
    } else {
        parsed
            .interface
            .dns_servers
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

pub(super) fn route_text(addr: IpAddr, cidr: u8) -> String {
    format!("{addr}/{cidr}")
}

pub(super) fn family_from_ip(ip: IpAddr) -> RoutePlanFamily {
    match ip {
        IpAddr::V4(_) => RoutePlanFamily::Ipv4,
        IpAddr::V6(_) => RoutePlanFamily::Ipv6,
    }
}

pub(super) fn is_full_tunnel_route(route: &AllowedIp) -> bool {
    route.addr.is_unspecified() && route.cidr == 0
}

pub(super) fn route_is_default_family(route: &AllowedIp, family: RoutePlanFamily) -> bool {
    match (family, route.addr) {
        (RoutePlanFamily::Ipv4, IpAddr::V4(addr)) => {
            addr == Ipv4Addr::UNSPECIFIED && route.cidr == 0
        }
        (RoutePlanFamily::Ipv6, IpAddr::V6(addr)) => {
            addr == Ipv6Addr::UNSPECIFIED && route.cidr == 0
        }
        _ => false,
    }
}

pub(super) fn cidr_contains(network: IpAddr, cidr: u8, value: IpAddr) -> bool {
    match (network, value) {
        (IpAddr::V4(network), IpAddr::V4(value)) => {
            let mask = if cidr == 0 {
                0
            } else {
                u32::MAX << (32 - cidr)
            };
            (u32::from(network) & mask) == (u32::from(value) & mask)
        }
        (IpAddr::V6(network), IpAddr::V6(value)) => {
            let mask = if cidr == 0 {
                0
            } else {
                u128::MAX << (128 - cidr)
            };
            (u128::from_be_bytes(network.octets()) & mask)
                == (u128::from_be_bytes(value.octets()) & mask)
        }
        _ => false,
    }
}

pub(super) fn graph_step(
    kind: RoutePlanStepKind,
    label: &str,
    value: &str,
    note: Option<&str>,
) -> RoutePlanGraphStep {
    RoutePlanGraphStep {
        kind,
        label: label.to_string(),
        value: value.to_string(),
        note: note.map(ToString::to_string),
    }
}

pub(super) fn chip(label: impl Into<String>, tone: RoutePlanTone) -> RoutePlanChip {
    RoutePlanChip {
        label: label.into(),
        tone,
    }
}

pub(super) fn item_id(prefix: &str, suffix: &str) -> String {
    format!("{prefix}:{}", suffix.to_ascii_lowercase())
}

pub(super) fn allowed_graph_steps(
    interface_label: &str,
    route_table_text: &str,
    peer_label: &str,
    endpoint_text: &str,
    route_text: &str,
    dns_label: &str,
    full_tunnel: bool,
) -> Vec<RoutePlanGraphStep> {
    vec![
        graph_step(
            RoutePlanStepKind::Interface,
            "Local Interface",
            interface_label,
            None,
        ),
        graph_step(
            RoutePlanStepKind::Dns,
            "DNS / Guard",
            dns_label,
            if full_tunnel {
                Some("Full tunnel makes DNS behaviour part of the routing trust story.")
            } else {
                None
            },
        ),
        graph_step(
            RoutePlanStepKind::Policy,
            "Route Table",
            route_table_text,
            if full_tunnel {
                Some("Full-tunnel families may add extra guardrails or policy handling.")
            } else {
                None
            },
        ),
        graph_step(
            RoutePlanStepKind::Peer,
            "Peer",
            peer_label,
            Some(endpoint_text),
        ),
        graph_step(
            RoutePlanStepKind::Destination,
            "Destination Prefix",
            route_text,
            None,
        ),
    ]
}

pub(super) fn allowed_platform_details(
    platform: RoutePlanPlatform,
    allowed: &AllowedIp,
    effective_table: &str,
    table: Option<RouteTable>,
    fwmark: Option<u32>,
    full_tunnel: FullTunnelStatus,
) -> Vec<String> {
    let mut details = vec![format!("Effective table: {effective_table}.")];
    if platform == RoutePlanPlatform::Linux {
        if super::linux_policy_table_id(platform, table, full_tunnel).is_some() {
            details.push(format!(
                "Linux may promote {} into the policy table when full tunnel is active.",
                route_text(allowed.addr, allowed.cidr)
            ));
        }
        if let Some(fwmark) = fwmark {
            details.push(format!("fwmark configured: 0x{fwmark:x}."));
        }
    } else if platform == RoutePlanPlatform::Windows {
        if matches!(table, Some(RouteTable::Id(_))) {
            details.push("Windows ignores explicit RouteTable IDs from the config.".to_string());
        }
        details.push(
            "Windows relies on adapter metric plus host exceptions for full tunnel.".to_string(),
        );
    }
    details
}

pub(super) fn allowed_risks(
    platform: RoutePlanPlatform,
    parsed: &WireGuardConfig,
    allowed: &AllowedIp,
    is_full: bool,
) -> Vec<String> {
    let mut risks = Vec::new();
    if parsed.interface.table == Some(RouteTable::Off) {
        risks.push(
            "Table=Off means this route stays explanatory only and is not installed.".to_string(),
        );
    }
    if is_full && parsed.interface.dns_servers.is_empty() {
        risks.push("Full tunnel is active without tunnel DNS servers.".to_string());
    }
    if platform == RoutePlanPlatform::Linux && is_full && parsed.interface.fwmark.is_none() {
        risks.push(
            "Linux full tunnel depends on fwmark/policy state for safe endpoint escape."
                .to_string(),
        );
    }
    if route_is_default_family(allowed, RoutePlanFamily::Ipv4)
        || route_is_default_family(allowed, RoutePlanFamily::Ipv6)
    {
        risks.push(
            "Default-route prefixes have the highest blast radius when misapplied.".to_string(),
        );
    }
    if risks.is_empty() {
        risks.push("No elevated risk flagged for this route in the planned model.".to_string());
    }
    risks
}
