use std::collections::HashSet;

use r_wg::core::config::{RouteTable, WireGuardConfig};
use r_wg::core::route_plan::OperationalRoutePlan;

use super::data::{
    RouteMapInspector, RouteMapInventoryItem, RouteMapItemStatus, RouteMapMatchTarget,
    RouteMapRouteRow, RouteMapTone,
};
use super::presentation_helpers::{
    allowed_graph_steps, allowed_item_id, allowed_platform_details, allowed_risks, chip,
    family_from_ip, family_label, is_full_tunnel_route, route_text,
};

pub(super) fn build_allowed_route_items(
    route_plan: &OperationalRoutePlan,
    parsed: &WireGuardConfig,
    interface_label: &str,
    dns_label: &str,
    route_table_text: &str,
) -> (Vec<RouteMapInventoryItem>, Vec<RouteMapInventoryItem>) {
    let table_off = parsed.interface.table == Some(RouteTable::Off);
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
                status: if table_off {
                    RouteMapItemStatus::Skipped
                } else {
                    RouteMapItemStatus::Planned
                },
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
                    interface_label,
                    route_table_text,
                    &peer_label,
                    &endpoint_text,
                    &destination_text,
                    dns_label,
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
                endpoint_host: peer
                    .endpoint
                    .as_ref()
                    .map(|endpoint| endpoint.host.clone().into()),
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

    (full_tunnel_items, split_route_items)
}
