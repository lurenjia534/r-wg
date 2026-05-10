use gpui::SharedString;
use r_wg::core::config::{RouteTable, WireGuardConfig};
use r_wg::core::route_plan::{OperationalRoutePlan, RoutePlanPlatform};

use super::data::{RouteMapChip, RouteMapInventoryGroup, RouteMapTone};
use super::presentation_helpers::{chip, dns_guard_label, format_route_table};
use super::presentation_inventory::build_inventory_groups;

pub(super) struct RouteMapPresentedPlan {
    pub(super) plan_status: SharedString,
    pub(super) summary_chips: Vec<RouteMapChip>,
    pub(super) inventory_groups: Vec<RouteMapInventoryGroup>,
}

pub(super) fn build_plan_presentation(
    route_plan: &OperationalRoutePlan,
    parsed: &WireGuardConfig,
    kill_switch_enabled: bool,
) -> RouteMapPresentedPlan {
    let full_tunnel_active = route_plan.full_tunnel.any();
    let table_off = parsed.interface.table == Some(RouteTable::Off);
    let dns_guard_text = dns_guard_label(
        route_plan.platform,
        full_tunnel_active,
        parsed.interface.dns_servers.is_empty(),
    );
    let inventory_groups = build_inventory_groups(route_plan, parsed, kill_switch_enabled);
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
            if kill_switch_enabled {
                "Kill Switch On"
            } else {
                "Kill Switch Off"
            },
            if full_tunnel_active && !kill_switch_enabled {
                RouteMapTone::Warning
            } else if kill_switch_enabled {
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
        chip(format!("Table {route_table_text}"), RouteMapTone::Secondary),
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
