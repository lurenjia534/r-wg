use r_wg::core::config::WireGuardConfig;
use r_wg::core::route_plan::{OperationalRoutePlan, RoutePlanFamily, RoutePlanPlatform};

use super::data::{
    RouteMapGraphStepKind, RouteMapInspector, RouteMapInventoryItem, RouteMapItemStatus,
    RouteMapTone,
};
use super::presentation_helpers::{
    chip, family_label, filter_from_backend_family, format_route_table, graph_step, interface_label,
};

pub(super) fn build_policy_items(
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
                status: if table_off {
                    RouteMapItemStatus::Skipped
                } else {
                    RouteMapItemStatus::Planned
                },
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
                status: if table_off {
                    RouteMapItemStatus::Skipped
                } else {
                    RouteMapItemStatus::Planned
                },
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
