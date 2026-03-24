use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use gpui::SharedString;
use gpui_component::IconName;
use r_wg::backend::wg::config::{AllowedIp, RouteTable, WireGuardConfig};
use r_wg::backend::wg::route_plan::{
    RoutePlanBypassOp, RoutePlanFamily, RoutePlanRouteKind,
};
use r_wg::backend::wg::{OperationalRoutePlan, RoutePlanPlatform};

use crate::ui::state::RouteFamilyFilter;

use super::data::{
    RouteMapChip, RouteMapExplainResult, RouteMapGraphStep, RouteMapGraphStepKind,
    RouteMapInspector, RouteMapInventoryGroup, RouteMapInventoryItem, RouteMapItemStatus,
    RouteMapRouteRow, RouteMapTone,
};

#[derive(Clone)]
pub(super) struct RouteMapMatchTarget {
    pub(super) addr: IpAddr,
    pub(super) cidr: u8,
}

pub(super) struct RouteMapPresentedPlan {
    pub(super) plan_status: SharedString,
    pub(super) summary_chips: Vec<RouteMapChip>,
    pub(super) inventory_groups: Vec<RouteMapInventoryGroup>,
}

pub(super) fn build_plan_presentation(
    route_plan: &OperationalRoutePlan,
    parsed: &WireGuardConfig,
) -> RouteMapPresentedPlan {
    let full_tunnel_active = route_plan.full_tunnel.any();
    let table_off = parsed.interface.table == Some(RouteTable::Off);
    let dns_guard_text = dns_guard_label(
        route_plan.platform,
        full_tunnel_active,
        parsed.interface.dns_servers.is_empty(),
    );
    let inventory_groups = build_inventory_groups(route_plan, parsed);
    let warning_count = inventory_groups
        .iter()
        .find(|group| group.id.as_ref() == "group-warnings")
        .map(|group| group.items.len())
        .unwrap_or(0);
    let route_table_text = format_route_table(parsed.interface.table);

    let mut summary_chips = vec![
        chip(
            if full_tunnel_active {
                "Full Tunnel"
            } else {
                "Split Tunnel"
            },
            RouteMapTone::Info,
        ),
        chip(
            if route_plan.full_tunnel.ipv4 {
                "IPv4 Full"
            } else {
                "IPv4 Split"
            },
            RouteMapTone::Secondary,
        ),
        chip(
            if route_plan.full_tunnel.ipv6 {
                "IPv6 Full"
            } else {
                "IPv6 Split"
            },
            RouteMapTone::Secondary,
        ),
        chip(
            dns_guard_text,
            if parsed.interface.dns_servers.is_empty() {
                RouteMapTone::Warning
            } else if full_tunnel_active {
                RouteMapTone::Info
            } else {
                RouteMapTone::Secondary
            },
        ),
        chip(
            format!("Guardrails {warning_count}"),
            if warning_count > 0 {
                RouteMapTone::Warning
            } else {
                RouteMapTone::Secondary
            },
        ),
        chip(
            format!("Bypass {}", route_plan.windows_planned_bypass_count),
            if route_plan.platform == RoutePlanPlatform::Windows && full_tunnel_active {
                RouteMapTone::Info
            } else {
                RouteMapTone::Secondary
            },
        ),
        chip(
            format!("Table {route_table_text}"),
            RouteMapTone::Secondary,
        ),
    ];
    if table_off {
        summary_chips.insert(1, chip("Table Off", RouteMapTone::Warning));
    }

    let plan_status: SharedString = if table_off {
        "Plan generated, but route apply is disabled by Table=Off.".into()
    } else if full_tunnel_active {
        "Decision Path combines effective routes, full-tunnel guardrails, and recent runtime evidence.".into()
    } else {
        "Decision Path combines effective routes and recent runtime evidence for the current config.".into()
    };

    RouteMapPresentedPlan {
        plan_status,
        summary_chips,
        inventory_groups,
    }
}

