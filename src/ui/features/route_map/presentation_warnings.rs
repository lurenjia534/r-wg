use gpui::SharedString;
use r_wg::core::config::{RouteTable, WireGuardConfig};
use r_wg::core::route_plan::{OperationalRoutePlan, RoutePlanPlatform};

use super::data::{
    RouteMapGraphStepKind, RouteMapInspector, RouteMapInventoryItem, RouteMapItemStatus,
    RouteMapTone,
};
use super::presentation_helpers::{chip, graph_step};

pub(super) fn build_warning_items(
    route_plan: &OperationalRoutePlan,
    parsed: &WireGuardConfig,
    kill_switch_enabled: bool,
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
                "AllowedIPs remain useful for explanation, but they will not become kernel routes."
                    .into(),
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

    if route_plan.full_tunnel.any() && !kill_switch_enabled {
        items.push(warning_item(
            "warning-kill-switch-disabled",
            "Kill switch is disabled",
            "Full-tunnel routing stays active, but off-tunnel leak blocking is intentionally turned off.",
            vec![
                "If the tunnel drops, traffic may escape through the normal network path until the session is fully torn down.".into(),
                "Use this only when you explicitly want looser behavior for testing or debugging.".into(),
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
