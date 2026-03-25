use std::collections::{hash_map::DefaultHasher, HashMap};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use gpui::SharedString;
use r_wg::backend::wg::config::RouteTable;
use r_wg::backend::wg::route_plan::{
    normalize_config_for_runtime, RouteApplyAttemptState as BackendRouteApplyAttemptState,
    RouteApplyFailureKind as BackendRouteApplyFailureKind, RouteApplyKind as BackendRouteApplyKind,
    RouteApplyPhase as BackendRouteApplyPhase, RouteApplyReport as BackendRouteApplyReport,
    RouteApplyReportSource as BackendRouteApplyReportSource,
    RouteApplyStatus as BackendRouteApplyStatus,
};
use r_wg::backend::wg::{OperationalRoutePlan, RoutePlanPlatform};
use r_wg::dns::DnsSelection;
use r_wg::log;

use crate::ui::state::{current_apply_report, RouteFamilyFilter, WgApp};
use crate::ui::view::shared::ViewData;

use super::presenter::{build_plan_explain, build_plan_presentation, RouteMapMatchTarget};

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

#[derive(Clone)]
pub(crate) struct RouteMapEvidence {
    pub(crate) cache_key: u64,
    pub(crate) net_events_raw: Vec<String>,
    pub(crate) net_events: Vec<SharedString>,
    pub(crate) apply_report: Option<BackendRouteApplyReport>,
    apply_report_index: Arc<HashMap<String, r_wg::backend::wg::route_plan::RouteApplyEntry>>,
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
        let source_label = current_source_label(app, data);
        let platform_label: SharedString = platform_label().into();
        plan_cache_key(app, data, &source_label, &platform_label)
    }

    pub(crate) fn build_plan(app: &WgApp, data: &ViewData) -> EffectiveRoutePlan {
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
        let presented = build_plan_presentation(&route_plan, &normalized);

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

    pub(crate) fn build_evidence() -> RouteMapEvidence {
        let net_events_raw = recent_net_events();
        let apply_report = current_apply_report();
        let cache_key = evidence_cache_key(&net_events_raw, apply_report.as_ref());

        RouteMapEvidence {
            cache_key,
            net_events: net_events_raw.iter().cloned().map(Into::into).collect(),
            net_events_raw,
            apply_report_index: Arc::new(
                apply_report
                    .as_ref()
                    .map(|report| {
                        report
                            .entries
                            .iter()
                            .cloned()
                            .map(|entry| (entry.item_id.clone(), entry))
                            .collect()
                    })
                    .unwrap_or_default(),
            ),
            apply_report,
        }
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
        apply_event_overlay(
            &mut groups,
            &evidence.net_events_raw,
            evidence.apply_report.as_ref(),
            evidence.apply_report_index.as_ref(),
        );
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

fn chip(label: impl Into<String>, tone: RouteMapTone) -> RouteMapChip {
    RouteMapChip {
        label: label.into().into(),
        tone,
    }
}

fn apply_event_overlay(
    groups: &mut [RouteMapInventoryGroup],
    net_events: &[String],
    apply_report: Option<&BackendRouteApplyReport>,
    apply_report_index: &HashMap<String, r_wg::backend::wg::route_plan::RouteApplyEntry>,
) {
    for group in groups {
        for item in &mut group.items {
            let report_entry = apply_report.and_then(|_| apply_report_index.get(item.id.as_ref()));
            let status = report_entry
                .map(|entry| map_apply_report_status(entry.status))
                .or(item.static_status)
                .unwrap_or_else(|| status_from_events(net_events, &item.event_patterns));
            item.status = status;
            if let Some(entry) = report_entry {
                item.inspector.runtime_evidence = build_report_runtime_evidence(
                    apply_report,
                    entry,
                    net_events,
                    &item.event_patterns,
                );
            } else if !item.event_patterns.is_empty() {
                item.inspector.runtime_evidence = matching_events(net_events, &item.event_patterns);
            }
            if let Some(route_row) = item.route_row.as_mut() {
                route_row.status = status.label().into();
            }
        }
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
        group.summary = group.items.len().to_string().into();
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

fn evidence_cache_key(
    net_events: &[String],
    apply_report: Option<&BackendRouteApplyReport>,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    for event in net_events {
        event.hash(&mut hasher);
    }
    if let Some(report) = apply_report {
        match report.platform {
            Some(RoutePlanPlatform::Linux) => 0u8.hash(&mut hasher),
            Some(RoutePlanPlatform::Windows) => 1u8.hash(&mut hasher),
            Some(RoutePlanPlatform::Other) => 2u8.hash(&mut hasher),
            None => 3u8.hash(&mut hasher),
        }
        match report.source {
            BackendRouteApplyReportSource::Live => 0u8.hash(&mut hasher),
            BackendRouteApplyReportSource::Persisted => 1u8.hash(&mut hasher),
        }
        match report.attempt_state {
            BackendRouteApplyAttemptState::Applying => 0u8.hash(&mut hasher),
            BackendRouteApplyAttemptState::Running => 1u8.hash(&mut hasher),
            BackendRouteApplyAttemptState::Failed => 2u8.hash(&mut hasher),
        }
        for entry in &report.entries {
            entry.item_id.hash(&mut hasher);
            match entry.status {
                BackendRouteApplyStatus::Applied => 0u8.hash(&mut hasher),
                BackendRouteApplyStatus::Skipped => 1u8.hash(&mut hasher),
                BackendRouteApplyStatus::Failed => 2u8.hash(&mut hasher),
            }
            match entry.phase {
                BackendRouteApplyPhase::Apply => 0u8.hash(&mut hasher),
                BackendRouteApplyPhase::Cleanup => 1u8.hash(&mut hasher),
                BackendRouteApplyPhase::Recovery => 2u8.hash(&mut hasher),
            }
            match entry.kind {
                BackendRouteApplyKind::Other => 0u8.hash(&mut hasher),
                BackendRouteApplyKind::Adapter => 1u8.hash(&mut hasher),
                BackendRouteApplyKind::RecoveryJournal => 2u8.hash(&mut hasher),
                BackendRouteApplyKind::Address => 3u8.hash(&mut hasher),
                BackendRouteApplyKind::PolicyRule => 4u8.hash(&mut hasher),
                BackendRouteApplyKind::Route => 5u8.hash(&mut hasher),
                BackendRouteApplyKind::Metric => 6u8.hash(&mut hasher),
                BackendRouteApplyKind::BypassRoute => 7u8.hash(&mut hasher),
                BackendRouteApplyKind::Dns => 8u8.hash(&mut hasher),
                BackendRouteApplyKind::Nrpt => 9u8.hash(&mut hasher),
                BackendRouteApplyKind::DnsGuard => 10u8.hash(&mut hasher),
            }
            match entry.failure_kind {
                Some(BackendRouteApplyFailureKind::Precondition) => 0u8.hash(&mut hasher),
                Some(BackendRouteApplyFailureKind::Lookup) => 1u8.hash(&mut hasher),
                Some(BackendRouteApplyFailureKind::Persistence) => 2u8.hash(&mut hasher),
                Some(BackendRouteApplyFailureKind::Verification) => 3u8.hash(&mut hasher),
                Some(BackendRouteApplyFailureKind::System) => 4u8.hash(&mut hasher),
                Some(BackendRouteApplyFailureKind::Cleanup) => 5u8.hash(&mut hasher),
                None => 6u8.hash(&mut hasher),
            }
            for value in &entry.evidence {
                value.hash(&mut hasher);
            }
        }
    }
    hasher.finish()
}

fn map_apply_report_status(status: BackendRouteApplyStatus) -> RouteMapItemStatus {
    match status {
        BackendRouteApplyStatus::Applied => RouteMapItemStatus::Applied,
        BackendRouteApplyStatus::Skipped => RouteMapItemStatus::Skipped,
        BackendRouteApplyStatus::Failed => RouteMapItemStatus::Failed,
    }
}

fn build_report_runtime_evidence(
    apply_report: Option<&BackendRouteApplyReport>,
    entry: &r_wg::backend::wg::route_plan::RouteApplyEntry,
    net_events: &[String],
    patterns: &[String],
) -> Vec<SharedString> {
    let mut evidence = Vec::new();
    if let Some(report) = apply_report {
        if let Some(summary) = format_report_entry_summary(report, entry) {
            evidence.push(summary.into());
        }
    }
    if entry.evidence.is_empty() {
        evidence.extend(matching_events(net_events, patterns));
    } else {
        evidence.extend(entry.evidence.iter().cloned().map(Into::into));
    }
    evidence
}

fn format_report_entry_summary(
    report: &BackendRouteApplyReport,
    entry: &r_wg::backend::wg::route_plan::RouteApplyEntry,
) -> Option<String> {
    let source = match report.source {
        BackendRouteApplyReportSource::Live => "Live backend report",
        BackendRouteApplyReportSource::Persisted => "Persisted backend report",
    };
    let phase = match entry.phase {
        BackendRouteApplyPhase::Apply => "apply",
        BackendRouteApplyPhase::Cleanup => "cleanup",
        BackendRouteApplyPhase::Recovery => "recovery",
    };
    let state = match report.attempt_state {
        BackendRouteApplyAttemptState::Applying => "applying",
        BackendRouteApplyAttemptState::Running => "running",
        BackendRouteApplyAttemptState::Failed => "failed",
    };
    let failure = entry
        .failure_kind
        .map(|kind| match kind {
            BackendRouteApplyFailureKind::Precondition => "precondition",
            BackendRouteApplyFailureKind::Lookup => "lookup",
            BackendRouteApplyFailureKind::Persistence => "persistence",
            BackendRouteApplyFailureKind::Verification => "verification",
            BackendRouteApplyFailureKind::System => "system",
            BackendRouteApplyFailureKind::Cleanup => "cleanup",
        })
        .map(|kind| format!(" failure type: {kind}."))
        .unwrap_or_default();

    if matches!(report.source, BackendRouteApplyReportSource::Live)
        && matches!(report.attempt_state, BackendRouteApplyAttemptState::Running)
        && matches!(entry.status, BackendRouteApplyStatus::Applied)
        && matches!(entry.kind, BackendRouteApplyKind::Other)
    {
        return None;
    }

    let verb = match entry.status {
        BackendRouteApplyStatus::Applied => "applied",
        BackendRouteApplyStatus::Skipped => "skipped",
        BackendRouteApplyStatus::Failed => "failed",
    };

    Some(format!(
        "{source}: {} {verb} during {phase}; attempt state {state}.{failure}",
        entry.kind.label()
    ))
}

fn recent_net_events() -> Vec<String> {
    log::snapshot()
        .into_iter()
        .filter(|line| line.contains("[net]"))
        .rev()
        .take(40)
        .collect()
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
