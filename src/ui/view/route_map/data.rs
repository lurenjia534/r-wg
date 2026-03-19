use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use gpui::SharedString;
use gpui_component::IconName;
use r_wg::backend::wg::config::{AllowedIp, RouteTable};
use r_wg::log;

use crate::ui::format::format_route_table;
use crate::ui::state::{RouteFamilyFilter, WgApp};
use crate::ui::view::data::ViewData;

const LINUX_POLICY_TABLE: u32 = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RouteMapItemStatus {
    Planned,
    Applied,
    Skipped,
    Failed,
    Warning,
}

impl RouteMapItemStatus {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Planned => "Planned",
            Self::Applied => "Applied",
            Self::Skipped => "Skipped",
            Self::Failed => "Failed",
            Self::Warning => "Warning",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RouteMapTone {
    Secondary,
    Info,
    Success,
    Warning,
}

#[derive(Clone)]
pub(crate) struct RouteMapChip {
    pub(crate) label: SharedString,
    pub(crate) tone: RouteMapTone,
}

#[derive(Clone)]
pub(crate) struct RouteMapGraphStep {
    pub(crate) icon: IconName,
    pub(crate) label: SharedString,
    pub(crate) value: SharedString,
    pub(crate) note: Option<SharedString>,
}

#[derive(Clone)]
pub(crate) struct RouteMapInspector {
    pub(crate) title: SharedString,
    pub(crate) subtitle: SharedString,
    pub(crate) why_match: Vec<SharedString>,
    pub(crate) platform_details: Vec<SharedString>,
    pub(crate) runtime_evidence: Vec<SharedString>,
    pub(crate) risk_assessment: Vec<SharedString>,
}

#[derive(Clone)]
pub(crate) struct RouteMapRouteRow {
    pub(crate) destination: SharedString,
    pub(crate) family: SharedString,
    pub(crate) kind: SharedString,
    pub(crate) peer: SharedString,
    pub(crate) endpoint: SharedString,
    pub(crate) table: SharedString,
    pub(crate) status: SharedString,
    pub(crate) note: SharedString,
}

#[derive(Clone)]
pub(crate) struct RouteMapMatchTarget {
    pub(crate) addr: IpAddr,
    pub(crate) cidr: u8,
}

#[derive(Clone)]
pub(crate) struct RouteMapInventoryItem {
    pub(crate) id: SharedString,
    pub(crate) title: SharedString,
    pub(crate) subtitle: SharedString,
    pub(crate) family: Option<RouteFamilyFilter>,
    pub(crate) status: RouteMapItemStatus,
    pub(crate) chips: Vec<RouteMapChip>,
    pub(crate) inspector: RouteMapInspector,
    pub(crate) graph_steps: Vec<RouteMapGraphStep>,
    pub(crate) route_row: Option<RouteMapRouteRow>,
    pub(crate) match_target: Option<RouteMapMatchTarget>,
    pub(crate) endpoint_host: Option<SharedString>,
}

#[derive(Clone)]
pub(crate) struct RouteMapInventoryGroup {
    pub(crate) id: SharedString,
    pub(crate) label: SharedString,
    pub(crate) summary: SharedString,
    pub(crate) empty_note: SharedString,
    pub(crate) items: Vec<RouteMapInventoryItem>,
}

#[derive(Clone)]
pub(crate) struct RouteMapExplainResult {
    pub(crate) query: SharedString,
    pub(crate) headline: SharedString,
    pub(crate) summary: SharedString,
    pub(crate) steps: Vec<SharedString>,
    pub(crate) risk: Vec<SharedString>,
    pub(crate) matched_item_id: Option<SharedString>,
}

pub(crate) struct RouteMapData {
    pub(crate) has_plan: bool,
    pub(crate) plan_status: SharedString,
    pub(crate) source_label: SharedString,
    pub(crate) platform_label: SharedString,
    pub(crate) summary_chips: Vec<RouteMapChip>,
    pub(crate) parse_error: Option<SharedString>,
    pub(crate) inventory_groups: Vec<RouteMapInventoryGroup>,
    pub(crate) route_rows: Vec<RouteMapRouteRow>,
    pub(crate) net_events: Vec<SharedString>,
    pub(crate) explain: Option<RouteMapExplainResult>,
    pub(crate) selected_item_id: Option<SharedString>,
    pub(crate) selected_item: Option<RouteMapInventoryItem>,
}

impl RouteMapData {
    pub(crate) fn new(app: &WgApp, data: &ViewData, search_query: &str) -> Self {
        let source_label = current_source_label(app, data);
        let platform_label: SharedString = platform_label().into();
        let net_events_raw = recent_net_events();
        let net_events = net_events_raw.iter().cloned().map(Into::into).collect();
        let search_query = search_query.trim().to_string();

        let Some(parsed) = data.parsed_config.as_ref() else {
            return Self {
                has_plan: false,
                plan_status: if let Some(parse_error) = data.parse_error.as_ref() {
                    format!("Config invalid: {parse_error}").into()
                } else {
                    "Select or validate a config to build a route plan.".into()
                },
                source_label,
                platform_label,
                summary_chips: vec![
                    chip("Planned", RouteMapTone::Secondary),
                    chip("No config", RouteMapTone::Warning),
                ],
                parse_error: data.parse_error.clone().map(Into::into),
                inventory_groups: Vec::new(),
                route_rows: Vec::new(),
                net_events,
                explain: build_empty_explain(&search_query),
                selected_item_id: None,
                selected_item: None,
            };
        };

        let routes = collect_allowed_routes(&parsed.peers);
        let (full_v4, full_v6) = detect_full_tunnel(&routes);
        let route_table_text = format_route_table(parsed.interface.table);
        let full_tunnel = full_v4 || full_v6;
        let table_off = parsed.interface.table == Some(RouteTable::Off);
        let dns_guard_text = dns_guard_label(full_tunnel, parsed.interface.dns_servers.is_empty());
        let mut groups = build_inventory_groups(parsed, &routes, &net_events_raw);

        apply_filters(
            &mut groups,
            app.ui_session.route_map_family_filter,
            search_query.as_str(),
        );

        let route_rows = groups
            .iter()
            .flat_map(|group| group.items.iter())
            .filter_map(|item| item.route_row.clone())
            .collect::<Vec<_>>();

        let explain = build_explain(&groups, &search_query);
        let selected_item = resolve_selected_item(
            &groups,
            app.ui_session.route_map_selected_item.as_ref(),
            explain
                .as_ref()
                .and_then(|value| value.matched_item_id.as_ref()),
        );
        let selected_item_id = selected_item.as_ref().map(|item| item.id.clone());

        let mut summary_chips = vec![
            chip(
                if full_tunnel {
                    "Full Tunnel"
                } else {
                    "Split Tunnel"
                },
                if full_tunnel {
                    RouteMapTone::Success
                } else {
                    RouteMapTone::Info
                },
            ),
            chip(
                if full_v4 { "IPv4 Full" } else { "IPv4 Split" },
                if full_v4 {
                    RouteMapTone::Success
                } else {
                    RouteMapTone::Secondary
                },
            ),
            chip(
                if full_v6 { "IPv6 Full" } else { "IPv6 Split" },
                if full_v6 {
                    RouteMapTone::Success
                } else {
                    RouteMapTone::Secondary
                },
            ),
            chip(
                dns_guard_text,
                if full_tunnel {
                    RouteMapTone::Info
                } else {
                    RouteMapTone::Secondary
                },
            ),
            chip(
                format!("Bypass {}", planned_bypass_count(parsed, full_v4, full_v6)),
                if cfg!(target_os = "windows") && full_tunnel {
                    RouteMapTone::Info
                } else {
                    RouteMapTone::Secondary
                },
            ),
            chip(format!("Table {route_table_text}"), RouteMapTone::Secondary),
            chip(
                format!("Applied {}", net_events_raw.len()),
                if net_events_raw.is_empty() {
                    RouteMapTone::Secondary
                } else {
                    RouteMapTone::Info
                },
            ),
            chip(
                format!("Routes {}", route_rows.len()),
                RouteMapTone::Secondary,
            ),
        ];
        if table_off {
            summary_chips.insert(1, chip("Table Off", RouteMapTone::Warning));
        }

        Self {
            has_plan: true,
            plan_status: if table_off {
                "Plan generated, but route apply is disabled by Table=Off.".into()
            } else if full_tunnel {
                "Planned view combines effective routes, full-tunnel guardrails, and runtime evidence.".into()
            } else {
                "Planned view combines effective routes and runtime evidence for the current config.".into()
            },
            source_label,
            platform_label,
            summary_chips,
            parse_error: data.parse_error.clone().map(Into::into),
            inventory_groups: groups,
            route_rows,
            net_events,
            explain,
            selected_item_id,
            selected_item,
        }
    }
}

fn build_inventory_groups(
    parsed: &r_wg::backend::wg::config::WireGuardConfig,
    routes: &[AllowedIp],
    net_events: &[String],
) -> Vec<RouteMapInventoryGroup> {
    let (full_v4, full_v6) = detect_full_tunnel(routes);
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

            let route_text = route_text(allowed.addr, allowed.cidr);
            let family_filter = family_from_ip(allowed.addr);
            let family = Some(family_filter);
            let is_full = is_full_tunnel_route(allowed);
            let effective_table =
                effective_route_table(parsed.interface.table, allowed, full_v4, full_v6);
            let patterns = vec![route_text.clone()];
            let status = if table_off {
                RouteMapItemStatus::Skipped
            } else {
                status_from_events(net_events, &patterns)
            };
            let runtime_evidence = matching_events(net_events, &patterns);
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
                id: item_id("allowed", &route_text),
                title: route_text.clone().into(),
                subtitle: format!("{peer_label} via {endpoint_text}").into(),
                family,
                status,
                chips: vec![
                    chip(
                        if is_full {
                            "Full Tunnel"
                        } else {
                            "Split Route"
                        },
                        if is_full {
                            RouteMapTone::Success
                        } else {
                            RouteMapTone::Info
                        },
                    ),
                    chip(family_filter.label(), RouteMapTone::Secondary),
                    chip(peer_label.clone(), RouteMapTone::Secondary),
                ],
                inspector: RouteMapInspector {
                    title: route_text.clone().into(),
                    subtitle: format!("{peer_label} advertises this prefix.").into(),
                    why_match: vec![
                        format!("Matched by AllowedIPs on {peer_label}.").into(),
                        format!("Endpoint for this peer is {endpoint_text}.").into(),
                    ],
                    platform_details: allowed_platform_details(
                        allowed,
                        effective_table.as_str(),
                        parsed.interface.table,
                        parsed.interface.fwmark,
                        full_v4,
                        full_v6,
                    ),
                    runtime_evidence,
                    risk_assessment: allowed_risks(parsed, allowed, is_full),
                },
                graph_steps: allowed_graph_steps(
                    &interface_label,
                    &route_table_text,
                    &peer_label,
                    &endpoint_text,
                    &route_text,
                    &dns_label,
                    is_full,
                ),
                route_row: Some(RouteMapRouteRow {
                    destination: route_text.clone().into(),
                    family: family_filter.label().into(),
                    kind: route_kind.into(),
                    peer: peer_label.clone().into(),
                    endpoint: endpoint_text.clone().into(),
                    table: effective_table.into(),
                    status: status.label().into(),
                    note: note.into(),
                }),
                match_target: Some(RouteMapMatchTarget {
                    addr: allowed.addr,
                    cidr: allowed.cidr,
                }),
                endpoint_host: peer
                    .endpoint
                    .as_ref()
                    .map(|endpoint| endpoint.host.clone().into()),
            };

