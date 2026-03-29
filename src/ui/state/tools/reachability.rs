use gpui::{Entity, SharedString, Subscription};
use gpui_component::input::InputState;
use r_wg::backend::wg::tools::{
    AddressFamilyPreference, ReachabilityMode, ReachabilityResult, ReachabilityVerdict,
};

use super::AsyncJobState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReachabilityTab {
    Single,
    Audit,
}

impl ReachabilityTab {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Single => "Single Test",
            Self::Audit => "Saved Config Audit",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReachabilityAuditFilter {
    All,
    Failures,
    Issues,
}

impl ReachabilityAuditFilter {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Failures => "Failures",
            Self::Issues => "Issues",
        }
    }

    pub(crate) fn matches(self, status: ReachabilityBatchStatus) -> bool {
        match self {
            Self::All => true,
            Self::Failures => matches!(
                status,
                ReachabilityBatchStatus::Failed | ReachabilityBatchStatus::PartiallyReachable
            ),
            Self::Issues => matches!(
                status,
                ReachabilityBatchStatus::ParseError
                    | ReachabilityBatchStatus::ReadError
                    | ReachabilityBatchStatus::NoEndpoint
                    | ReachabilityBatchStatus::Cancelled
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReachabilityAuditPhase {
    LoadingConfigs,
    ProbingEndpoints,
    Finalizing,
    Completed,
}

impl ReachabilityAuditPhase {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::LoadingConfigs => "Loading configs",
            Self::ProbingEndpoints => "Checking endpoints",
            Self::Finalizing => "Finalizing",
            Self::Completed => "Completed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ReachabilityFormState {
    pub(crate) mode: ReachabilityMode,
    pub(crate) family_preference: AddressFamilyPreference,
    pub(crate) stop_on_first_success: bool,
}

impl Default for ReachabilityFormState {
    fn default() -> Self {
        Self {
            mode: ReachabilityMode::ResolveOnly,
            family_preference: AddressFamilyPreference::PreferIpv4,
            stop_on_first_success: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReachabilityBatchStatus {
    Resolved,
    Reachable,
    PartiallyReachable,
    Failed,
    ParseError,
    ReadError,
    NoEndpoint,
    Cancelled,
}

impl ReachabilityBatchStatus {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Resolved => "Resolved",
            Self::Reachable => "Reachable",
            Self::PartiallyReachable => "Partial",
            Self::Failed => "Failed",
            Self::ParseError => "Parse Error",
            Self::ReadError => "Read Error",
            Self::NoEndpoint => "No Endpoint",
            Self::Cancelled => "Cancelled",
        }
    }

    pub(crate) fn from_verdict(verdict: ReachabilityVerdict) -> Self {
        match verdict {
            ReachabilityVerdict::Resolved => Self::Resolved,
            ReachabilityVerdict::Reachable => Self::Reachable,
            ReachabilityVerdict::PartiallyReachable => Self::PartiallyReachable,
            ReachabilityVerdict::Failed => Self::Failed,
        }
    }
}

#[derive(Clone)]
pub(crate) struct ReachabilityBatchRow {
    pub(crate) config_name: SharedString,
    pub(crate) peer_label: SharedString,
    pub(crate) target: SharedString,
    pub(crate) status: ReachabilityBatchStatus,
    pub(crate) summary: SharedString,
}

#[derive(Clone)]
pub(crate) struct ReachabilityBatchResult {
    pub(crate) total_configs: usize,
    pub(crate) endpoint_rows: usize,
    pub(crate) reachable_rows: usize,
    pub(crate) partial_rows: usize,
    pub(crate) failed_rows: usize,
    pub(crate) issue_rows: usize,
    pub(crate) rows: Vec<ReachabilityBatchRow>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReachabilityAuditProgress {
    pub(crate) phase: ReachabilityAuditPhase,
    pub(crate) total_configs: usize,
    pub(crate) processed_configs: usize,
    pub(crate) total_endpoints: usize,
    pub(crate) completed_endpoints: usize,
}

pub(crate) struct ReachabilitySingleViewModel {
    pub(crate) result: ReachabilityResult,
}

pub(crate) struct ReachabilityAuditViewModel {
    pub(crate) result: ReachabilityBatchResult,
}

pub(crate) struct ReachabilityToolState {
    pub(crate) target_input: Option<Entity<InputState>>,
    pub(crate) port_input: Option<Entity<InputState>>,
    pub(crate) timeout_input: Option<Entity<InputState>>,
    pub(crate) target_subscription: Option<Subscription>,
    pub(crate) port_subscription: Option<Subscription>,
    pub(crate) timeout_subscription: Option<Subscription>,
    pub(crate) active_tab: ReachabilityTab,
    pub(crate) form: ReachabilityFormState,
    pub(crate) form_error: Option<SharedString>,
    pub(crate) audit_filter: ReachabilityAuditFilter,
    pub(crate) audit_progress: Option<ReachabilityAuditProgress>,
    pub(crate) single_generation: u64,
    pub(crate) single: AsyncJobState<ReachabilitySingleViewModel>,
    pub(crate) audit_generation: u64,
    pub(crate) audit: AsyncJobState<ReachabilityAuditViewModel>,
}

impl Default for ReachabilityToolState {
    fn default() -> Self {
        Self {
            target_input: None,
            port_input: None,
            timeout_input: None,
            target_subscription: None,
            port_subscription: None,
            timeout_subscription: None,
            active_tab: ReachabilityTab::Single,
            form: ReachabilityFormState::default(),
            form_error: None,
            audit_filter: ReachabilityAuditFilter::All,
            audit_progress: None,
            single_generation: 0,
            single: AsyncJobState::Idle,
            audit_generation: 0,
            audit: AsyncJobState::Idle,
        }
    }
}
