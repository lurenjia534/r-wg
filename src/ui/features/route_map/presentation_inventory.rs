use std::net::IpAddr;

use r_wg::core::config::{RouteTable, WireGuardConfig};
use r_wg::core::route_plan::{
    OperationalRoutePlan, RoutePlanBypassOp, RoutePlanPlatform, RoutePlanRouteKind,
};

use super::data::{
    RouteMapGraphStepKind, RouteMapInspector, RouteMapInventoryGroup, RouteMapInventoryItem,
    RouteMapItemStatus, RouteMapMatchTarget, RouteMapRouteRow, RouteMapTone,
};
use super::presentation_allowed::build_allowed_route_items;
use super::presentation_helpers::{
    chip, dns_label, family_from_ip, family_label, format_route_table, graph_step, interface_label,
    route_text,
};
use super::presentation_policy::build_policy_items;
use super::presentation_warnings::build_warning_items;

pub(super) fn build_inventory_groups(
    route_plan: &OperationalRoutePlan,
    parsed: &WireGuardConfig,
    kill_switch_enabled: bool,
) -> Vec<RouteMapInventoryGroup> {
    let table_off = parsed.interface.table == Some(RouteTable::Off);
    let interface_label = interface_label(parsed);
    let dns_label = dns_label(parsed);
    let route_table_text = format_route_table(parsed.interface.table);

    let (full_tunnel_items, split_route_items) = build_allowed_route_items(
        route_plan,
        parsed,
        &interface_label,
        &dns_label,
        &route_table_text,
    );

    let endpoint_bypass_items = build_endpoint_bypass_items(route_plan, parsed, &interface_label);
    let dns_route_items = build_dns_route_items(route_plan, parsed, &interface_label);
    let policy_items = build_policy_items(route_plan, parsed, table_off);
    let warning_items = build_warning_items(route_plan, parsed, kill_switch_enabled);

    vec![
        group(
            "group-full-tunnel",
            "Full Tunnel",
            full_tunnel_items,
            if route_plan.full_tunnel.any() {
                "Default-route prefixes that take over an address family."
            } else {
                "No 0/0 prefixes in the current config."
            },
        ),
        group(
            "group-split-routes",
            "Split Routes",
            split_route_items,
            "Specific prefixes that stay routed through the selected peer.",
        ),
        group(
            "group-endpoint-bypass",
            "Endpoint Bypass",
            endpoint_bypass_items,
            if route_plan.platform == RoutePlanPlatform::Windows {
                "Endpoint escape routes only exist when full tunnel needs them."
            } else {
                "Linux uses policy routing instead of explicit endpoint bypass routes."
            },
        ),
        group(
            "group-dns-routes",
            "DNS Routes",
            dns_route_items,
            if route_plan.platform == RoutePlanPlatform::Windows {
                "Host routes that keep tunnel DNS reachable under full tunnel."
            } else {
                "No dedicated DNS host routes are planned on this platform."
            },
        ),
        group(
            "group-policy-rules",
            "Policy Rules",
            policy_items,
            if route_plan.platform == RoutePlanPlatform::Linux {
                "Linux full tunnel relies on fwmark and suppress-main rules."
            } else {
                "Windows full tunnel relies on interface metrics and explicit bypass routes."
            },
        ),
        group(
            "group-warnings",
            "Warnings / Guardrails",
            warning_items,
            "Potential leak risks or disabled routing behaviour.",
        ),
    ]
}