            if is_full {
                full_tunnel_items.push(item);
            } else {
                split_route_items.push(item);
            }
        }
    }

    let endpoint_bypass_items = build_endpoint_bypass_items(parsed, full_v4, full_v6, net_events);
    let dns_route_items = build_dns_route_items(parsed, full_v4, full_v6, net_events);
    let policy_items = build_policy_items(parsed, full_v4, full_v6, table_off, net_events);
    let warning_items = build_warning_items(parsed, routes, full_v4, full_v6);

    vec![
        group(
            "group-full-tunnel",
            "Full Tunnel",
            full_tunnel_items,
            if full_v4 || full_v6 {
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
            if cfg!(target_os = "windows") {
                "Endpoint escape routes only exist when full tunnel needs them."
            } else {
                "Linux uses policy routing instead of explicit endpoint bypass routes."
            },
        ),
        group(
            "group-dns-routes",
            "DNS Routes",
            dns_route_items,
            if cfg!(target_os = "windows") {
                "Host routes that keep tunnel DNS reachable under full tunnel."
            } else {
                "No dedicated DNS host routes are planned on this platform."
            },
        ),
        group(
            "group-policy-rules",
            "Policy Rules",
            policy_items,
            if cfg!(target_os = "linux") {
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
    parsed: &r_wg::backend::wg::config::WireGuardConfig,
    full_v4: bool,
    full_v6: bool,
    net_events: &[String],
) -> Vec<RouteMapInventoryItem> {
    if !cfg!(target_os = "windows") || !(full_v4 || full_v6) {
        return Vec::new();
    }

    let interface_label = interface_label(parsed);
    let mut items = Vec::new();
    for (peer_index, peer) in parsed.peers.iter().enumerate() {
        let Some(endpoint) = peer.endpoint.as_ref() else {
            continue;
        };
        let peer_label = format!("Peer {}", peer_index + 1);
        let endpoint_text = format!("{}:{}", endpoint.host, endpoint.port);
        let endpoint_ip = endpoint.host.parse::<IpAddr>().ok();
        let applies = match endpoint_ip {
            Some(IpAddr::V4(_)) => full_v4,
            Some(IpAddr::V6(_)) => full_v6,
            None => full_v4 || full_v6,
        };
        if !applies {
            continue;
        }

        match endpoint_ip {
            Some(ip) => {
                let route_text = route_text(ip, if ip.is_ipv4() { 32 } else { 128 });
                let patterns = vec![
                    ip.to_string(),
                    format!("bypass route add: {}", ip),
                    route_text.clone(),
                ];
                let status = status_from_events(net_events, &patterns);
                let runtime_evidence = matching_events(net_events, &patterns);
                items.push(RouteMapInventoryItem {
                    id: item_id("bypass", &endpoint.host),
                    title: endpoint.host.clone().into(),
                    subtitle: format!("{peer_label} endpoint bypass").into(),
                    family: Some(family_from_ip(ip)),
                    status,
                    chips: vec![
                        chip("Bypass", RouteMapTone::Info),
                        chip(family_from_ip(ip).label(), RouteMapTone::Secondary),
                        chip(peer_label.clone(), RouteMapTone::Secondary),
                    ],
                    inspector: RouteMapInspector {
                        title: endpoint.host.clone().into(),
                        subtitle: "Endpoint must stay on the underlay path.".into(),
                        why_match: vec![
                            format!("Full tunnel would otherwise capture traffic to {endpoint_text}.").into(),
                            "Windows creates a host route outside the tunnel for endpoint reachability.".into(),
                        ],
                        platform_details: vec![
                            "Windows resolves or reuses endpoint IPs before installing bypass routes.".into(),
                            "The bypass route follows the system best route instead of the tunnel adapter.".into(),
                        ],
                        runtime_evidence,
                        risk_assessment: vec![
                            "If this bypass route cannot be installed, full-tunnel apply aborts to avoid leaks.".into(),
                        ],
                    },
                    graph_steps: vec![
                        graph_step(
                            IconName::LayoutDashboard,
                            "Local Interface",
                            &interface_label,
                            None,
                        ),
                        graph_step(
                            IconName::CircleCheck,
                            "Guardrail",
                            "Full tunnel requires endpoint escape",
                            Some("The endpoint must not recurse into the tunnel."),
                        ),
                        graph_step(
                            IconName::ArrowRight,
                            "Bypass Route",
                            &route_text,
                            Some("Uses the current system best route."),
                        ),
                        graph_step(IconName::Globe, "Endpoint", &endpoint_text, None),
                    ],
                    route_row: Some(RouteMapRouteRow {
                        destination: route_text.clone().into(),
                        family: family_from_ip(ip).label().into(),
                        kind: "bypass".into(),
                        peer: peer_label.clone().into(),
                        endpoint: endpoint_text.clone().into(),
                        table: "system route".into(),
                        status: status.label().into(),
                        note: "Host route keeps the endpoint outside the tunnel.".into(),
                    }),
                    match_target: Some(RouteMapMatchTarget {
                        addr: ip,
                        cidr: if ip.is_ipv4() { 32 } else { 128 },
                    }),
                    endpoint_host: Some(endpoint.host.clone().into()),
                });
            }
            None => {
                items.push(RouteMapInventoryItem {
                    id: item_id("bypass-pending", &endpoint.host),
                    title: endpoint.host.clone().into(),
                    subtitle: format!("{peer_label} endpoint hostname").into(),
                    family: None,
                    status: RouteMapItemStatus::Warning,
                    chips: vec![
                        chip("Bypass Pending", RouteMapTone::Warning),
                        chip(peer_label.clone(), RouteMapTone::Secondary),
                    ],
                    inspector: RouteMapInspector {
                        title: endpoint.host.clone().into(),
                        subtitle: "Needs runtime resolution before bypass exists.".into(),
                        why_match: vec![
                            format!("Endpoint hostname {endpoint_text} cannot be resolved from config alone.").into(),
                        ],
                        platform_details: vec![
                            "Windows resolves endpoint hostnames inside the privileged backend during apply.".into(),
                            "The UI keeps this item visible so the user can verify the leak guard path.".into(),
                        ],
                        runtime_evidence: matching_events(net_events, &[endpoint.host.clone()]),
                        risk_assessment: vec![
                            "Until endpoint resolution succeeds, the bypass route is not concrete.".into(),
                            "The backend will abort full-tunnel apply if resolution/bypass fails.".into(),
                        ],
                    },
                    graph_steps: vec![
                        graph_step(
                            IconName::LayoutDashboard,
                            "Local Interface",
                            &interface_label,
                            None,
                        ),
                        graph_step(
                            IconName::CircleX,
                            "Guardrail",
                            "Endpoint bypass pending",
                            Some("Runtime DNS resolution is required first."),
                        ),
                        graph_step(IconName::Globe, "Endpoint Host", &endpoint_text, None),
                    ],
                    route_row: None,
                    match_target: None,
                    endpoint_host: Some(endpoint.host.clone().into()),
                });
            }
        }
    }
    items
}

fn build_dns_route_items(
    parsed: &r_wg::backend::wg::config::WireGuardConfig,
    full_v4: bool,
    full_v6: bool,
    net_events: &[String],
) -> Vec<RouteMapInventoryItem> {
    if !cfg!(target_os = "windows") || parsed.interface.table == Some(RouteTable::Off) {
        return Vec::new();
    }

    let interface_label = interface_label(parsed);
    let mut items = Vec::new();
    for dns_server in &parsed.interface.dns_servers {
        let applies = (dns_server.is_ipv4() && full_v4) || (dns_server.is_ipv6() && full_v6);
        if !applies {
            continue;
        }

        let route_text = route_text(*dns_server, if dns_server.is_ipv4() { 32 } else { 128 });
        let patterns = vec![
            route_text.clone(),
            format!("dns route add: {}", route_text),
            dns_server.to_string(),
        ];
        let status = status_from_events(net_events, &patterns);
        items.push(RouteMapInventoryItem {
            id: item_id("dns-route", &dns_server.to_string()),
            title: dns_server.to_string().into(),
            subtitle: "Tunnel DNS host route".into(),
            family: Some(family_from_ip(*dns_server)),
            status,
            chips: vec![
                chip("DNS Route", RouteMapTone::Info),
                chip(family_from_ip(*dns_server).label(), RouteMapTone::Secondary),
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
                runtime_evidence: matching_events(net_events, &patterns),
                risk_assessment: vec![
                    "Without the host route, a more specific underlay route could bypass tunnel DNS.".into(),
                ],
            },
            graph_steps: vec![
                graph_step(
                    IconName::LayoutDashboard,
                    "Local Interface",
                    &interface_label,
                    None,
                ),
                graph_step(
                    IconName::Search,
                    "Tunnel DNS",
                    &dns_server.to_string(),
                    Some("Pinned with a host route under full tunnel."),
                ),
                graph_step(
                    IconName::ArrowRight,
                    "Route",
                    &route_text,
                    Some("Installed on the tunnel adapter."),
                ),
            ],
            route_row: Some(RouteMapRouteRow {
                destination: route_text.clone().into(),
                family: family_from_ip(*dns_server).label().into(),
                kind: "dns".into(),
                peer: "Interface DNS".into(),
                endpoint: "-".into(),
                table: "tunnel adapter".into(),
                status: status.label().into(),
                note: "Host route protects tunnel DNS reachability.".into(),
            }),
            match_target: Some(RouteMapMatchTarget {
                addr: *dns_server,
                cidr: if dns_server.is_ipv4() { 32 } else { 128 },
            }),
            endpoint_host: None,
        });
    }
    items
}

fn build_policy_items(
    parsed: &r_wg::backend::wg::config::WireGuardConfig,
    full_v4: bool,
    full_v6: bool,
    table_off: bool,
    net_events: &[String],
) -> Vec<RouteMapInventoryItem> {
    let route_table_text = format_route_table(parsed.interface.table);
    let mut items = Vec::new();

    if cfg!(target_os = "linux") {
        let Some(table_id) = linux_policy_table(parsed.interface.table, full_v4, full_v6) else {
            return items;
        };

        if full_v4 {
            let patterns = vec!["policy rule add: v4".to_string()];
            items.push(RouteMapInventoryItem {
                id: "policy-v4".into(),
                title: "IPv4 policy routing".into(),
                subtitle: format!("fwmark -> main, not fwmark -> table {table_id}").into(),
                family: Some(RouteFamilyFilter::Ipv4),
                status: if table_off {
                    RouteMapItemStatus::Skipped
                } else {
                    status_from_events(net_events, &patterns)
                },
                chips: vec![
                    chip("Policy", RouteMapTone::Info),
                    chip("IPv4", RouteMapTone::Secondary),
                    chip(format!("table {table_id}"), RouteMapTone::Secondary),
                ],
                inspector: RouteMapInspector {
                    title: "IPv4 policy routing".into(),
                    subtitle: "Linux full tunnel keeps engine traffic on main.".into(),
                    why_match: vec![
                        "Full tunnel is active for IPv4.".into(),
                        "Linux uses fwmark rules instead of endpoint bypass routes.".into(),
                    ],
                    platform_details: vec![
                        format!(
                            "Traffic with fwmark 0x{:x} stays on main; unmarked traffic enters table {}.",
                            parsed.interface.fwmark.unwrap_or_default(),
                            table_id
                        )
                        .into(),
                        "A suppress-main rule prevents business traffic from falling back to the default route.".into(),
                    ],
                    runtime_evidence: matching_events(net_events, &patterns),
                    risk_assessment: vec![
                        "Missing fwmark or failed rule install would make full-tunnel routing unsafe.".into(),
                    ],
                },
                graph_steps: vec![
                    graph_step(
                        IconName::LayoutDashboard,
                        "Local Interface",
                        &interface_label(parsed),
                        None,
                    ),
                    graph_step(
                        IconName::CircleCheck,
                        "fwmark",
                        &format!("0x{:x}", parsed.interface.fwmark.unwrap_or_default()),
                        Some("Engine traffic stays on main."),
                    ),
                    graph_step(
                        IconName::Map,
                        "Policy Table",
                        &format!("table {table_id}"),
                        Some("Business traffic enters the tunnel table."),
                    ),
                    graph_step(
                        IconName::CircleX,
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

        if full_v6 {
            let patterns = vec!["policy rule add: v6".to_string()];
            items.push(RouteMapInventoryItem {
                id: "policy-v6".into(),
                title: "IPv6 policy routing".into(),
                subtitle: format!("fwmark -> main, not fwmark -> table {table_id}").into(),
                family: Some(RouteFamilyFilter::Ipv6),
                status: if table_off {
                    RouteMapItemStatus::Skipped
                } else {
                    status_from_events(net_events, &patterns)
                },
                chips: vec![
                    chip("Policy", RouteMapTone::Info),
                    chip("IPv6", RouteMapTone::Secondary),
                    chip(format!("table {table_id}"), RouteMapTone::Secondary),
                ],
                inspector: RouteMapInspector {
                    title: "IPv6 policy routing".into(),
                    subtitle: "Linux full tunnel keeps engine traffic on main.".into(),
                    why_match: vec![
                        "Full tunnel is active for IPv6.".into(),
                        "Linux uses fwmark rules instead of endpoint bypass routes.".into(),
                    ],
                    platform_details: vec![
                        format!(
                            "Traffic with fwmark 0x{:x} stays on main; unmarked traffic enters table {}.",
                            parsed.interface.fwmark.unwrap_or_default(),
                            table_id
                        )
                        .into(),
                        "A suppress-main rule prevents business traffic from falling back to the default route.".into(),
                    ],
                    runtime_evidence: matching_events(net_events, &patterns),
                    risk_assessment: vec![
                        "Missing fwmark or failed rule install would make full-tunnel routing unsafe.".into(),
                    ],
                },
                graph_steps: vec![
                    graph_step(
                        IconName::LayoutDashboard,
                        "Local Interface",
                        &interface_label(parsed),
                        None,
                    ),
                    graph_step(
                        IconName::CircleCheck,
                        "fwmark",
                        &format!("0x{:x}", parsed.interface.fwmark.unwrap_or_default()),
                        Some("Engine traffic stays on main."),
                    ),
                    graph_step(
                        IconName::Map,
                        "Policy Table",
                        &format!("table {table_id}"),
                        Some("Business traffic enters the tunnel table."),
                    ),
                    graph_step(
                        IconName::CircleX,
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
    } else if cfg!(target_os = "windows") && (full_v4 || full_v6) {
        if full_v4 {
            let patterns = vec!["interface metric set: v4".to_string()];
            items.push(RouteMapInventoryItem {
                id: "metric-v4".into(),
                title: "IPv4 full-tunnel metric".into(),
                subtitle: "Windows lowers interface metric for the tunnel".into(),
                family: Some(RouteFamilyFilter::Ipv4),
                status: if table_off {
                    RouteMapItemStatus::Skipped
                } else {
                    status_from_events(net_events, &patterns)
                },
                chips: vec![
                    chip("Metric", RouteMapTone::Info),
                    chip("IPv4", RouteMapTone::Secondary),
                    chip(format!("table {route_table_text}"), RouteMapTone::Secondary),
                ],
                inspector: RouteMapInspector {
                    title: "IPv4 full-tunnel metric".into(),
                    subtitle: "Windows uses interface priority rather than policy rules.".into(),
                    why_match: vec![
                        "Full tunnel is active for IPv4.".into(),
                    ],
                    platform_details: vec![
                        "The tunnel adapter metric is lowered so 0.0.0.0/0 prefers the tunnel.".into(),
                        "Endpoint and DNS exceptions are handled with host routes.".into(),
                    ],
                    runtime_evidence: matching_events(net_events, &patterns),
                    risk_assessment: vec![
                        "If metric programming fails, apply aborts rather than leaving a half-configured full tunnel.".into(),
                    ],
                },
                graph_steps: vec![
                    graph_step(
                        IconName::LayoutDashboard,
                        "Local Interface",
                        &interface_label(parsed),
                        None,
                    ),
                    graph_step(
                        IconName::SortAscending,
                        "Interface Metric",
                        "50",
                        Some("Lower metric makes full tunnel preferred."),
                    ),
                    graph_step(IconName::Map, "Config Table", &route_table_text, Some("RouteTable IDs are ignored on Windows.")),
                ],
                route_row: None,
                match_target: None,
                endpoint_host: None,
            });
        }

        if full_v6 {
            let patterns = vec!["interface metric set: v6".to_string()];
            items.push(RouteMapInventoryItem {
                id: "metric-v6".into(),
                title: "IPv6 full-tunnel metric".into(),
                subtitle: "Windows lowers interface metric for the tunnel".into(),
                family: Some(RouteFamilyFilter::Ipv6),
                status: if table_off {
                    RouteMapItemStatus::Skipped
                } else {
                    status_from_events(net_events, &patterns)
                },
                chips: vec![
                    chip("Metric", RouteMapTone::Info),
                    chip("IPv6", RouteMapTone::Secondary),
                    chip(format!("table {route_table_text}"), RouteMapTone::Secondary),
                ],
                inspector: RouteMapInspector {
                    title: "IPv6 full-tunnel metric".into(),
                    subtitle: "Windows uses interface priority rather than policy rules.".into(),
                    why_match: vec![
                        "Full tunnel is active for IPv6.".into(),
                    ],
                    platform_details: vec![
                        "The tunnel adapter metric is lowered so ::/0 prefers the tunnel.".into(),
                        "Endpoint and DNS exceptions are handled with host routes.".into(),
                    ],
                    runtime_evidence: matching_events(net_events, &patterns),
                    risk_assessment: vec![
                        "If metric programming fails, apply aborts rather than leaving a half-configured full tunnel.".into(),
                    ],
                },
                graph_steps: vec![
                    graph_step(
                        IconName::LayoutDashboard,
                        "Local Interface",
                        &interface_label(parsed),
                        None,
                    ),
                    graph_step(
                        IconName::SortAscending,
                        "Interface Metric",
                        "50",
                        Some("Lower metric makes full tunnel preferred."),
                    ),
                    graph_step(IconName::Map, "Config Table", &route_table_text, Some("RouteTable IDs are ignored on Windows.")),
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
    parsed: &r_wg::backend::wg::config::WireGuardConfig,
    routes: &[AllowedIp],
    full_v4: bool,
    full_v6: bool,
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

    if parsed.interface.table == Some(RouteTable::Off) && !routes.is_empty() {
        items.push(warning_item(
            "warning-table-off",
            "Table=Off disables route apply",
            "Matches still exist logically, but the backend skips route installation.",
            vec![
                "AllowedIPs remain useful for explanation, but they will not become kernel routes."
                    .into(),
                "This is safe only if the user expects to manage routes out-of-band.".into(),
            ],
        ));
    }

    if (full_v4 || full_v6) && parsed.interface.dns_servers.is_empty() {
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

    if cfg!(target_os = "linux") && (full_v4 || full_v6) && parsed.interface.fwmark.is_none() {
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
        status: RouteMapItemStatus::Warning,
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
            IconName::CircleX,
            "Guardrail",
            title,
            Some(subtitle),
        )],
        route_row: None,
        match_target: None,
        endpoint_host: None,
    }
}

fn group(
    id: &str,
    label: &str,
    items: Vec<RouteMapInventoryItem>,
    empty_note: &str,
) -> RouteMapInventoryGroup {
    let count = items.len();
    RouteMapInventoryGroup {
        id: id.to_string().into(),
        label: label.to_string().into(),
        summary: format!("{count} items").into(),
        empty_note: empty_note.to_string().into(),
        items,
    }
}

fn apply_filters(
    groups: &mut [RouteMapInventoryGroup],
    family_filter: RouteFamilyFilter,
    search_query: &str,
) {
    for group in groups {
        group
            .items
            .retain(|item| item_matches(item, family_filter, search_query));
        group.summary = format!("{} items", group.items.len()).into();
    }
}

fn item_matches(
    item: &RouteMapInventoryItem,
    family_filter: RouteFamilyFilter,
    search_query: &str,
) -> bool {
    let family_match = match family_filter {
        RouteFamilyFilter::All => true,
        _ => item.family.is_none() || item.family == Some(family_filter),
    };
    if !family_match {
        return false;
    }

    let query = search_query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }

    let mut haystack = vec![
        item.title.to_string(),
        item.subtitle.to_string(),
        item.status.label().to_string(),
    ];
    haystack.extend(item.chips.iter().map(|chip| chip.label.to_string()));
    if let Some(route_row) = &item.route_row {
        haystack.push(route_row.destination.to_string());
        haystack.push(route_row.kind.to_string());
        haystack.push(route_row.endpoint.to_string());
        haystack.push(route_row.note.to_string());
    }
    if let Some(endpoint_host) = item.endpoint_host.as_ref() {
        haystack.push(endpoint_host.to_string());
    }

    haystack
        .into_iter()
        .any(|value| value.to_lowercase().contains(&query))
}

fn resolve_selected_item(
    groups: &[RouteMapInventoryGroup],
    requested_id: Option<&SharedString>,
    explain_match_id: Option<&SharedString>,
) -> Option<RouteMapInventoryItem> {
    requested_id
        .and_then(|id| find_item(groups, id.as_ref()))
        .or_else(|| explain_match_id.and_then(|id| find_item(groups, id.as_ref())))
        .or_else(|| {
            groups
                .iter()
                .flat_map(|group| group.items.iter())
                .next()
                .cloned()
        })
}

fn find_item(groups: &[RouteMapInventoryGroup], id: &str) -> Option<RouteMapInventoryItem> {
    groups
        .iter()
        .flat_map(|group| group.items.iter())
        .find(|item| item.id.as_ref() == id)
        .cloned()
}

fn build_explain(
    groups: &[RouteMapInventoryGroup],
    search_query: &str,
) -> Option<RouteMapExplainResult> {
    let query = search_query.trim().to_string();
    if query.is_empty() {
        return Some(RouteMapExplainResult {
            query: "".to_string().into(),
            headline: "Explain a target".into(),
            summary: "Search for an IP, CIDR, or endpoint hostname to see how the current plan resolves it.".into(),
            steps: vec![
                "IP queries pick the most specific planned route or bypass rule.".into(),
                "Hostname queries match configured peer endpoint hosts and call out when runtime resolution is still needed.".into(),
            ],
            risk: Vec::new(),
            matched_item_id: None,
        });
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
            return Some(RouteMapExplainResult {
                query: query.clone().into(),
                headline: format!("{ip} matches {}", item.title).into(),
                summary: item.subtitle.clone(),
                steps: explain_steps_from_item(item, Some(ip)),
                risk: item.inspector.risk_assessment.clone(),
                matched_item_id: Some(item.id.clone()),
            });
        }

        return Some(RouteMapExplainResult {
            query: query.clone().into(),
            headline: format!("{ip} has no planned match").into(),
            summary: "The current config does not advertise a concrete route for this destination.".into(),
            steps: vec![
                "No AllowedIPs, DNS host route, or endpoint bypass rule contains this IP.".into(),
                "If the expectation differs, inspect the inventory for missing prefixes or family filters.".into(),
            ],
            risk: vec!["Unmatched traffic follows the platform default routing policy, not the tunnel plan.".into()],
            matched_item_id: None,
        });
    }

    let host_match = items.iter().find(|item| {
        item.endpoint_host
            .as_ref()
            .map(|host| host.eq_ignore_ascii_case(&query))
            .unwrap_or(false)
    });

    host_match
        .map(|item| RouteMapExplainResult {
            query: query.clone().into(),
            headline: format!("{query} is a configured endpoint").into(),
            summary: item.subtitle.clone(),
            steps: explain_steps_from_item(item, None),
            risk: item.inspector.risk_assessment.clone(),
            matched_item_id: Some(item.id.clone()),
        })
        .or_else(|| {
            Some(RouteMapExplainResult {
                query: query.clone().into(),
                headline: format!("No direct explanation for {query}").into(),
                summary: "Only configured endpoint hosts can be explained without runtime DNS resolution.".into(),
                steps: vec![
                    "Route Map can explain raw IPs, CIDRs, and configured endpoint hostnames.".into(),
                    "Generic domains need a resolved IP before they can be matched against the current route plan.".into(),
                ],
                risk: Vec::new(),
                matched_item_id: None,
            })
        })
}

fn build_empty_explain(search_query: &str) -> Option<RouteMapExplainResult> {
    if search_query.is_empty() {
        None
    } else {
        Some(RouteMapExplainResult {
            query: search_query.to_string().into(),
            headline: "No route plan available".into(),
            summary: "Validate a config first, then Route Map can explain destinations.".into(),
            steps: Vec::new(),
            risk: Vec::new(),
            matched_item_id: None,
        })
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
        graph_step(
            IconName::LayoutDashboard,
            "Local Interface",
            interface_label,
            None,
        ),
        graph_step(
            IconName::Search,
            "DNS / Guard",
            dns_label,
            if full_tunnel {
                Some("Full tunnel makes DNS behaviour part of the routing trust story.")
            } else {
                None
            },
        ),
        graph_step(
            IconName::Map,
            "Route Table",
            route_table_text,
            if full_tunnel {
                Some("Full-tunnel families may add extra guardrails or policy handling.")
            } else {
                None
            },
        ),
        graph_step(
            IconName::CircleUser,
            "Peer",
            peer_label,
            Some(endpoint_text),
        ),
        graph_step(IconName::Map, "Destination Prefix", route_text, None),
    ]
}

fn allowed_platform_details(
    allowed: &AllowedIp,
    effective_table: &str,
    table: Option<RouteTable>,
    fwmark: Option<u32>,
    full_v4: bool,
    full_v6: bool,
) -> Vec<SharedString> {
    let mut details = vec![format!("Effective table: {effective_table}.").into()];
    if cfg!(target_os = "linux") {
        if linux_policy_table(table, full_v4, full_v6).is_some() {
            details.push(
                format!(
                    "Linux may promote {} into the policy table when full tunnel is active.",
                    route_text(allowed.addr, allowed.cidr)
                )
                .into(),
            );
        }
        if let Some(fwmark) = fwmark {
            details.push(format!("fwmark configured: 0x{fwmark:x}.").into());
        }
    } else if cfg!(target_os = "windows") {
        if matches!(table, Some(RouteTable::Id(_))) {
            details.push("Windows ignores explicit RouteTable IDs from the config.".into());
        }
        details
            .push("Windows relies on adapter metric plus host exceptions for full tunnel.".into());
    }
    details
}

fn allowed_risks(
    parsed: &r_wg::backend::wg::config::WireGuardConfig,
    allowed: &AllowedIp,
    is_full: bool,
) -> Vec<SharedString> {
    let mut risks = Vec::new();
    if parsed.interface.table == Some(RouteTable::Off) {
        risks
            .push("Table=Off means this route stays explanatory only and is not installed.".into());
    }
    if is_full && parsed.interface.dns_servers.is_empty() {
        risks.push("Full tunnel is active without tunnel DNS servers.".into());
    }
    if cfg!(target_os = "linux") && is_full && parsed.interface.fwmark.is_none() {
        risks.push(
            "Linux full tunnel depends on fwmark/policy state for safe endpoint escape.".into(),
        );
    }
    if route_is_default_family(allowed, RouteFamilyFilter::Ipv4)
        || route_is_default_family(allowed, RouteFamilyFilter::Ipv6)
    {
        risks.push("Default-route prefixes have the highest blast radius when misapplied.".into());
    }
    if risks.is_empty() {
        risks.push("No elevated risk flagged for this route in the planned model.".into());
    }
    risks
}

fn current_source_label(app: &WgApp, data: &ViewData) -> SharedString {
    let name = app
        .selection
        .selected_id
        .and_then(|id| app.configs.get_by_id(id))
        .map(|config| config.name.clone())
        .unwrap_or_else(|| {
            if data.has_saved_source {
                "Saved config".to_string()
            } else {
                "Unsaved draft".to_string()
            }
        });

    if data.draft_dirty {
        format!("{name} · unsaved changes").into()
    } else {
        name.into()
    }
}

fn recent_net_events() -> Vec<String> {
    log::snapshot()
        .into_iter()
        .filter(|line| line.contains("[net]"))
        .rev()
        .take(40)
        .collect()
}

fn collect_allowed_routes(peers: &[r_wg::backend::wg::config::PeerConfig]) -> Vec<AllowedIp> {
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

fn detect_full_tunnel(routes: &[AllowedIp]) -> (bool, bool) {
    let mut full_v4 = false;
    let mut full_v6 = false;
    for route in routes {
        match route.addr {
            IpAddr::V4(addr) if addr.is_unspecified() && route.cidr == 0 => full_v4 = true,
            IpAddr::V6(addr) if addr.is_unspecified() && route.cidr == 0 => full_v6 = true,
            _ => {}
        }
    }
    (full_v4, full_v6)
}

fn effective_route_table(
    table: Option<RouteTable>,
    route: &AllowedIp,
    full_v4: bool,
    full_v6: bool,
) -> String {
    if cfg!(target_os = "linux") {
        if let Some(policy_table) = linux_policy_table(table, full_v4, full_v6) {
            match route.addr {
                IpAddr::V4(_) if full_v4 => return format!("table {policy_table}"),
                IpAddr::V6(_) if full_v6 => return format!("table {policy_table}"),
                _ => {}
            }
        }
        match table {
            Some(RouteTable::Id(id)) => format!("table {id}"),
            Some(RouteTable::Off) => "off".to_string(),
            _ => "main".to_string(),
        }
    } else if cfg!(target_os = "windows") {
        match table {
            Some(RouteTable::Off) => "off".to_string(),
            _ => "tunnel adapter".to_string(),
        }
    } else {
        format_route_table(table)
    }
}

fn linux_policy_table(table: Option<RouteTable>, full_v4: bool, full_v6: bool) -> Option<u32> {
    if !cfg!(target_os = "linux") || table == Some(RouteTable::Off) || !(full_v4 || full_v6) {
        return None;
    }
    Some(match table {
        Some(RouteTable::Id(id)) => id,
        _ => LINUX_POLICY_TABLE,
    })
}

fn planned_bypass_count(
    parsed: &r_wg::backend::wg::config::WireGuardConfig,
    full_v4: bool,
    full_v6: bool,
) -> usize {
    if !cfg!(target_os = "windows") || !(full_v4 || full_v6) {
        return 0;
    }

    let mut count = 0usize;
    for peer in &parsed.peers {
        let Some(endpoint) = peer.endpoint.as_ref() else {
            continue;
        };
        match endpoint.host.parse::<IpAddr>() {
            Ok(IpAddr::V4(_)) if full_v4 => count += 1,
            Ok(IpAddr::V6(_)) if full_v6 => count += 1,
            Err(_) if full_v4 || full_v6 => count += 1,
            _ => {}
        }
    }
    count
}

fn dns_guard_label(full_tunnel: bool, dns_empty: bool) -> &'static str {
    if dns_empty {
        "No Tunnel DNS"
    } else if cfg!(target_os = "windows") && full_tunnel {
        "NRPT + Guard"
    } else if full_tunnel {
        "Tunnel DNS"
    } else {
        "Config DNS"
    }
}

fn platform_label() -> &'static str {
    if cfg!(target_os = "linux") {
        "Linux"
    } else if cfg!(target_os = "windows") {
        "Windows"
    } else {
        "Platform"
    }
}

fn interface_label(parsed: &r_wg::backend::wg::config::WireGuardConfig) -> String {
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

fn dns_label(parsed: &r_wg::backend::wg::config::WireGuardConfig) -> String {
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

fn status_from_events(net_events: &[String], patterns: &[String]) -> RouteMapItemStatus {
    let matched = net_events
        .iter()
        .any(|line| patterns.iter().any(|pattern| line.contains(pattern)));
    if !matched {
        return RouteMapItemStatus::Planned;
    }
    if net_events.iter().any(|line| {
        patterns.iter().any(|pattern| line.contains(pattern))
            && (line.contains("failed") || line.contains("abort"))
    }) {
        RouteMapItemStatus::Failed
    } else {
        RouteMapItemStatus::Applied
    }
}

fn matching_events(net_events: &[String], patterns: &[String]) -> Vec<SharedString> {
    let matches = net_events
        .iter()
        .filter(|line| patterns.iter().any(|pattern| line.contains(pattern)))
        .take(5)
        .cloned()
        .map(Into::into)
        .collect::<Vec<_>>();
    if matches.is_empty() {
        vec!["No matching net event captured yet.".into()]
    } else {
        matches
    }
}

fn route_text(addr: IpAddr, cidr: u8) -> String {
    format!("{addr}/{cidr}")
}

fn family_from_ip(ip: IpAddr) -> RouteFamilyFilter {
    match ip {
        IpAddr::V4(_) => RouteFamilyFilter::Ipv4,
        IpAddr::V6(_) => RouteFamilyFilter::Ipv6,
    }
}

fn is_full_tunnel_route(route: &AllowedIp) -> bool {
    route.addr.is_unspecified() && route.cidr == 0
}

fn route_is_default_family(route: &AllowedIp, family: RouteFamilyFilter) -> bool {
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

fn graph_step(icon: IconName, label: &str, value: &str, note: Option<&str>) -> RouteMapGraphStep {
    RouteMapGraphStep {
        icon,
        label: label.to_string().into(),
        value: value.to_string().into(),
        note: note.map(|note| note.to_string().into()),
    }
}

fn chip(label: impl Into<String>, tone: RouteMapTone) -> RouteMapChip {
    RouteMapChip {
        label: label.into().into(),
        tone,
    }
}

fn item_id(prefix: &str, suffix: &str) -> SharedString {
    format!("{prefix}:{}", suffix.to_ascii_lowercase()).into()
}
