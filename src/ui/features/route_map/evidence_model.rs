use std::collections::{hash_map::DefaultHasher, HashMap};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use gpui::SharedString;
use r_wg::core::route_plan::{
    RouteApplyAttemptState as BackendRouteApplyAttemptState, RouteApplyEntry,
    RouteApplyFailureKind as BackendRouteApplyFailureKind, RouteApplyKind as BackendRouteApplyKind,
    RouteApplyPhase as BackendRouteApplyPhase, RouteApplyReport as BackendRouteApplyReport,
    RouteApplyReportSource as BackendRouteApplyReportSource,
    RouteApplyStatus as BackendRouteApplyStatus, RoutePlanPlatform,
};
use r_wg::log;

use crate::ui::state::WgApp;

use super::data::{RouteMapInventoryGroup, RouteMapItemStatus};

#[derive(Clone)]
pub(crate) struct RouteMapEvidence {
    pub(crate) cache_key: u64,
    pub(crate) net_events_raw: Vec<String>,
    pub(crate) net_events: Vec<SharedString>,
    apply_report: Option<BackendRouteApplyReport>,
    apply_report_index: Arc<HashMap<String, RouteApplyEntry>>,
}

pub(crate) fn build_route_map_evidence(app: &WgApp) -> RouteMapEvidence {
    let net_events_raw = recent_net_events();
    let apply_report = app.runtime.last_apply_report.clone();
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

pub(crate) fn overlay_evidence(groups: &mut [RouteMapInventoryGroup], evidence: &RouteMapEvidence) {
    for group in groups {
        for item in &mut group.items {
            let report_entry = evidence
                .apply_report
                .as_ref()
                .and_then(|_| evidence.apply_report_index.get(item.id.as_ref()));
            let status = report_entry
                .map(|entry| map_apply_report_status(entry.status))
                .or(item.static_status)
                .unwrap_or_else(|| {
                    status_from_events(&evidence.net_events_raw, &item.event_patterns)
                });
            item.status = status;
            if let Some(entry) = report_entry {
                item.inspector.runtime_evidence = build_report_runtime_evidence(
                    evidence.apply_report.as_ref(),
                    entry,
                    &evidence.net_events_raw,
                    &item.event_patterns,
                );
            } else if !item.event_patterns.is_empty() {
                item.inspector.runtime_evidence =
                    matching_events(&evidence.net_events_raw, &item.event_patterns);
            }
            if let Some(route_row) = item.route_row.as_mut() {
                route_row.status = status.label().into();
            }
        }
    }
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
                BackendRouteApplyKind::KillSwitch => 11u8.hash(&mut hasher),
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
    entry: &RouteApplyEntry,
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
    entry: &RouteApplyEntry,
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
