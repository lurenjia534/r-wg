use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use gpui::SharedString;
use r_wg::core::config::RouteTable;
use r_wg::core::route_plan::{
    normalize_config_for_runtime, OperationalRoutePlan, RoutePlanPlatform,
};
use r_wg::dns::DnsSelection;

use crate::ui::state::WgApp;
use crate::ui::view::shared::ViewData;

use super::data::{chip, RouteMapChip, RouteMapInventoryGroup, RouteMapTone};
use super::presenter::build_plan_presentation;

#[derive(Clone)]
pub(crate) struct EffectiveRoutePlan {
    pub(crate) cache_key: u64,
    pub(crate) has_plan: bool,
    pub(crate) plan_status: SharedString,
    pub(crate) source_label: SharedString,
    pub(crate) platform_label: SharedString,
    pub(crate) summary_chips: Vec<RouteMapChip>,
    pub(crate) parse_error: Option<SharedString>,
    pub(crate) inventory_groups: Vec<RouteMapInventoryGroup>,
}

pub(crate) fn effective_route_plan_key(app: &WgApp, data: &ViewData) -> u64 {
    let source_label = current_source_label(app, data);
    let platform_label: SharedString = platform_label().into();
    plan_cache_key(app, data, &source_label, &platform_label)
}

pub(crate) fn build_effective_route_plan(app: &WgApp, data: &ViewData) -> EffectiveRoutePlan {
    let source_label = current_source_label(app, data);
    let platform_label: SharedString = platform_label().into();
    let cache_key = plan_cache_key(app, data, &source_label, &platform_label);

    let Some(parsed) = data.parsed_config.as_ref() else {
        return EffectiveRoutePlan {
            cache_key,
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
        };
    };

    let normalized = normalize_config_for_runtime(
        parsed.clone(),
        DnsSelection::new(app.ui_prefs.dns_mode, app.ui_prefs.dns_preset),
    );
    let route_plan = OperationalRoutePlan::build(RoutePlanPlatform::current(), &normalized);
    let presented =
        build_plan_presentation(&route_plan, &normalized, app.ui_prefs.kill_switch_enabled);

    EffectiveRoutePlan {
        cache_key,
        has_plan: true,
        plan_status: presented.plan_status,
        source_label,
        platform_label,
        summary_chips: presented.summary_chips,
        parse_error: data.parse_error.clone().map(Into::into),
        inventory_groups: presented.inventory_groups,
    }
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

fn plan_cache_key(
    app: &WgApp,
    data: &ViewData,
    source_label: &SharedString,
    platform_label: &SharedString,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    source_label.hash(&mut hasher);
    platform_label.hash(&mut hasher);
    data.parse_error.hash(&mut hasher);
    data.has_saved_source.hash(&mut hasher);
    data.draft_dirty.hash(&mut hasher);
    app.selection.selected_id.hash(&mut hasher);
    std::mem::discriminant(&app.ui_prefs.dns_mode).hash(&mut hasher);
    std::mem::discriminant(&app.ui_prefs.dns_preset).hash(&mut hasher);
    app.ui_prefs.kill_switch_enabled.hash(&mut hasher);

    if let Some(parsed) = data.parsed_config.as_ref() {
        match parsed.interface.table {
            Some(RouteTable::Auto) => 0u8.hash(&mut hasher),
            Some(RouteTable::Off) => 1u8.hash(&mut hasher),
            Some(RouteTable::Id(id)) => {
                2u8.hash(&mut hasher);
                id.hash(&mut hasher);
            }
            None => 3u8.hash(&mut hasher),
        }
        parsed.interface.fwmark.hash(&mut hasher);
        for address in &parsed.interface.addresses {
            address.addr.hash(&mut hasher);
            address.cidr.hash(&mut hasher);
        }
        for dns in &parsed.interface.dns_servers {
            dns.hash(&mut hasher);
        }
        for peer in &parsed.peers {
            peer.endpoint
                .as_ref()
                .map(|endpoint| (&endpoint.host, endpoint.port))
                .hash(&mut hasher);
            for allowed in &peer.allowed_ips {
                allowed.addr.hash(&mut hasher);
                allowed.cidr.hash(&mut hasher);
            }
        }
    }

    hasher.finish()
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