fn build_endpoint_bypass_items(
    route_plan: &OperationalRoutePlan,
    parsed: &WireGuardConfig,
    interface_label: &str,
) -> Vec<RouteMapInventoryItem> {
    if route_plan.platform != RoutePlanPlatform::Windows || !route_plan.full_tunnel.any() {
        return Vec::new();
    }

    let mut items = Vec::new();
    for (peer_index, peer) in parsed.peers.iter().enumerate() {
        let Some(endpoint) = peer.endpoint.as_ref() else {
            continue;
        };
        let peer_label = format!("Peer {}", peer_index + 1);
        let endpoint_text = format!("{}:{}", endpoint.host, endpoint.port);
        let endpoint_ip = endpoint.host.parse::<IpAddr>().ok();
        let applies = match endpoint_ip {
            Some(ip) => route_plan.full_tunnel.matches(ip),
            None => route_plan.full_tunnel.any(),
        };
        if !applies {
            continue;
        }

        let bypass_op = RoutePlanBypassOp {
            host: endpoint.host.clone(),
            port: endpoint.port,
        };

        match endpoint_ip {
            Some(ip) => {
                let destination_text = route_text(ip, if ip.is_ipv4() { 32 } else { 128 });
                items.push(RouteMapInventoryItem {
                    id: OperationalRoutePlan::bypass_item_id(&bypass_op).into(),
                    title: endpoint.host.clone().into(),
                    subtitle: format!("{peer_label} endpoint bypass").into(),
                    family: Some(family_from_ip(ip)),
                    static_status: None,
                    status: RouteMapItemStatus::Planned,
                    event_patterns: vec![
                        ip.to_string(),
                        format!("bypass route add: {ip}"),
                        destination_text.clone(),
                    ],
                    chips: vec![
                        chip("Bypass", RouteMapTone::Info),
                        chip(family_label(family_from_ip(ip)), RouteMapTone::Secondary),
                        chip(peer_label.clone(), RouteMapTone::Secondary),
                    ],
                    inspector: RouteMapInspector {
                        title: endpoint.host.clone().into(),
                        subtitle: "Endpoint must stay on the underlay path.".into(),
                        why_match: vec![
                            format!(
                                "Full tunnel would otherwise capture traffic to {endpoint_text}."
                            )
                            .into(),
                            "Windows creates a host route outside the tunnel for endpoint reachability.".into(),
                        ],
                        platform_details: vec![
                            "Windows resolves or reuses endpoint IPs before installing bypass routes.".into(),
                            "The bypass route follows the system best route instead of the tunnel adapter.".into(),
                        ],
                        runtime_evidence: Vec::new(),
                        risk_assessment: vec![
                            "If this bypass route cannot be installed, full-tunnel apply aborts to avoid leaks.".into(),
                        ],
                    },
                    graph_steps: vec![
                        graph_step(
                            RouteMapGraphStepKind::Interface,
                            "Local Interface",
                            interface_label,
                            None,
                        ),
                        graph_step(
                            RouteMapGraphStepKind::Guardrail,
                            "Guardrail",
                            "Full tunnel requires endpoint escape",
                            Some("The endpoint must not recurse into the tunnel."),
                        ),
                        graph_step(
                            RouteMapGraphStepKind::Destination,
                            "Bypass Route",
                            &destination_text,
                            Some("Uses the current system best route."),
                        ),
                        graph_step(
                            RouteMapGraphStepKind::Endpoint,
                            "Endpoint",
                            &endpoint_text,
                            None,
                        ),
                    ],
                    route_row: Some(RouteMapRouteRow {
                        destination: destination_text.clone().into(),
                        family: family_label(family_from_ip(ip)).into(),
                        kind: "bypass".into(),
                        peer: peer_label.clone().into(),
                        endpoint: endpoint_text.clone().into(),
                        table: "system route".into(),
                        status: RouteMapItemStatus::Planned.label().into(),
                        note: "Host route keeps the endpoint outside the tunnel.".into(),
                    }),
                    endpoint_host: Some(endpoint.host.clone().into()),
                    match_target: Some(RouteMapMatchTarget {
                        addr: ip,
                        cidr: if ip.is_ipv4() { 32 } else { 128 },
                    }),
                });
            }
            None => {
                items.push(RouteMapInventoryItem {
                    id: OperationalRoutePlan::bypass_item_id(&bypass_op).into(),
                    title: endpoint.host.clone().into(),
                    subtitle: format!("{peer_label} endpoint hostname").into(),
                    family: None,
                    static_status: Some(RouteMapItemStatus::Warning),
                    status: RouteMapItemStatus::Warning,
                    event_patterns: vec![endpoint.host.clone()],
                    chips: vec![
                        chip("Bypass Pending", RouteMapTone::Warning),
                        chip(peer_label.clone(), RouteMapTone::Secondary),
                    ],
                    inspector: RouteMapInspector {
                        title: endpoint.host.clone().into(),
                        subtitle: "Needs runtime resolution before bypass exists.".into(),
                        why_match: vec![
                            format!(
                                "Endpoint hostname {endpoint_text} cannot be resolved from config alone."
                            )
                            .into(),
                        ],
                        platform_details: vec![
                            "Windows resolves endpoint hostnames inside the privileged backend during apply.".into(),
                            "The UI keeps this item visible so the user can verify the leak guard path.".into(),
                        ],
                        runtime_evidence: Vec::new(),
                        risk_assessment: vec![
                            "Until endpoint resolution succeeds, the bypass route is not concrete.".into(),
                            "The backend will abort full-tunnel apply if resolution/bypass fails.".into(),
                        ],
                    },
                    graph_steps: vec![
                        graph_step(
                            RouteMapGraphStepKind::Interface,
                            "Local Interface",
                            interface_label,
                            None,
                        ),
                        graph_step(
                            RouteMapGraphStepKind::Guardrail,
                            "Guardrail",
                            "Endpoint bypass pending",
                            Some("Runtime DNS resolution is required first."),
                        ),
                        graph_step(
                            RouteMapGraphStepKind::Endpoint,
                            "Endpoint Host",
                            &endpoint_text,
                            None,
                        ),
                    ],
                    route_row: None,
                    endpoint_host: Some(endpoint.host.clone().into()),
                    match_target: None,
                });
            }
        }
    }

    items
}

