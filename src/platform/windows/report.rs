use crate::core::config::InterfaceAddress;
use crate::core::route_plan::{
    RouteApplyFailureKind, RouteApplyKind, RouteApplyReport, RoutePlan,
};
use crate::platform::{NetworkApplyError, NetworkApplyResult};

use super::adapter::AdapterInfo;
use super::cleanup_pipeline::cleanup_network_config;
use super::dns::DnsState;
use super::error::NetworkError;
use super::firewall::DnsGuardState;
use super::metrics::InterfaceMetricState;
use super::nrpt::NrptState;
use super::recovery::{write_persisted_apply_report, RecoveryGuard};
use super::routes::RouteEntry;

/// State recorded after a Windows network apply so cleanup can roll it back.
pub struct AppliedNetworkState {
    pub(super) tun_name: String,
    pub(super) adapter: AdapterInfo,
    pub(super) addresses: Vec<InterfaceAddress>,
    pub(super) routes: Vec<RouteEntry>,
    pub(super) bypass_routes: Vec<RouteEntry>,
    pub(super) iface_metrics: Vec<InterfaceMetricState>,
    pub(super) dns: Option<DnsState>,
    pub(super) nrpt: Option<NrptState>,
    pub(super) dns_guard: Option<DnsGuardState>,
    pub(super) recovery: Option<RecoveryGuard>,
}

pub(super) fn apply_stage_failed(
    stage_id: impl Into<String>,
    kind: RouteApplyKind,
    failure_kind: Option<RouteApplyFailureKind>,
    evidence: Vec<String>,
    error: NetworkError,
    mut report: RouteApplyReport,
    route_plan: &RoutePlan,
    metric_from: usize,
    bypass_from: usize,
    route_from: usize,
    skip_addresses: bool,
    skip_dns: bool,
    skip_nrpt: bool,
    skip_dns_guard: bool,
    reason: &str,
) -> NetworkApplyError {
    report.push_failed_kind(stage_id, kind, failure_kind, evidence);
    skip_remaining_route_plan_ops(
        &mut report,
        route_plan,
        metric_from,
        bypass_from,
        route_from,
        reason,
    );
    skip_remaining_windows_stages(
        &mut report,
        skip_addresses,
        skip_dns,
        skip_nrpt,
        skip_dns_guard,
        reason,
    );
    abort_with_report(error, report)
}

pub(super) async fn apply_stage_failed_with_cleanup(
    state: AppliedNetworkState,
    stage_id: impl Into<String>,
    kind: RouteApplyKind,
    failure_kind: Option<RouteApplyFailureKind>,
    evidence: Vec<String>,
    error: NetworkError,
    mut report: RouteApplyReport,
    route_plan: &RoutePlan,
    metric_from: usize,
    bypass_from: usize,
    route_from: usize,
    skip_addresses: bool,
    skip_dns: bool,
    skip_nrpt: bool,
    skip_dns_guard: bool,
    reason: &str,
) -> Result<NetworkApplyResult, NetworkApplyError> {
    report.push_failed_kind(stage_id, kind, failure_kind, evidence);
    skip_remaining_route_plan_ops(
        &mut report,
        route_plan,
        metric_from,
        bypass_from,
        route_from,
        reason,
    );
    skip_remaining_windows_stages(
        &mut report,
        skip_addresses,
        skip_dns,
        skip_nrpt,
        skip_dns_guard,
        reason,
    );
    abort_with_cleanup(state, error, report).await
}

fn skip_remaining_route_plan_ops(
    report: &mut RouteApplyReport,
    route_plan: &RoutePlan,
    metric_from: usize,
    bypass_from: usize,
    route_from: usize,
    reason: &str,
) {
    let evidence = vec![reason.to_string()];

    for op in route_plan.metric_ops.iter().skip(metric_from) {
        report.push_skipped_kind(
            RoutePlan::metric_item_id(op),
            RouteApplyKind::Metric,
            evidence.clone(),
        );
    }
    for op in route_plan.bypass_ops.iter().skip(bypass_from) {
        report.push_skipped_kind(
            RoutePlan::bypass_item_id(op),
            RouteApplyKind::BypassRoute,
            evidence.clone(),
        );
    }
    for op in route_plan.route_ops.iter().skip(route_from) {
        report.push_skipped_kind(
            RoutePlan::route_item_id(op),
            RouteApplyKind::Route,
            evidence.clone(),
        );
    }
}

fn skip_remaining_windows_stages(
    report: &mut RouteApplyReport,
    skip_addresses: bool,
    skip_dns: bool,
    skip_nrpt: bool,
    skip_dns_guard: bool,
    reason: &str,
) {
    let evidence = vec![reason.to_string()];

    if skip_addresses {
        report.push_skipped_kind("apply:addresses", RouteApplyKind::Address, evidence.clone());
    }
    if skip_dns {
        report.push_skipped_kind("apply:dns", RouteApplyKind::Dns, evidence.clone());
    }
    if skip_nrpt {
        report.push_skipped_kind("apply:nrpt", RouteApplyKind::Nrpt, evidence.clone());
    }
    if skip_dns_guard {
        report.push_skipped_kind(
            "apply:dns_guard",
            RouteApplyKind::DnsGuard,
            evidence.clone(),
        );
    }
}

fn persist_apply_report_best_effort(report: &RouteApplyReport) {
    let _ = write_persisted_apply_report(report);
}

pub(super) fn abort_with_report(
    error: NetworkError,
    mut report: RouteApplyReport,
) -> NetworkApplyError {
    report.mark_failed();
    persist_apply_report_best_effort(&report);
    NetworkApplyError { error, report }
}

async fn abort_with_cleanup(
    state: AppliedNetworkState,
    error: NetworkError,
    report: RouteApplyReport,
) -> Result<NetworkApplyResult, NetworkApplyError> {
    let _ = cleanup_network_config(state).await;
    Err(abort_with_report(error, report))
}

pub(super) fn finish_with_report(
    state: AppliedNetworkState,
    mut report: RouteApplyReport,
) -> Result<NetworkApplyResult, NetworkApplyError> {
    report.mark_running();
    persist_apply_report_best_effort(&report);
    Ok(NetworkApplyResult { state, report })
}
