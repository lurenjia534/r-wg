use serde::{Deserialize, Serialize};

use super::RoutePlanPlatform;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteApplyStatus {
    Applied,
    Skipped,
    Failed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteApplyReportSource {
    #[default]
    Live,
    Persisted,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteApplyAttemptState {
    #[default]
    Applying,
    Running,
    Failed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteApplyPhase {
    #[default]
    Apply,
    Cleanup,
    Recovery,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteApplyKind {
    #[default]
    Other,
    Adapter,
    RecoveryJournal,
    Address,
    PolicyRule,
    Route,
    Metric,
    BypassRoute,
    Dns,
    Nrpt,
    DnsGuard,
}

impl RouteApplyKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Other => "operation",
            Self::Adapter => "adapter",
            Self::RecoveryJournal => "recovery journal",
            Self::Address => "address",
            Self::PolicyRule => "policy rule",
            Self::Route => "route",
            Self::Metric => "metric",
            Self::BypassRoute => "bypass route",
            Self::Dns => "DNS",
            Self::Nrpt => "NRPT",
            Self::DnsGuard => "DNS guard",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteApplyFailureKind {
    Precondition,
    Lookup,
    Persistence,
    Verification,
    System,
    Cleanup,
}

impl RouteApplyFailureKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Precondition => "precondition",
            Self::Lookup => "lookup",
            Self::Persistence => "persistence",
            Self::Verification => "verification",
            Self::System => "system",
            Self::Cleanup => "cleanup",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteApplyEntry {
    pub item_id: String,
    pub status: RouteApplyStatus,
    #[serde(default)]
    pub phase: RouteApplyPhase,
    #[serde(default)]
    pub kind: RouteApplyKind,
    #[serde(default)]
    pub failure_kind: Option<RouteApplyFailureKind>,
    pub evidence: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteApplyReport {
    pub platform: Option<RoutePlanPlatform>,
    #[serde(default)]
    pub source: RouteApplyReportSource,
    #[serde(default)]
    pub attempt_state: RouteApplyAttemptState,
    pub entries: Vec<RouteApplyEntry>,
}

impl RouteApplyReport {
    pub fn new(platform: RoutePlanPlatform) -> Self {
        Self {
            platform: Some(platform),
            source: RouteApplyReportSource::Live,
            attempt_state: RouteApplyAttemptState::Applying,
            entries: Vec::new(),
        }
    }

    pub fn push_applied(&mut self, item_id: impl Into<String>, evidence: Vec<String>) {
        self.push_applied_kind(item_id, RouteApplyKind::Other, evidence);
    }

    pub fn push_applied_kind(
        &mut self,
        item_id: impl Into<String>,
        kind: RouteApplyKind,
        evidence: Vec<String>,
    ) {
        let item_id = item_id.into();
        let inferred_phase = infer_apply_phase(&item_id);
        let inferred_kind = infer_apply_kind(&item_id).unwrap_or(kind);
        self.entries.push(RouteApplyEntry {
            phase: inferred_phase,
            item_id,
            status: RouteApplyStatus::Applied,
            kind: inferred_kind,
            failure_kind: None,
            evidence,
        });
    }

    pub fn push_skipped(&mut self, item_id: impl Into<String>, evidence: Vec<String>) {
        self.push_skipped_kind(item_id, RouteApplyKind::Other, evidence);
    }

    pub fn push_skipped_kind(
        &mut self,
        item_id: impl Into<String>,
        kind: RouteApplyKind,
        evidence: Vec<String>,
    ) {
        let item_id = item_id.into();
        let inferred_phase = infer_apply_phase(&item_id);
        let inferred_kind = infer_apply_kind(&item_id).unwrap_or(kind);
        self.entries.push(RouteApplyEntry {
            phase: inferred_phase,
            item_id,
            status: RouteApplyStatus::Skipped,
            kind: inferred_kind,
            failure_kind: None,
            evidence,
        });
    }

    pub fn push_failed(&mut self, item_id: impl Into<String>, evidence: Vec<String>) {
        self.push_failed_kind(item_id, RouteApplyKind::Other, None, evidence);
    }

    pub fn push_failed_kind(
        &mut self,
        item_id: impl Into<String>,
        kind: RouteApplyKind,
        failure_kind: Option<RouteApplyFailureKind>,
        evidence: Vec<String>,
    ) {
        let item_id = item_id.into();
        let inferred_phase = infer_apply_phase(&item_id);
        let inferred_kind = infer_apply_kind(&item_id).unwrap_or(kind);
        let inferred_failure_kind = failure_kind.or_else(|| infer_failure_kind(&item_id));
        self.entries.push(RouteApplyEntry {
            phase: inferred_phase,
            item_id,
            status: RouteApplyStatus::Failed,
            kind: inferred_kind,
            failure_kind: inferred_failure_kind,
            evidence,
        });
    }

    pub fn mark_running(&mut self) {
        self.attempt_state = RouteApplyAttemptState::Running;
    }

    pub fn mark_failed(&mut self) {
        self.attempt_state = RouteApplyAttemptState::Failed;
    }

    pub fn mark_persisted(&mut self) {
        self.source = RouteApplyReportSource::Persisted;
    }
}

fn infer_apply_phase(item_id: &str) -> RouteApplyPhase {
    if item_id == "apply:recovery" || item_id == "apply:recovery_init" {
        RouteApplyPhase::Recovery
    } else if item_id.starts_with("cleanup:") {
        RouteApplyPhase::Cleanup
    } else {
        RouteApplyPhase::Apply
    }
}

fn infer_apply_kind(item_id: &str) -> Option<RouteApplyKind> {
    if item_id == "policy-v4" || item_id == "policy-v6" || item_id.starts_with("policy:") {
        Some(RouteApplyKind::PolicyRule)
    } else if item_id == "metric-v4" || item_id == "metric-v6" || item_id.starts_with("metric:") {
        Some(RouteApplyKind::Metric)
    } else if item_id.starts_with("bypass:") || item_id.starts_with("bypass-pending:") {
        Some(RouteApplyKind::BypassRoute)
    } else if item_id.starts_with("allowed:")
        || item_id.starts_with("dns-route:")
        || item_id.starts_with("route:")
    {
        Some(RouteApplyKind::Route)
    } else if item_id == "apply:adapter_lookup" || item_id == "apply:linux:netlink" {
        Some(RouteApplyKind::Adapter)
    } else if item_id.starts_with("apply:address:") || item_id == "apply:addresses" {
        Some(RouteApplyKind::Address)
    } else if item_id == "apply:dns" {
        Some(RouteApplyKind::Dns)
    } else if item_id == "apply:nrpt" {
        Some(RouteApplyKind::Nrpt)
    } else if item_id == "apply:dns_guard" {
        Some(RouteApplyKind::DnsGuard)
    } else if item_id == "apply:recovery" || item_id == "apply:recovery_init" {
        Some(RouteApplyKind::RecoveryJournal)
    } else {
        None
    }
}

fn infer_failure_kind(item_id: &str) -> Option<RouteApplyFailureKind> {
    if item_id == "apply:adapter_lookup" {
        Some(RouteApplyFailureKind::Lookup)
    } else if item_id == "apply:recovery" || item_id == "apply:recovery_init" {
        Some(RouteApplyFailureKind::Persistence)
    } else if item_id == "apply:stale_address_cleanup" {
        Some(RouteApplyFailureKind::Cleanup)
    } else if item_id.starts_with("apply:")
        || item_id.starts_with("allowed:")
        || item_id == "policy-v4"
        || item_id == "policy-v6"
        || item_id == "metric-v4"
        || item_id == "metric-v6"
        || item_id.starts_with("bypass:")
        || item_id.starts_with("bypass-pending:")
        || item_id.starts_with("dns-route:")
    {
        Some(RouteApplyFailureKind::System)
    } else {
        None
    }
}
