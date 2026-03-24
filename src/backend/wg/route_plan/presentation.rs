use std::collections::HashSet;

use super::super::config::{AllowedIp, RouteTable, WireGuardConfig};
use super::{
    FullTunnelStatus, RoutePlanBypassOp, RoutePlanChip, RoutePlanFamily, RoutePlanGroup,
    RoutePlanInspector, RoutePlanItem, RoutePlanMatchTarget, RoutePlanPlatform, RoutePlanRouteRow,
    RoutePlanStaticStatus, RoutePlanStepKind, RoutePlanTone,
};

pub(super) fn build_plan_presentation(
    platform: RoutePlanPlatform,
    parsed: &WireGuardConfig,
    routes: &[AllowedIp],
    full_tunnel: FullTunnelStatus,
    linux_policy_table_id: Option<u32>,
    windows_planned_bypass_count: usize,
) -> (String, Vec<RoutePlanChip>, Vec<RoutePlanGroup>) {
    let full_tunnel_active = full_tunnel.any();
    let table_off = parsed.interface.table == Some(RouteTable::Off);
    let dns_guard_text = super::util::dns_guard_label(
        platform,
        full_tunnel_active,
        parsed.interface.dns_servers.is_empty(),
    );
    let inventory_groups =
        build_inventory_groups(platform, parsed, routes, full_tunnel, linux_policy_table_id);
    let warning_count = inventory_groups
        .iter()
        .find(|group| group.id == "group-warnings")
        .map(|group| group.items.len())
        .unwrap_or(0);
    let route_table_text = super::util::format_route_table(parsed.interface.table);

    let mut summary_chips = vec![
        super::util::chip(
            if full_tunnel_active {
                "Full Tunnel"
            } else {
                "Split Tunnel"
            },
            RoutePlanTone::Info,
        ),
        super::util::chip(
            if full_tunnel.ipv4 {
                "IPv4 Full"
            } else {
                "IPv4 Split"
            },
            RoutePlanTone::Secondary,
        ),
        super::util::chip(
            if full_tunnel.ipv6 {
                "IPv6 Full"
            } else {
                "IPv6 Split"
            },
            RoutePlanTone::Secondary,
        ),
        super::util::chip(
            dns_guard_text,
            if parsed.interface.dns_servers.is_empty() {
                RoutePlanTone::Warning
            } else if full_tunnel_active {
                RoutePlanTone::Info
            } else {
                RoutePlanTone::Secondary
            },
        ),
        super::util::chip(
            format!("Guardrails {warning_count}"),
            if warning_count > 0 {
                RoutePlanTone::Warning
            } else {
                RoutePlanTone::Secondary
            },
        ),
        super::util::chip(
            format!("Bypass {windows_planned_bypass_count}"),
            if platform == RoutePlanPlatform::Windows && full_tunnel_active {
                RoutePlanTone::Info
            } else {
                RoutePlanTone::Secondary
            },
        ),
        super::util::chip(
            format!("Table {route_table_text}"),
            RoutePlanTone::Secondary,
        ),
    ];
    if table_off {
        summary_chips.insert(1, super::util::chip("Table Off", RoutePlanTone::Warning));
    }

    let plan_status = if table_off {
        "Plan generated, but route apply is disabled by Table=Off.".to_string()
    } else if full_tunnel_active {
        "Decision Path combines effective routes, full-tunnel guardrails, and recent runtime evidence.".to_string()
    } else {
        "Decision Path combines effective routes and recent runtime evidence for the current config.".to_string()
    };

    (plan_status, summary_chips, inventory_groups)
}

