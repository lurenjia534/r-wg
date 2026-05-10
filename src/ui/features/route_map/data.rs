use std::net::IpAddr;
use std::sync::Arc;

use gpui::SharedString;

use crate::ui::state::{RouteFamilyFilter, WgApp};
use crate::ui::view::shared::ViewData;

pub(crate) use super::evidence_model::RouteMapEvidence;
use super::evidence_model::{build_route_map_evidence, overlay_evidence};
use super::explain_model::build_plan_explain;
pub(crate) use super::plan_model::EffectiveRoutePlan;
use super::plan_model::{build_effective_route_plan, effective_route_plan_key};
use super::selection_model::{apply_filters, resolve_selected_item};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
    Warning,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RouteMapGraphStepKind {
    Interface,
    Dns,
    Policy,
    Peer,
    Endpoint,
    Guardrail,
    Destination,
}

#[derive(Clone)]
pub(crate) struct RouteMapChip {
    pub(crate) label: SharedString,
    pub(crate) tone: RouteMapTone,
}

#[derive(Clone)]
pub(crate) struct RouteMapGraphStep {
    pub(crate) kind: RouteMapGraphStepKind,
    pub(crate) icon: gpui_component::IconName,
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
    pub(crate) static_status: Option<RouteMapItemStatus>,
    pub(crate) status: RouteMapItemStatus,
    pub(crate) event_patterns: Vec<String>,
    pub(crate) chips: Vec<RouteMapChip>,
    pub(crate) inspector: RouteMapInspector,
    pub(crate) graph_steps: Vec<RouteMapGraphStep>,
    pub(crate) route_row: Option<RouteMapRouteRow>,
    pub(crate) endpoint_host: Option<SharedString>,
    pub(crate) match_target: Option<RouteMapMatchTarget>,
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
    pub(crate) route_rows: Arc<[RouteMapRouteRow]>,
    pub(crate) net_events: Vec<SharedString>,
    pub(crate) explain: Option<RouteMapExplainResult>,
    pub(crate) explain_match_id: Option<SharedString>,
    pub(crate) selected_item_id: Option<SharedString>,
    pub(crate) selected_item: Option<RouteMapInventoryItem>,
}

impl RouteMapData {
    pub(crate) fn plan_key(app: &WgApp, data: &ViewData) -> u64 {
        effective_route_plan_key(app, data)
    }

    pub(crate) fn build_plan(app: &WgApp, data: &ViewData) -> EffectiveRoutePlan {
        build_effective_route_plan(app, data)
    }

    pub(crate) fn build_evidence(app: &WgApp) -> RouteMapEvidence {
        build_route_map_evidence(app)
    }

    pub(crate) fn from_cached(
        app: &WgApp,
        search_query: &str,
        plan: &EffectiveRoutePlan,
        evidence: &RouteMapEvidence,
    ) -> Self {
        let search_query = search_query.trim().to_string();

        if !plan.has_plan {
            return Self {
                has_plan: false,
                plan_status: plan.plan_status.clone(),
                source_label: plan.source_label.clone(),
                platform_label: plan.platform_label.clone(),
                summary_chips: plan.summary_chips.clone(),
                parse_error: plan.parse_error.clone(),
                inventory_groups: Vec::new(),
                route_rows: Arc::<[RouteMapRouteRow]>::from(Vec::new()),
                net_events: evidence.net_events.clone(),
                explain: None,
                explain_match_id: None,
                selected_item_id: None,
                selected_item: None,
            };
        }

        let mut groups = plan.inventory_groups.clone();
        overlay_evidence(&mut groups, evidence);
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

        let explain = Some(build_plan_explain(&plan.inventory_groups, &search_query));
        let explain_match_id = explain
            .as_ref()
            .and_then(|value| value.matched_item_id.as_ref())
            .cloned();
        let selected_item = resolve_selected_item(
            &groups,
            app.ui_session.route_map_selected_item.as_ref(),
            explain_match_id.as_ref(),
        );
        let selected_item_id = selected_item.as_ref().map(|item| item.id.clone());
        let mut summary_chips = plan.summary_chips.clone();
        summary_chips.push(chip(
            format!("Evidence {}", evidence.net_events_raw.len()),
            if evidence.net_events_raw.is_empty() {
                RouteMapTone::Secondary
            } else {
                RouteMapTone::Info
            },
        ));
        summary_chips.push(chip(
            format!("Routes {}", route_rows.len()),
            RouteMapTone::Secondary,
        ));

        Self {
            has_plan: true,
            plan_status: plan.plan_status.clone(),
            source_label: plan.source_label.clone(),
            platform_label: plan.platform_label.clone(),
            summary_chips,
            parse_error: plan.parse_error.clone(),
            inventory_groups: groups,
            route_rows: route_rows.into(),
            net_events: evidence.net_events.clone(),
            explain,
            explain_match_id,
            selected_item_id,
            selected_item,
        }
    }
}

pub(crate) fn chip(label: impl Into<String>, tone: RouteMapTone) -> RouteMapChip {
    RouteMapChip {
        label: label.into().into(),
        tone,
    }
}