pub(super) fn build_plan_explain(
    groups: &[RouteMapInventoryGroup],
    search_query: &str,
) -> RouteMapExplainResult {
    let query = search_query.trim().to_string();
    if query.is_empty() {
        return RouteMapExplainResult {
            query: query.into(),
            headline: "Explain a target".into(),
            summary: "Search for an IP, CIDR, or endpoint hostname to see how the current plan resolves it.".into(),
            steps: vec![
                "IP queries pick the most specific planned route or bypass rule.".into(),
                "Hostname queries match configured peer endpoint hosts and call out when runtime resolution is still needed.".into(),
            ],
            risk: Vec::new(),
            matched_item_id: None,
        };
    }

    let items = groups
        .iter()
        .flat_map(|group| group.items.iter())
        .collect::<Vec<_>>();

    if let Ok(ip) = query.parse::<IpAddr>() {
        let best = items
            .iter()
            .filter_map(|item| {
                let target = item.match_target.as_ref()?;
                cidr_contains(target.addr, target.cidr, ip).then_some((item, target.cidr))
            })
            .max_by_key(|(_, cidr)| *cidr)
            .map(|(item, _)| item);

        if let Some(item) = best {
            return RouteMapExplainResult {
                query: query.clone().into(),
                headline: format!("{ip} matches {}", item.title).into(),
                summary: item.subtitle.clone(),
                steps: explain_steps_from_item(item, Some(ip)),
                risk: item.inspector.risk_assessment.clone(),
                matched_item_id: Some(item.id.clone()),
            };
        }

        return RouteMapExplainResult {
            query: query.clone().into(),
            headline: format!("{ip} has no planned match").into(),
            summary: "The current config does not advertise a concrete route for this destination.".into(),
            steps: vec![
                "No AllowedIPs, DNS host route, or endpoint bypass rule contains this IP.".into(),
                "If the expectation differs, inspect the inventory for missing prefixes or family filters.".into(),
            ],
            risk: vec![
                "Unmatched traffic follows the platform default routing policy, not the tunnel plan.".into(),
            ],
            matched_item_id: None,
        };
    }

    if let Some(item) = items.iter().find(|item| {
        item.endpoint_host
            .as_ref()
            .map(|host| host.as_ref().eq_ignore_ascii_case(&query))
            .unwrap_or(false)
    }) {
        return RouteMapExplainResult {
            query: query.clone().into(),
            headline: format!("{query} is a configured endpoint").into(),
            summary: item.subtitle.clone(),
            steps: explain_steps_from_item(item, None),
            risk: item.inspector.risk_assessment.clone(),
            matched_item_id: Some(item.id.clone()),
        };
    }

    RouteMapExplainResult {
        query: query.clone().into(),
        headline: format!("No direct explanation for {query}").into(),
        summary: "Only configured endpoint hosts can be explained without runtime DNS resolution.".into(),
        steps: vec![
            "Route Map can explain raw IPs, CIDRs, and configured endpoint hostnames.".into(),
            "Generic domains need a resolved IP before they can be matched against the current route plan.".into(),
        ],
        risk: Vec::new(),
        matched_item_id: None,
    }
}