fn build_inventory_groups(
    platform: RoutePlanPlatform,
    parsed: &WireGuardConfig,
    routes: &[AllowedIp],
    full_tunnel: FullTunnelStatus,
    linux_policy_table_id: Option<u32>,
) -> Vec<RoutePlanGroup> {
    let table_off = parsed.interface.table == Some(RouteTable::Off);
    let interface_label = super::util::interface_label(parsed);
    let dns_label = super::util::dns_label(parsed);
    let route_table_text = super::util::format_route_table(parsed.interface.table);

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

            let destination_text = super::util::route_text(allowed.addr, allowed.cidr);
            let family = super::util::family_from_ip(allowed.addr);
            let is_full = super::util::is_full_tunnel_route(allowed);
            let effective_table = super::util::effective_route_table_label(
                platform,
                parsed.interface.table,
                linux_policy_table_id,
                allowed,
                full_tunnel,
            );
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

            let item = RoutePlanItem {
                id: super::util::item_id("allowed", &destination_text),
                title: destination_text.clone(),
                subtitle: format!("{peer_label} via {endpoint_text}"),
                family: Some(family),
                static_status: table_off.then_some(RoutePlanStaticStatus::Skipped),
                event_patterns: vec![destination_text.clone()],
                chips: vec![
                    super::util::chip(
                        if is_full {
                            "Full Tunnel"
                        } else {
                            "Split Route"
                        },
                        if is_full {
                            RoutePlanTone::Info
                        } else {
                            RoutePlanTone::Secondary
                        },
                    ),
                    super::util::chip(family.label(), RoutePlanTone::Secondary),
                    super::util::chip(peer_label.clone(), RoutePlanTone::Secondary),
                ],
                inspector: RoutePlanInspector {
                    title: destination_text.clone(),
                    subtitle: format!("{peer_label} advertises this prefix."),
                    why_match: vec![
                        format!("Matched by AllowedIPs on {peer_label}."),
                        format!("Endpoint for this peer is {endpoint_text}."),
                    ],
                    platform_details: super::util::allowed_platform_details(
                        platform,
                        allowed,
                        &effective_table,
                        parsed.interface.table,
                        parsed.interface.fwmark,
                        full_tunnel,
                    ),
                    risk_assessment: super::util::allowed_risks(platform, parsed, allowed, is_full),
                },
                graph_steps: super::util::allowed_graph_steps(
                    &interface_label,
                    &route_table_text,
                    &peer_label,
                    &endpoint_text,
                    &destination_text,
                    &dns_label,
                    is_full,
                ),
                route_row: Some(RoutePlanRouteRow {
                    destination: destination_text.clone(),
                    family: family.label().to_string(),
                    kind: route_kind.to_string(),
                    peer: peer_label.clone(),
                    endpoint: endpoint_text.clone(),
                    table: effective_table,
                    note,
                }),
                match_target: Some(RoutePlanMatchTarget {
                    addr: allowed.addr,
                    cidr: allowed.cidr,
                }),
                endpoint_host: peer.endpoint.as_ref().map(|endpoint| endpoint.host.clone()),
            };

            if is_full {
                full_tunnel_items.push(item);
            } else {
                split_route_items.push(item);
            }
        }
    }

    let endpoint_bypass_items =
        build_endpoint_bypass_items(platform, parsed, full_tunnel, &interface_label);
    let dns_route_items = build_dns_route_items(platform, parsed, full_tunnel, &interface_label);
    let policy_items = build_policy_items(
        platform,
        parsed,
        full_tunnel,
        table_off,
        linux_policy_table_id,
    );
    let warning_items = build_warning_items(platform, parsed, routes, full_tunnel);

    vec![
        group(
            "group-full-tunnel",
            "Full Tunnel",
            full_tunnel_items,
            if full_tunnel.any() {
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
            if platform == RoutePlanPlatform::Windows {
                "Endpoint escape routes only exist when full tunnel needs them."
            } else {
                "Linux uses policy routing instead of explicit endpoint bypass routes."
            },
        ),
        group(
            "group-dns-routes",
            "DNS Routes",
            dns_route_items,
            if platform == RoutePlanPlatform::Windows {
                "Host routes that keep tunnel DNS reachable under full tunnel."
            } else {
                "No dedicated DNS host routes are planned on this platform."
            },
        ),
        group(
            "group-policy-rules",
            "Policy Rules",
            policy_items,
            if platform == RoutePlanPlatform::Linux {
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
    platform: RoutePlanPlatform,
    parsed: &WireGuardConfig,
    full_tunnel: FullTunnelStatus,
    interface_label: &str,
) -> Vec<RoutePlanItem> {
    if platform != RoutePlanPlatform::Windows || !full_tunnel.any() {
        return Vec::new();
    }

    let mut items = Vec::new();
    for (peer_index, peer) in parsed.peers.iter().enumerate() {
        let Some(endpoint) = peer.endpoint.as_ref() else {
            continue;
        };
        let peer_label = format!("Peer {}", peer_index + 1);
        let endpoint_text = format!("{}:{}", endpoint.host, endpoint.port);
        let endpoint_ip = endpoint.host.parse::<std::net::IpAddr>().ok();
        let applies = match endpoint_ip {
            Some(ip) => full_tunnel.matches(ip),
            None => full_tunnel.any(),
        };
        if !applies {
            continue;
        }

        match endpoint_ip {
            Some(ip) => {
                let destination_text =
                    super::util::route_text(ip, if ip.is_ipv4() { 32 } else { 128 });
                let bypass_op = RoutePlanBypassOp {
                    host: endpoint.host.clone(),
                    port: endpoint.port,
                };
                items.push(RoutePlanItem {
                    id: super::ids::bypass_item_id(&bypass_op),
                    title: endpoint.host.clone(),
                    subtitle: format!("{peer_label} endpoint bypass"),
                    family: Some(super::util::family_from_ip(ip)),
                    static_status: None,
                    event_patterns: vec![
                        ip.to_string(),
                        format!("bypass route add: {ip}"),
                        destination_text.clone(),
                    ],
                    chips: vec![
                        super::util::chip("Bypass", RoutePlanTone::Info),
                        super::util::chip(
                            super::util::family_from_ip(ip).label(),
                            RoutePlanTone::Secondary,
                        ),
                        super::util::chip(peer_label.clone(), RoutePlanTone::Secondary),
                    ],
                    inspector: RoutePlanInspector {
                        title: endpoint.host.clone(),
                        subtitle: "Endpoint must stay on the underlay path.".to_string(),
                        why_match: vec![
                            format!(
                                "Full tunnel would otherwise capture traffic to {endpoint_text}."
                            ),
                            "Windows creates a host route outside the tunnel for endpoint reachability.".to_string(),
                        ],
                        platform_details: vec![
                            "Windows resolves or reuses endpoint IPs before installing bypass routes.".to_string(),
                            "The bypass route follows the system best route instead of the tunnel adapter.".to_string(),
                        ],
                        risk_assessment: vec![
                            "If this bypass route cannot be installed, full-tunnel apply aborts to avoid leaks.".to_string(),
                        ],
                    },
                    graph_steps: vec![
                        super::util::graph_step(
                            RoutePlanStepKind::Interface,
                            "Local Interface",
                            interface_label,
                            None,
                        ),
                        super::util::graph_step(
                            RoutePlanStepKind::Guardrail,
                            "Guardrail",
                            "Full tunnel requires endpoint escape",
                            Some("The endpoint must not recurse into the tunnel."),
                        ),
                        super::util::graph_step(
                            RoutePlanStepKind::Destination,
                            "Bypass Route",
                            &destination_text,
                            Some("Uses the current system best route."),
                        ),
                        super::util::graph_step(
                            RoutePlanStepKind::Endpoint,
                            "Endpoint",
                            &endpoint_text,
                            None,
                        ),
                    ],
                    route_row: Some(RoutePlanRouteRow {
                        destination: destination_text.clone(),
                        family: super::util::family_from_ip(ip).label().to_string(),
                        kind: "bypass".to_string(),
                        peer: peer_label.clone(),
                        endpoint: endpoint_text.clone(),
                        table: "system route".to_string(),
                        note: "Host route keeps the endpoint outside the tunnel.".to_string(),
                    }),
                    match_target: Some(RoutePlanMatchTarget {
                        addr: ip,
                        cidr: if ip.is_ipv4() { 32 } else { 128 },
                    }),
                    endpoint_host: Some(endpoint.host.clone()),
                });
            }
            None => {
                let bypass_op = RoutePlanBypassOp {
                    host: endpoint.host.clone(),
                    port: endpoint.port,
                };
                items.push(RoutePlanItem {
                    id: super::ids::bypass_item_id(&bypass_op),
                    title: endpoint.host.clone(),
                    subtitle: format!("{peer_label} endpoint hostname"),
                    family: None,
                    static_status: Some(RoutePlanStaticStatus::Warning),
                    event_patterns: vec![endpoint.host.clone()],
                    chips: vec![
                        super::util::chip("Bypass Pending", RoutePlanTone::Warning),
                        super::util::chip(peer_label.clone(), RoutePlanTone::Secondary),
                    ],
                    inspector: RoutePlanInspector {
                        title: endpoint.host.clone(),
                        subtitle: "Needs runtime resolution before bypass exists.".to_string(),
                        why_match: vec![format!(
                            "Endpoint hostname {endpoint_text} cannot be resolved from config alone."
                        )],
                        platform_details: vec![
                            "Windows resolves endpoint hostnames inside the privileged backend during apply.".to_string(),
                            "The UI keeps this item visible so the user can verify the leak guard path.".to_string(),
                        ],
                        risk_assessment: vec![
                            "Until endpoint resolution succeeds, the bypass route is not concrete.".to_string(),
                            "The backend will abort full-tunnel apply if resolution/bypass fails.".to_string(),
                        ],
                    },
                    graph_steps: vec![
                        super::util::graph_step(
                            RoutePlanStepKind::Interface,
                            "Local Interface",
                            interface_label,
                            None,
                        ),
                        super::util::graph_step(
                            RoutePlanStepKind::Guardrail,
                            "Guardrail",
                            "Endpoint bypass pending",
                            Some("Runtime DNS resolution is required first."),
                        ),
                        super::util::graph_step(
                            RoutePlanStepKind::Endpoint,
                            "Endpoint Host",
                            &endpoint_text,
                            None,
                        ),
                    ],
                    route_row: None,
                    match_target: None,
                    endpoint_host: Some(endpoint.host.clone()),
                });
            }
        }
    }

    items
}

fn build_dns_route_items(
    platform: RoutePlanPlatform,
    parsed: &WireGuardConfig,
    full_tunnel: FullTunnelStatus,
    interface_label: &str,
) -> Vec<RoutePlanItem> {
    if platform != RoutePlanPlatform::Windows || parsed.interface.table == Some(RouteTable::Off) {
        return Vec::new();
    }

    let mut items = Vec::new();
    for dns_server in &parsed.interface.dns_servers {
        if !full_tunnel.matches(*dns_server) {
            continue;
        }

        let destination_text = super::util::route_text(
            *dns_server,
            if dns_server.is_ipv4() { 32 } else { 128 },
        );
        items.push(RoutePlanItem {
            id: super::util::item_id("dns-route", &dns_server.to_string()),
            title: dns_server.to_string(),
            subtitle: "Tunnel DNS host route".to_string(),
            family: Some(super::util::family_from_ip(*dns_server)),
            static_status: None,
            event_patterns: vec![
                destination_text.clone(),
                format!("dns route add: {destination_text}"),
                dns_server.to_string(),
            ],
            chips: vec![
                super::util::chip("DNS Route", RoutePlanTone::Info),
                super::util::chip(
                    super::util::family_from_ip(*dns_server).label(),
                    RoutePlanTone::Secondary,
                ),
            ],
            inspector: RoutePlanInspector {
                title: dns_server.to_string(),
                subtitle: "Keeps tunnel DNS reachable under full tunnel.".to_string(),
                why_match: vec![
                    "This DNS server belongs to the tunnel config.".to_string(),
                    "Windows adds a host route so tunnel DNS does not get pre-empted by other routes.".to_string(),
                ],
                platform_details: vec![
                    "The host route points back to the tunnel adapter with tunnel metric."
                        .to_string(),
                ],
                risk_assessment: vec![
                    "Without the host route, a more specific underlay route could bypass tunnel DNS.".to_string(),
                ],
            },
            graph_steps: vec![
                super::util::graph_step(
                    RoutePlanStepKind::Interface,
                    "Local Interface",
                    interface_label,
                    None,
                ),
                super::util::graph_step(
                    RoutePlanStepKind::Dns,
                    "Tunnel DNS",
                    &dns_server.to_string(),
                    Some("Pinned with a host route under full tunnel."),
                ),
                super::util::graph_step(
                    RoutePlanStepKind::Destination,
                    "Route",
                    &destination_text,
                    Some("Installed on the tunnel adapter."),
                ),
            ],
            route_row: Some(RoutePlanRouteRow {
                destination: destination_text.clone(),
                family: super::util::family_from_ip(*dns_server).label().to_string(),
                kind: "dns".to_string(),
                peer: "Interface DNS".to_string(),
                endpoint: "-".to_string(),
                table: "tunnel adapter".to_string(),
                note: "Host route protects tunnel DNS reachability.".to_string(),
            }),
            match_target: Some(RoutePlanMatchTarget {
                addr: *dns_server,
                cidr: if dns_server.is_ipv4() { 32 } else { 128 },
            }),
            endpoint_host: None,
        });
    }

    items
}

fn build_policy_items(
    platform: RoutePlanPlatform,
    parsed: &WireGuardConfig,
    full_tunnel: FullTunnelStatus,
    table_off: bool,
    linux_policy_table_id: Option<u32>,
) -> Vec<RoutePlanItem> {
    let route_table_text = super::util::format_route_table(parsed.interface.table);
    let mut items = Vec::new();

    if platform == RoutePlanPlatform::Linux {
        let Some(table_id) = linux_policy_table_id else {
            return items;
        };

        if full_tunnel.ipv4 {
            items.push(RoutePlanItem {
                id: "policy-v4".to_string(),
                title: "IPv4 policy routing".to_string(),
                subtitle: format!("fwmark -> main, not fwmark -> table {table_id}"),
                family: Some(RoutePlanFamily::Ipv4),
                static_status: table_off.then_some(RoutePlanStaticStatus::Skipped),
                event_patterns: vec!["policy rule add: v4".to_string()],
                chips: vec![
                    super::util::chip("Policy", RoutePlanTone::Info),
                    super::util::chip("IPv4", RoutePlanTone::Secondary),
                    super::util::chip(format!("table {table_id}"), RoutePlanTone::Secondary),
                ],
                inspector: RoutePlanInspector {
                    title: "IPv4 policy routing".to_string(),
                    subtitle: "Linux full tunnel keeps engine traffic on main.".to_string(),
                    why_match: vec![
                        "Full tunnel is active for IPv4.".to_string(),
                        "Linux uses fwmark rules instead of endpoint bypass routes.".to_string(),
                    ],
                    platform_details: vec![
                        format!(
                            "Traffic with fwmark 0x{:x} stays on main; unmarked traffic enters table {}.",
                            parsed.interface.fwmark.unwrap_or_default(),
                            table_id
                        ),
                        "A suppress-main rule prevents business traffic from falling back to the default route.".to_string(),
                    ],
                    risk_assessment: vec![
                        "Missing fwmark or failed rule install would make full-tunnel routing unsafe.".to_string(),
                    ],
                },
                graph_steps: vec![
                    super::util::graph_step(
                        RoutePlanStepKind::Interface,
                        "Local Interface",
                        &super::util::interface_label(parsed),
                        None,
                    ),
                    super::util::graph_step(
                        RoutePlanStepKind::Guardrail,
                        "fwmark",
                        &format!("0x{:x}", parsed.interface.fwmark.unwrap_or_default()),
                        Some("Engine traffic stays on main."),
                    ),
                    super::util::graph_step(
                        RoutePlanStepKind::Policy,
                        "Policy Table",
                        &format!("table {table_id}"),
                        Some("Business traffic enters the tunnel table."),
                    ),
                    super::util::graph_step(
                        RoutePlanStepKind::Guardrail,
                        "Suppress Main",
                        "pref rule",
                        Some("Prevents unmarked traffic from falling back to main."),
                    ),
                ],
                route_row: None,
                match_target: None,
                endpoint_host: None,
            });
        }

        if full_tunnel.ipv6 {
            items.push(RoutePlanItem {
                id: "policy-v6".to_string(),
                title: "IPv6 policy routing".to_string(),
                subtitle: format!("fwmark -> main, not fwmark -> table {table_id}"),
                family: Some(RoutePlanFamily::Ipv6),
                static_status: table_off.then_some(RoutePlanStaticStatus::Skipped),
                event_patterns: vec!["policy rule add: v6".to_string()],
                chips: vec![
                    super::util::chip("Policy", RoutePlanTone::Info),
                    super::util::chip("IPv6", RoutePlanTone::Secondary),
                    super::util::chip(format!("table {table_id}"), RoutePlanTone::Secondary),
                ],
                inspector: RoutePlanInspector {
                    title: "IPv6 policy routing".to_string(),
                    subtitle: "Linux full tunnel keeps engine traffic on main.".to_string(),
                    why_match: vec![
                        "Full tunnel is active for IPv6.".to_string(),
                        "Linux uses fwmark rules instead of endpoint bypass routes.".to_string(),
                    ],
                    platform_details: vec![
                        format!(
                            "Traffic with fwmark 0x{:x} stays on main; unmarked traffic enters table {}.",
                            parsed.interface.fwmark.unwrap_or_default(),
                            table_id
                        ),
                        "A suppress-main rule prevents business traffic from falling back to the default route.".to_string(),
                    ],
                    risk_assessment: vec![
                        "Missing fwmark or failed rule install would make full-tunnel routing unsafe.".to_string(),
                    ],
                },
                graph_steps: vec![
                    super::util::graph_step(
                        RoutePlanStepKind::Interface,
                        "Local Interface",
                        &super::util::interface_label(parsed),
                        None,
                    ),
                    super::util::graph_step(
                        RoutePlanStepKind::Guardrail,
                        "fwmark",
                        &format!("0x{:x}", parsed.interface.fwmark.unwrap_or_default()),
                        Some("Engine traffic stays on main."),
                    ),
                    super::util::graph_step(
                        RoutePlanStepKind::Policy,
                        "Policy Table",
                        &format!("table {table_id}"),
                        Some("Business traffic enters the tunnel table."),
                    ),
                    super::util::graph_step(
                        RoutePlanStepKind::Guardrail,
                        "Suppress Main",
                        "pref rule",
                        Some("Prevents unmarked traffic from falling back to main."),
                    ),
                ],
                route_row: None,
                match_target: None,
                endpoint_host: None,
            });
        }
    } else if platform == RoutePlanPlatform::Windows && full_tunnel.any() {
        if full_tunnel.ipv4 {
            items.push(RoutePlanItem {
                id: "metric-v4".to_string(),
                title: "IPv4 full-tunnel metric".to_string(),
                subtitle: "Windows lowers interface metric for the tunnel".to_string(),
                family: Some(RoutePlanFamily::Ipv4),
                static_status: table_off.then_some(RoutePlanStaticStatus::Skipped),
                event_patterns: vec!["interface metric set: v4".to_string()],
                chips: vec![
                    super::util::chip("Metric", RoutePlanTone::Info),
                    super::util::chip("IPv4", RoutePlanTone::Secondary),
                    super::util::chip(format!("table {route_table_text}"), RoutePlanTone::Secondary),
                ],
                inspector: RoutePlanInspector {
                    title: "IPv4 full-tunnel metric".to_string(),
                    subtitle: "Windows uses interface priority rather than policy rules.".to_string(),
                    why_match: vec!["Full tunnel is active for IPv4.".to_string()],
                    platform_details: vec![
                        "The tunnel adapter metric is lowered so 0.0.0.0/0 prefers the tunnel.".to_string(),
                        "Endpoint and DNS exceptions are handled with host routes.".to_string(),
                    ],
                    risk_assessment: vec![
                        "If metric programming fails, apply aborts rather than leaving a half-configured full tunnel.".to_string(),
                    ],
                },
                graph_steps: vec![
                    super::util::graph_step(
                        RoutePlanStepKind::Interface,
                        "Local Interface",
                        &super::util::interface_label(parsed),
                        None,
                    ),
                    super::util::graph_step(
                        RoutePlanStepKind::Policy,
                        "Interface Metric",
                        "50",
                        Some("Lower metric makes full tunnel preferred."),
                    ),
                    super::util::graph_step(
                        RoutePlanStepKind::Policy,
                        "Config Table",
                        &route_table_text,
                        Some("RouteTable IDs are ignored on Windows."),
                    ),
                ],
                route_row: None,
                match_target: None,
                endpoint_host: None,
            });
        }

        if full_tunnel.ipv6 {
            items.push(RoutePlanItem {
                id: "metric-v6".to_string(),
                title: "IPv6 full-tunnel metric".to_string(),
                subtitle: "Windows lowers interface metric for the tunnel".to_string(),
                family: Some(RoutePlanFamily::Ipv6),
                static_status: table_off.then_some(RoutePlanStaticStatus::Skipped),
                event_patterns: vec!["interface metric set: v6".to_string()],
                chips: vec![
                    super::util::chip("Metric", RoutePlanTone::Info),
                    super::util::chip("IPv6", RoutePlanTone::Secondary),
                    super::util::chip(format!("table {route_table_text}"), RoutePlanTone::Secondary),
                ],
                inspector: RoutePlanInspector {
                    title: "IPv6 full-tunnel metric".to_string(),
                    subtitle: "Windows uses interface priority rather than policy rules.".to_string(),
                    why_match: vec!["Full tunnel is active for IPv6.".to_string()],
                    platform_details: vec![
                        "The tunnel adapter metric is lowered so ::/0 prefers the tunnel.".to_string(),
                        "Endpoint and DNS exceptions are handled with host routes.".to_string(),
                    ],
                    risk_assessment: vec![
                        "If metric programming fails, apply aborts rather than leaving a half-configured full tunnel.".to_string(),
                    ],
                },
                graph_steps: vec![
                    super::util::graph_step(
                        RoutePlanStepKind::Interface,
                        "Local Interface",
                        &super::util::interface_label(parsed),
                        None,
                    ),
                    super::util::graph_step(
                        RoutePlanStepKind::Policy,
                        "Interface Metric",
                        "50",
                        Some("Lower metric makes full tunnel preferred."),
                    ),
                    super::util::graph_step(
                        RoutePlanStepKind::Policy,
                        "Config Table",
                        &route_table_text,
                        Some("RouteTable IDs are ignored on Windows."),
                    ),
                ],
                route_row: None,
                match_target: None,
                endpoint_host: None,
            });
        }
    }

    items
}

fn build_warning_items(
    platform: RoutePlanPlatform,
    parsed: &WireGuardConfig,
    routes: &[AllowedIp],
    full_tunnel: FullTunnelStatus,
) -> Vec<RoutePlanItem> {
    let mut items = Vec::new();

    if parsed.peers.is_empty() {
        items.push(warning_item(
            "warning-no-peers",
            "No peers configured",
            "The route plan cannot carry traffic without at least one peer.",
            vec![
                "No [Peer] sections exist in the current config.".to_string(),
                "Route Map can still preview interface settings, but no destination will reach a tunnel peer.".to_string(),
            ],
        ));
    }

    if parsed.interface.table == Some(RouteTable::Off) && !routes.is_empty() {
        items.push(warning_item(
            "warning-table-off",
            "Table=Off disables route apply",
            "Matches still exist logically, but the backend skips route installation.",
            vec![
                "AllowedIPs remain useful for explanation, but they will not become kernel routes."
                    .to_string(),
                "This is safe only if the user expects to manage routes out-of-band.".to_string(),
            ],
        ));
    }

    if full_tunnel.any() && parsed.interface.dns_servers.is_empty() {
        items.push(warning_item(
            "warning-no-dns",
            "Full tunnel without tunnel DNS",
            "Traffic may be protected, but DNS intent is ambiguous.",
            vec![
                "No DNS servers are configured inside the tunnel.".to_string(),
                "Users should verify whether system DNS is acceptable for this profile."
                    .to_string(),
            ],
        ));
    }

    if platform == RoutePlanPlatform::Linux
        && full_tunnel.any()
        && parsed.interface.fwmark.is_none()
    {
        items.push(warning_item(
            "warning-fwmark",
            "Full tunnel needs fwmark",
            "Linux policy routing depends on fwmark to keep endpoint traffic on main.",
            vec![
                "The engine currently auto-fills a default fwmark when full tunnel is detected.".to_string(),
                "Route Map surfaces this because a mismatch here breaks trust in the route decision.".to_string(),
            ],
        ));
    }

    items
}

fn warning_item(
    id: &str,
    title: &str,
    subtitle: &str,
    risk_assessment: Vec<String>,
) -> RoutePlanItem {
    RoutePlanItem {
        id: id.to_string(),
        title: title.to_string(),
        subtitle: subtitle.to_string(),
        family: None,
        static_status: Some(RoutePlanStaticStatus::Warning),
        event_patterns: Vec::new(),
        chips: vec![super::util::chip("Guardrail", RoutePlanTone::Warning)],
        inspector: RoutePlanInspector {
            title: title.to_string(),
            subtitle: subtitle.to_string(),
            why_match: vec![
                "This item is derived from config semantics rather than a single route."
                    .to_string(),
            ],
            platform_details: vec![
                "Use this section to spot leak-prone or intentionally disabled behaviour."
                    .to_string(),
            ],
            risk_assessment,
        },
        graph_steps: vec![super::util::graph_step(
            RoutePlanStepKind::Guardrail,
            "Guardrail",
            title,
            Some(subtitle),
        )],
        route_row: None,
        match_target: None,
        endpoint_host: None,
    }
}

fn group(id: &str, label: &str, items: Vec<RoutePlanItem>, empty_note: &str) -> RoutePlanGroup {
    RoutePlanGroup {
        id: id.to_string(),
        label: label.to_string(),
        empty_note: empty_note.to_string(),
        items,
    }
}
