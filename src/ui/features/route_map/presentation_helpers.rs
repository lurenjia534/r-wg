use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use gpui_component::IconName;
use r_wg::core::config::{AllowedIp, RouteTable, WireGuardConfig};
use r_wg::core::route_plan::{OperationalRoutePlan, RoutePlanFamily, RoutePlanPlatform};

use crate::ui::state::RouteFamilyFilter;

use super::data::{RouteMapChip, RouteMapGraphStep, RouteMapGraphStepKind, RouteMapTone};

pub(super) fn chip(label: impl Into<String>, tone: RouteMapTone) -> RouteMapChip {
    RouteMapChip {
        label: label.into().into(),
        tone,
    }
}

pub(super) fn graph_step(
    kind: RouteMapGraphStepKind,
    label: &str,
    value: &str,
    note: Option<&str>,
) -> RouteMapGraphStep {
    RouteMapGraphStep {
        kind,
        icon: plan_step_icon(kind),
        label: label.to_string().into(),
        value: value.to_string().into(),
        note: note.map(|note| note.to_string().into()),
    }
}

pub(super) fn family_from_ip(ip: IpAddr) -> RouteFamilyFilter {
    match ip {
        IpAddr::V4(_) => RouteFamilyFilter::Ipv4,
        IpAddr::V6(_) => RouteFamilyFilter::Ipv6,
    }
}

pub(super) fn filter_from_backend_family(family: RoutePlanFamily) -> RouteFamilyFilter {
    match family {
        RoutePlanFamily::Ipv4 => RouteFamilyFilter::Ipv4,
        RoutePlanFamily::Ipv6 => RouteFamilyFilter::Ipv6,
    }
}

pub(super) fn family_label(family: RouteFamilyFilter) -> &'static str {
    match family {
        RouteFamilyFilter::Ipv4 => "IPv4",
        RouteFamilyFilter::Ipv6 => "IPv6",
        RouteFamilyFilter::All => "All",
    }
}

pub(super) fn route_text(addr: IpAddr, cidr: u8) -> String {
    format!("{addr}/{cidr}")
}

pub(super) fn allowed_item_id(destination_text: &str) -> String {
    format!("allowed:{}", destination_text.to_ascii_lowercase())
}

pub(super) fn is_full_tunnel_route(route: &AllowedIp) -> bool {
    route.addr.is_unspecified() && route.cidr == 0
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

pub(super) fn allowed_platform_details(
    route_plan: &OperationalRoutePlan,
    parsed: &WireGuardConfig,
    allowed: &AllowedIp,
    effective_table: &str,
) -> Vec<String> {
    let mut details = vec![format!("Effective table: {effective_table}.")];
    if route_plan.platform == RoutePlanPlatform::Linux {
        if route_plan.linux_policy_table_id.is_some() {
            details.push(format!(
                "Linux may promote {} into the policy table when full tunnel is active.",
                route_text(allowed.addr, allowed.cidr)
            ));
        }
        if let Some(fwmark) = parsed.interface.fwmark {
            details.push(format!("fwmark configured: 0x{fwmark:x}."));
        }
    } else if route_plan.platform == RoutePlanPlatform::Windows {
        if matches!(parsed.interface.table, Some(RouteTable::Id(_))) {
            details.push("Windows ignores explicit RouteTable IDs from the config.".to_string());
        }
        details.push(
            "Windows relies on adapter metric plus host exceptions for full tunnel.".to_string(),
        );
    }
    details
}

pub(super) fn allowed_risks(
    route_plan: &OperationalRoutePlan,
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
    if route_plan.platform == RoutePlanPlatform::Linux
        && is_full
        && parsed.interface.fwmark.is_none()
    {
        risks.push(
            "Linux full tunnel depends on fwmark/policy state for safe endpoint escape."
                .to_string(),
        );
    }
    if is_default_route_family(allowed, RouteFamilyFilter::Ipv4)
        || is_default_route_family(allowed, RouteFamilyFilter::Ipv6)
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

pub(super) fn allowed_graph_steps(
    interface_label: &str,
    route_table_text: &str,
    peer_label: &str,
    endpoint_text: &str,
    route_text: &str,
    dns_label: &str,
    full_tunnel: bool,
) -> Vec<RouteMapGraphStep> {
    vec![
        graph_step(
            RouteMapGraphStepKind::Interface,
            "Local Interface",
            interface_label,
            None,
        ),
        graph_step(
            RouteMapGraphStepKind::Dns,
            "DNS / Guard",
            dns_label,
            if full_tunnel {
                Some("Full tunnel makes DNS behaviour part of the routing trust story.")
            } else {
                None
            },
        ),
        graph_step(
            RouteMapGraphStepKind::Policy,
            "Route Table",
            route_table_text,
            if full_tunnel {
                Some("Full-tunnel families may add extra guardrails or policy handling.")
            } else {
                None
            },
        ),
        graph_step(
            RouteMapGraphStepKind::Peer,
            "Peer",
            peer_label,
            Some(endpoint_text),
        ),
        graph_step(
            RouteMapGraphStepKind::Destination,
            "Destination Prefix",
            route_text,
            None,
        ),
    ]
}

fn plan_step_icon(kind: RouteMapGraphStepKind) -> IconName {
    match kind {
        RouteMapGraphStepKind::Interface => IconName::LayoutDashboard,
        RouteMapGraphStepKind::Dns => IconName::Search,
        RouteMapGraphStepKind::Policy => IconName::Map,
        RouteMapGraphStepKind::Peer => IconName::CircleUser,
        RouteMapGraphStepKind::Endpoint => IconName::Globe,
        RouteMapGraphStepKind::Guardrail => IconName::TriangleAlert,
        RouteMapGraphStepKind::Destination => IconName::Map,
    }
}

fn is_default_route_family(route: &AllowedIp, family: RouteFamilyFilter) -> bool {
    match (family, route.addr) {
        (RouteFamilyFilter::Ipv4, IpAddr::V4(addr)) => {
            addr == Ipv4Addr::UNSPECIFIED && route.cidr == 0
        }
        (RouteFamilyFilter::Ipv6, IpAddr::V6(addr)) => {
            addr == Ipv6Addr::UNSPECIFIED && route.cidr == 0
        }
        _ => false,
    }
}