fn build_dns_route_items(
    route_plan: &OperationalRoutePlan,
    parsed: &WireGuardConfig,
    interface_label: &str,
) -> Vec<RouteMapInventoryItem> {
    if route_plan.platform != RoutePlanPlatform::Windows
        || parsed.interface.table == Some(RouteTable::Off)
    {
        return Vec::new();
    }

    route_plan
        .route_ops
        .iter()
        .filter(|op| matches!(op.kind, RoutePlanRouteKind::DnsHost))
        .map(|op| {
            let dns_server = op.route.addr;
            let destination_text = route_text(dns_server, op.route.cidr);
            RouteMapInventoryItem {
                id: OperationalRoutePlan::route_item_id(op).into(),
                title: dns_server.to_string().into(),
                subtitle: "Tunnel DNS host route".into(),
                family: Some(family_from_ip(dns_server)),
                static_status: None,
                status: RouteMapItemStatus::Planned,
                event_patterns: vec![
                    destination_text.clone(),
                    format!("dns route add: {destination_text}"),
                    dns_server.to_string(),
                ],
                chips: vec![
                    chip("DNS Route", RouteMapTone::Info),
                    chip(family_label(family_from_ip(dns_server)), RouteMapTone::Secondary),
                ],
                inspector: RouteMapInspector {
                    title: dns_server.to_string().into(),
                    subtitle: "Keeps tunnel DNS reachable under full tunnel.".into(),
                    why_match: vec![
                        "This DNS server belongs to the tunnel config.".into(),
                        "Windows adds a host route so tunnel DNS does not get pre-empted by other routes.".into(),
                    ],
                    platform_details: vec![
                        "The host route points back to the tunnel adapter with tunnel metric.".into(),
                    ],
                    runtime_evidence: Vec::new(),
                    risk_assessment: vec![
                        "Without the host route, a more specific underlay route could bypass tunnel DNS.".into(),
                    ],
                },
                graph_steps: vec![
                    graph_step(
                        RouteMapGraphStepKind::Interface,
                        "Local Interface",
                        interface_label,
                        None,
                    ),
                    graph_step(
                        RouteMapGraphStepKind::Dns,
                        "Tunnel DNS",
                        &dns_server.to_string(),
                        Some("Pinned with a host route under full tunnel."),
                    ),
                    graph_step(
                        RouteMapGraphStepKind::Destination,
                        "Route",
                        &destination_text,
                        Some("Installed on the tunnel adapter."),
                    ),
                ],
                route_row: Some(RouteMapRouteRow {
                    destination: destination_text.clone().into(),
                    family: family_label(family_from_ip(dns_server)).into(),
                    kind: "dns".into(),
                    peer: "Interface DNS".into(),
                    endpoint: "-".into(),
                    table: "tunnel adapter".into(),
                    status: RouteMapItemStatus::Planned.label().into(),
                    note: "Host route protects tunnel DNS reachability.".into(),
                }),
                endpoint_host: None,
                match_target: Some(RouteMapMatchTarget {
                    addr: dns_server,
                    cidr: op.route.cidr,
                }),
            }
        })
        .collect()
}

fn group(
    id: &str,
    label: &str,
    items: Vec<RouteMapInventoryItem>,
    empty_note: &str,
) -> RouteMapInventoryGroup {
    RouteMapInventoryGroup {
        id: id.to_string().into(),
        label: label.to_string().into(),
        summary: items.len().to_string().into(),
        empty_note: empty_note.to_string().into(),
        items,
    }
}