fn build_inventory_groups(
    route_plan: &OperationalRoutePlan,
    parsed: &WireGuardConfig,
) -> Vec<RouteMapInventoryGroup> {
    let table_off = parsed.interface.table == Some(RouteTable::Off);
    let interface_label = interface_label(parsed);
    let dns_label = dns_label(parsed);
    let route_table_text = format_route_table(parsed.interface.table);

    let mut full_tunnel_items = Vec::new();
    let mut split_route_items = Vec::new();
    let mut seen = HashSet::new();

    for (peer_index, peer) in parsed.peers.iter().enumerate() {
        let peer_label = format!("Peer {}", peer_index + 1);
        let endpoint_text = peer
            .endpoint
            .as_ref()
            .map(|endpoint| format!("{}:{}", endpoint.host, endpoint.port))
            .unwrap_or_else(|| "No endpoint".to_string());

        for allowed in &peer.allowed_ips {
            if !seen.insert((allowed.addr, allowed.cidr)) {
                continue;
            }

            let destination_text = route_text(allowed.addr, allowed.cidr);
            let family = family_from_ip(allowed.addr);
            let is_full = is_full_tunnel_route(allowed);
            let effective_table = route_plan.effective_route_table_label(allowed);
            let route_kind = if is_full {
                "allowed / full tunnel"
            } else {
                "allowed / split"
            };
            let note = if table_off {
                "Table=Off keeps the match in config but skips route installation.".to_string()
            } else if is_full {
                "This prefix turns the family into full tunnel routing.".to_string()
            } else {
                "This prefix stays split and only captures matching destinations.".to_string()
            };

            let item = RouteMapInventoryItem {
                id: allowed_item_id(&destination_text).into(),
                title: destination_text.clone().into(),
                subtitle: format!("{peer_label} via {endpoint_text}").into(),
                family: Some(family),
                static_status: table_off.then_some(RouteMapItemStatus::Skipped),
                status: table_off
                    .then_some(RouteMapItemStatus::Skipped)
                    .unwrap_or(RouteMapItemStatus::Planned),
                event_patterns: vec![destination_text.clone()],
                chips: vec![
                    chip(
                        if is_full {
                            "Full Tunnel"
                        } else {
                            "Split Route"
                        },
                        if is_full {
                            RouteMapTone::Info
                        } else {
                            RouteMapTone::Secondary
                        },
                    ),
                    chip(family_label(family), RouteMapTone::Secondary),
                    chip(peer_label.clone(), RouteMapTone::Secondary),
                ],
                inspector: RouteMapInspector {
                    title: destination_text.clone().into(),
                    subtitle: format!("{peer_label} advertises this prefix.").into(),
                    why_match: vec![
                        format!("Matched by AllowedIPs on {peer_label}.").into(),
                        format!("Endpoint for this peer is {endpoint_text}.").into(),
                    ],
                    platform_details: allowed_platform_details(
                        route_plan,
                        parsed,
                        allowed,
                        &effective_table,
                    )
                    .into_iter()
                    .map(Into::into)
                    .collect(),
                    runtime_evidence: Vec::new(),
                    risk_assessment: allowed_risks(route_plan, parsed, allowed, is_full)
                        .into_iter()
                        .map(Into::into)
                        .collect(),
                },
                graph_steps: allowed_graph_steps(
                    &interface_label,
                    &route_table_text,
                    &peer_label,
                    &endpoint_text,
                    &destination_text,
                    &dns_label,
                    is_full,
                ),
                route_row: Some(RouteMapRouteRow {
                    destination: destination_text.clone().into(),
                    family: family_label(family).into(),
                    kind: route_kind.into(),
                    peer: peer_label.clone().into(),
                    endpoint: endpoint_text.clone().into(),
                    table: effective_table.into(),
                    status: RouteMapItemStatus::Planned.label().into(),
                    note: note.into(),
                }),
                endpoint_host: peer.endpoint.as_ref().map(|endpoint| endpoint.host.clone().into()),
                match_target: Some(RouteMapMatchTarget {
                    addr: allowed.addr,
                    cidr: allowed.cidr,
                }),
            };

            if is_full {
                full_tunnel_items.push(item);
            } else {
                split_route_items.push(item);
            }
        }
    }

    let endpoint_bypass_items = build_endpoint_bypass_items(route_plan, parsed, &interface_label);
    let dns_route_items = build_dns_route_items(route_plan, parsed, &interface_label);
    let policy_items = build_policy_items(route_plan, parsed, table_off);
    let warning_items = build_warning_items(route_plan, parsed);

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
                        graph_step(RouteMapGraphStepKind::Endpoint, "Endpoint", &endpoint_text, None),
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

fn build_policy_items(
    route_plan: &OperationalRoutePlan,
    parsed: &WireGuardConfig,
    table_off: bool,
) -> Vec<RouteMapInventoryItem> {
    let route_table_text = format_route_table(parsed.interface.table);
    let mut items = Vec::new();

    if route_plan.platform == RoutePlanPlatform::Linux {
        let Some(table_id) = route_plan.linux_policy_table_id else {
            return items;
        };

        for op in &route_plan.policy_rule_ops {
            let family_label = family_label(filter_from_backend_family(op.family));
            items.push(RouteMapInventoryItem {
                id: OperationalRoutePlan::policy_item_id(op).into(),
                title: format!("{family_label} policy routing").into(),
                subtitle: format!("fwmark -> main, not fwmark -> table {table_id}").into(),
                family: Some(filter_from_backend_family(op.family)),
                static_status: table_off.then_some(RouteMapItemStatus::Skipped),
                status: table_off
                    .then_some(RouteMapItemStatus::Skipped)
                    .unwrap_or(RouteMapItemStatus::Planned),
                event_patterns: vec![format!("policy rule add: {}", family_label.to_lowercase())],
                chips: vec![
                    chip("Policy", RouteMapTone::Info),
                    chip(family_label, RouteMapTone::Secondary),
                    chip(format!("table {table_id}"), RouteMapTone::Secondary),
                ],
                inspector: RouteMapInspector {
                    title: format!("{family_label} policy routing").into(),
                    subtitle: "Linux full tunnel keeps engine traffic on main.".into(),
                    why_match: vec![
                        format!("Full tunnel is active for {family_label}.").into(),
                        "Linux uses fwmark rules instead of endpoint bypass routes.".into(),
                    ],
                    platform_details: vec![
                        format!(
                            "Traffic with fwmark 0x{:x} stays on main; unmarked traffic enters table {}.",
                            op.fwmark, table_id
                        )
                        .into(),
                        "A suppress-main rule prevents business traffic from falling back to the default route.".into(),
                    ],
                    runtime_evidence: Vec::new(),
                    risk_assessment: vec![
                        "Missing fwmark or failed rule install would make full-tunnel routing unsafe.".into(),
                    ],
                },
                graph_steps: vec![
                    graph_step(
                        RouteMapGraphStepKind::Interface,
                        "Local Interface",
                        &interface_label(parsed),
                        None,
                    ),
                    graph_step(
                        RouteMapGraphStepKind::Guardrail,
                        "fwmark",
                        &format!("0x{:x}", op.fwmark),
                        Some("Engine traffic stays on main."),
                    ),
                    graph_step(
                        RouteMapGraphStepKind::Policy,
                        "Policy Table",
                        &format!("table {table_id}"),
                        Some("Business traffic enters the tunnel table."),
                    ),
                    graph_step(
                        RouteMapGraphStepKind::Guardrail,
                        "Suppress Main",
                        "pref rule",
                        Some("Prevents unmarked traffic from falling back to main."),
                    ),
                ],
                route_row: None,
                endpoint_host: None,
                match_target: None,
            });
        }
    } else if route_plan.platform == RoutePlanPlatform::Windows && route_plan.full_tunnel.any() {
        for op in &route_plan.metric_ops {
            let family_label = family_label(filter_from_backend_family(op.family));
            items.push(RouteMapInventoryItem {
                id: OperationalRoutePlan::metric_item_id(op).into(),
                title: format!("{family_label} full-tunnel metric").into(),
                subtitle: "Windows lowers interface metric for the tunnel".into(),
                family: Some(filter_from_backend_family(op.family)),
                static_status: table_off.then_some(RouteMapItemStatus::Skipped),
                status: table_off
                    .then_some(RouteMapItemStatus::Skipped)
                    .unwrap_or(RouteMapItemStatus::Planned),
                event_patterns: vec![format!("interface metric set: {}", family_label.to_lowercase())],
                chips: vec![
                    chip("Metric", RouteMapTone::Info),
                    chip(family_label, RouteMapTone::Secondary),
                    chip(format!("table {route_table_text}"), RouteMapTone::Secondary),
                ],
                inspector: RouteMapInspector {
                    title: format!("{family_label} full-tunnel metric").into(),
                    subtitle: "Windows uses interface priority rather than policy rules.".into(),
                    why_match: vec![format!("Full tunnel is active for {family_label}.").into()],
                    platform_details: vec![
                        format!(
                            "The tunnel adapter metric is lowered so {} prefers the tunnel.",
                            if matches!(op.family, RoutePlanFamily::Ipv4) { "0.0.0.0/0" } else { "::/0" }
                        )
                        .into(),
                        "Endpoint and DNS exceptions are handled with host routes.".into(),
                    ],
                    runtime_evidence: Vec::new(),
                    risk_assessment: vec![
                        "If metric programming fails, apply aborts rather than leaving a half-configured full tunnel.".into(),
                    ],
                },
                graph_steps: vec![
                    graph_step(
                        RouteMapGraphStepKind::Interface,
                        "Local Interface",
                        &interface_label(parsed),
                        None,
                    ),
                    graph_step(
                        RouteMapGraphStepKind::Policy,
                        "Interface Metric",
                        "50",
                        Some("Lower metric makes full tunnel preferred."),
                    ),
                    graph_step(
                        RouteMapGraphStepKind::Policy,
                        "Config Table",
                        &route_table_text,
                        Some("RouteTable IDs are ignored on Windows."),
                    ),
                ],
                route_row: None,
                endpoint_host: None,
                match_target: None,
            });
        }
    }

    items
}

fn build_warning_items(
    route_plan: &OperationalRoutePlan,
    parsed: &WireGuardConfig,
) -> Vec<RouteMapInventoryItem> {
    let mut items = Vec::new();

    if parsed.peers.is_empty() {
        items.push(warning_item(
            "warning-no-peers",
            "No peers configured",
            "The route plan cannot carry traffic without at least one peer.",
            vec![
                "No [Peer] sections exist in the current config.".into(),
                "Route Map can still preview interface settings, but no destination will reach a tunnel peer.".into(),
            ],
        ));
    }

    if parsed.interface.table == Some(RouteTable::Off) && !route_plan.allowed_routes.is_empty() {
        items.push(warning_item(
            "warning-table-off",
            "Table=Off disables route apply",
            "Matches still exist logically, but the backend skips route installation.",
            vec![
                "AllowedIPs remain useful for explanation, but they will not become kernel routes.".into(),
                "This is safe only if the user expects to manage routes out-of-band.".into(),
            ],
        ));
    }

    if route_plan.full_tunnel.any() && parsed.interface.dns_servers.is_empty() {
        items.push(warning_item(
            "warning-no-dns",
            "Full tunnel without tunnel DNS",
            "Traffic may be protected, but DNS intent is ambiguous.",
            vec![
                "No DNS servers are configured inside the tunnel.".into(),
                "Users should verify whether system DNS is acceptable for this profile.".into(),
            ],
        ));
    }

    if route_plan.platform == RoutePlanPlatform::Linux
        && route_plan.full_tunnel.any()
        && parsed.interface.fwmark.is_none()
    {
        items.push(warning_item(
            "warning-fwmark",
            "Full tunnel needs fwmark",
            "Linux policy routing depends on fwmark to keep endpoint traffic on main.",
            vec![
                "The engine currently auto-fills a default fwmark when full tunnel is detected.".into(),
                "Route Map surfaces this because a mismatch here breaks trust in the route decision.".into(),
            ],
        ));
    }

    items
}

fn warning_item(
    id: &str,
    title: &str,
    subtitle: &str,
    risk_assessment: Vec<SharedString>,
) -> RouteMapInventoryItem {
    RouteMapInventoryItem {
        id: id.to_string().into(),
        title: title.to_string().into(),
        subtitle: subtitle.to_string().into(),
        family: None,
        static_status: Some(RouteMapItemStatus::Warning),
        status: RouteMapItemStatus::Warning,
        event_patterns: Vec::new(),
        chips: vec![chip("Guardrail", RouteMapTone::Warning)],
        inspector: RouteMapInspector {
            title: title.to_string().into(),
            subtitle: subtitle.to_string().into(),
            why_match: vec![
                "This item is derived from config semantics rather than a single route.".into(),
            ],
            platform_details: vec![
                "Use this section to spot leak-prone or intentionally disabled behaviour.".into(),
            ],
            runtime_evidence: Vec::new(),
            risk_assessment,
        },
        graph_steps: vec![graph_step(
            RouteMapGraphStepKind::Guardrail,
            "Guardrail",
            title,
            Some(subtitle),
        )],
        route_row: None,
        endpoint_host: None,
        match_target: None,
    }
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

fn explain_steps_from_item(item: &RouteMapInventoryItem, ip: Option<IpAddr>) -> Vec<SharedString> {
    let mut steps = Vec::new();
    if let Some(ip) = ip {
        steps.push(format!("{ip} matched the most specific visible route.").into());
    }
    for step in &item.graph_steps {
        match step.note.as_ref() {
            Some(note) => steps.push(format!("{} -> {} ({note})", step.label, step.value).into()),
            None => steps.push(format!("{} -> {}", step.label, step.value).into()),
        }
    }
    steps
}

fn chip(label: impl Into<String>, tone: RouteMapTone) -> RouteMapChip {
    RouteMapChip {
        label: label.into().into(),
        tone,
    }
}

fn graph_step(
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

fn family_from_ip(ip: IpAddr) -> RouteFamilyFilter {
    match ip {
        IpAddr::V4(_) => RouteFamilyFilter::Ipv4,
        IpAddr::V6(_) => RouteFamilyFilter::Ipv6,
    }
}

fn filter_from_backend_family(family: RoutePlanFamily) -> RouteFamilyFilter {
    match family {
        RoutePlanFamily::Ipv4 => RouteFamilyFilter::Ipv4,
        RoutePlanFamily::Ipv6 => RouteFamilyFilter::Ipv6,
    }
}

fn family_label(family: RouteFamilyFilter) -> &'static str {
    match family {
        RouteFamilyFilter::Ipv4 => "IPv4",
        RouteFamilyFilter::Ipv6 => "IPv6",
        RouteFamilyFilter::All => "All",
    }
}

fn route_text(addr: IpAddr, cidr: u8) -> String {
    format!("{addr}/{cidr}")
}

fn allowed_item_id(destination_text: &str) -> String {
    format!("allowed:{}", destination_text.to_ascii_lowercase())
}

fn is_full_tunnel_route(route: &AllowedIp) -> bool {
    route.addr.is_unspecified() && route.cidr == 0
}

fn format_route_table(table: Option<RouteTable>) -> String {
    match table {
        Some(RouteTable::Auto) => "auto".to_string(),
        Some(RouteTable::Off) => "off".to_string(),
        Some(RouteTable::Id(id)) => format!("id:{id}"),
        None => "main".to_string(),
    }
}

fn dns_guard_label(
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

fn interface_label(parsed: &WireGuardConfig) -> String {
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

fn dns_label(parsed: &WireGuardConfig) -> String {
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

fn allowed_platform_details(
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

fn allowed_risks(
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
    if route_plan.platform == RoutePlanPlatform::Linux && is_full && parsed.interface.fwmark.is_none()
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

fn allowed_graph_steps(
    interface_label: &str,
    route_table_text: &str,
    peer_label: &str,
    endpoint_text: &str,
    route_text: &str,
    dns_label: &str,
    full_tunnel: bool,
) -> Vec<RouteMapGraphStep> {
    vec![
        graph_step(RouteMapGraphStepKind::Interface, "Local Interface", interface_label, None),
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

fn cidr_contains(network: IpAddr, cidr: u8, value: IpAddr) -> bool {
    match (network, value) {
        (IpAddr::V4(network), IpAddr::V4(value)) => {
            let mask = if cidr == 0 { 0 } else { u32::MAX << (32 - cidr) };
            (u32::from(network) & mask) == (u32::from(value) & mask)
        }
        (IpAddr::V6(network), IpAddr::V6(value)) => {
            let mask = if cidr == 0 { 0 } else { u128::MAX << (128 - cidr) };
            (u128::from_be_bytes(network.octets()) & mask)
                == (u128::from_be_bytes(value.octets()) & mask)
        }
        _ => false,
    }
}
