use std::net::IpAddr;

use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6};

use crate::core::config::{RouteTable, WireGuardConfig};
use crate::core::route_plan::{
    RouteApplyFailureKind, RouteApplyKind, RouteApplyReport, RoutePlan, RoutePlanBypassOp,
    RoutePlanFamily, RoutePlanRouteKind,
};
use crate::log::events::{dns as log_dns, net as log_net};
use crate::platform::{NetworkApplyError, NetworkApplyResult};

use super::adapter::{self, AdapterInfo};
use super::addresses::{add_unicast_address, cleanup_stale_unicast_addresses};
use super::dns::apply_dns;
use super::error::NetworkError;
use super::firewall::apply_dns_guard;
use super::metrics::set_interface_metric;
use super::nrpt::apply_nrpt_guard;
use super::recovery::RecoveryGuard;
use super::report::{
    apply_stage_failed, apply_stage_failed_with_cleanup, finish_with_report, AppliedNetworkState,
};
use super::routes::{add_route, best_route_to, resolve_bypass_ips_for_op, RouteEntry};
use super::TUNNEL_METRIC;

struct ApplyContext<'a> {
    config: &'a WireGuardConfig,
    route_plan: &'a RoutePlan,
    report: RouteApplyReport,
    state: AppliedNetworkState,
    full_v4: bool,
    full_v6: bool,
}

struct ResolvedBypassOp {
    op: RoutePlanBypassOp,
    endpoint_ips: Vec<IpAddr>,
}

impl<'a> ApplyContext<'a> {
    fn new(
        tun_name: &str,
        config: &'a WireGuardConfig,
        route_plan: &'a RoutePlan,
        report: RouteApplyReport,
        adapter: AdapterInfo,
        recovery: RecoveryGuard,
    ) -> Self {
        let full_v4 = route_plan
            .metric_ops
            .iter()
            .any(|op| matches!(op.family, RoutePlanFamily::Ipv4));
        let full_v6 = route_plan
            .metric_ops
            .iter()
            .any(|op| matches!(op.family, RoutePlanFamily::Ipv6));

        Self {
            config,
            route_plan,
            report,
            state: AppliedNetworkState {
                tun_name: tun_name.to_string(),
                adapter,
                addresses: Vec::new(),
                routes: Vec::new(),
                bypass_routes: Vec::new(),
                iface_metrics: Vec::new(),
                dns: None,
                nrpt: None,
                dns_guard: None,
                recovery: Some(recovery),
            },
            full_v4,
            full_v6,
        }
    }

    async fn abort_with_cleanup(
        self,
        stage_id: impl Into<String>,
        kind: RouteApplyKind,
        failure_kind: Option<RouteApplyFailureKind>,
        evidence: Vec<String>,
        error: NetworkError,
        metric_from: usize,
        bypass_from: usize,
        route_from: usize,
        skip_addresses: bool,
        skip_dns: bool,
        skip_nrpt: bool,
        skip_dns_guard: bool,
        reason: &str,
    ) -> Result<NetworkApplyResult, NetworkApplyError> {
        apply_stage_failed_with_cleanup(
            self.state,
            stage_id,
            kind,
            failure_kind,
            evidence,
            error,
            self.report,
            self.route_plan,
            metric_from,
            bypass_from,
            route_from,
            skip_addresses,
            skip_dns,
            skip_nrpt,
            skip_dns_guard,
            reason,
        )
        .await
    }

    async fn abort_error_with_cleanup(
        self,
        stage_id: impl Into<String>,
        kind: RouteApplyKind,
        failure_kind: Option<RouteApplyFailureKind>,
        evidence: Vec<String>,
        error: NetworkError,
        metric_from: usize,
        bypass_from: usize,
        route_from: usize,
        skip_addresses: bool,
        skip_dns: bool,
        skip_nrpt: bool,
        skip_dns_guard: bool,
        reason: &str,
    ) -> NetworkApplyError {
        match self
            .abort_with_cleanup(
                stage_id,
                kind,
                failure_kind,
                evidence,
                error,
                metric_from,
                bypass_from,
                route_from,
                skip_addresses,
                skip_dns,
                skip_nrpt,
                skip_dns_guard,
                reason,
            )
            .await
        {
            Err(err) => err,
            Ok(_) => unreachable!("cleanup abort should always return an error"),
        }
    }
}

/// Apply Windows network configuration via a staged pipeline.
pub async fn apply_network_config(
    tun_name: &str,
    config: &WireGuardConfig,
    route_plan: &RoutePlan,
) -> Result<NetworkApplyResult, NetworkApplyError> {
    log_apply_prelude(tun_name, config);

    let report = RouteApplyReport::new(route_plan.platform);
    let (adapter, report) = lookup_adapter(tun_name, route_plan, report).await?;
    let (recovery, report) = begin_recovery(tun_name, adapter, route_plan, report)?;

    let ctx = ApplyContext::new(tun_name, config, route_plan, report, adapter, recovery);
    let ctx = cleanup_stale_addresses(ctx)?;
    let ctx = apply_addresses(ctx).await?;
    let ctx = apply_interface_metrics(ctx).await?;
    let (ctx, resolved_bypass_ops) = resolve_bypass_targets(ctx).await?;
    let ctx = apply_bypass_routes(ctx, resolved_bypass_ops).await?;
    let ctx = apply_route_entries(ctx).await?;
    let ctx = apply_dns_stage(ctx).await?;
    let ctx = apply_guard_stages(ctx).await?;
    let ctx = mark_recovery_running(ctx).await?;

    finish_with_report(ctx.state, ctx.report)
}

fn log_apply_prelude(tun_name: &str, config: &WireGuardConfig) {
    log_net::apply_windows(
        tun_name,
        config.interface.addresses.len(),
        config.interface.dns_servers.len(),
        config.interface.dns_search.len(),
    );

    if let Some(RouteTable::Id(id)) = config.interface.table {
        log_net::route_table_id_ignored(id);
    }
    if config.interface.fwmark.is_some() {
        log_net::fwmark_ignored();
    }
}

async fn lookup_adapter(
    tun_name: &str,
    route_plan: &RoutePlan,
    mut report: RouteApplyReport,
) -> Result<(AdapterInfo, RouteApplyReport), NetworkApplyError> {
    match adapter::find_adapter_with_retry(tun_name).await {
        Ok(adapter) => {
            report.push_applied_kind(
                "apply:adapter_lookup",
                RouteApplyKind::Adapter,
                vec![format!("Located Windows adapter {tun_name}.")],
            );
            Ok((adapter, report))
        }
        Err(error) => Err(apply_stage_failed(
            "apply:adapter_lookup",
            RouteApplyKind::Adapter,
            Some(RouteApplyFailureKind::Lookup),
            vec![format!(
                "failed to locate Windows adapter {tun_name}: {error}"
            )],
            error,
            report,
            route_plan,
            0,
            0,
            0,
            true,
            true,
            true,
            true,
            "Windows apply aborted before adapter lookup completed.",
        )),
    }
}

fn begin_recovery(
    tun_name: &str,
    adapter: AdapterInfo,
    route_plan: &RoutePlan,
    report: RouteApplyReport,
) -> Result<(RecoveryGuard, RouteApplyReport), NetworkApplyError> {
    match RecoveryGuard::begin(tun_name, adapter) {
        Ok(guard) => Ok((guard, report)),
        Err(error) => Err(apply_stage_failed(
            "apply:recovery_init",
            RouteApplyKind::RecoveryJournal,
            Some(RouteApplyFailureKind::Persistence),
            vec![format!(
                "failed to initialize Windows recovery journal: {error}"
            )],
            error,
            report,
            route_plan,
            0,
            0,
            0,
            true,
            true,
            true,
            true,
            "Windows apply aborted before any planned network operation ran.",
        )),
    }
}

fn cleanup_stale_addresses<'a>(
    ctx: ApplyContext<'a>,
) -> Result<ApplyContext<'a>, NetworkApplyError> {
    if let Err(error) =
        cleanup_stale_unicast_addresses(ctx.state.adapter, &ctx.config.interface.addresses)
    {
        return Err(apply_stage_failed(
            "apply:stale_address_cleanup",
            RouteApplyKind::Address,
            Some(RouteApplyFailureKind::Cleanup),
            vec![format!(
                "failed to clear stale Windows interface addresses before apply: {error}"
            )],
            error,
            ctx.report,
            ctx.route_plan,
            0,
            0,
            0,
            true,
            true,
            true,
            true,
            "Windows apply aborted before route planning could proceed.",
        ));
    }
    Ok(ctx)
}

async fn apply_addresses<'a>(
    mut ctx: ApplyContext<'a>,
) -> Result<ApplyContext<'a>, NetworkApplyError> {
    for address in ctx.config.interface.addresses.iter().cloned() {
        log_net::address_add_windows(address.addr, address.cidr);
        if let Err(error) = add_unicast_address(ctx.state.adapter, &address) {
            return Err(ctx
                .abort_error_with_cleanup(
                    format!("apply:address:{}/{}", address.addr, address.cidr),
                    RouteApplyKind::Address,
                    Some(RouteApplyFailureKind::System),
                    vec![format!(
                        "failed to add Windows interface address {}/{}: {error}",
                        address.addr, address.cidr
                    )],
                    error,
                    0,
                    0,
                    0,
                    false,
                    true,
                    true,
                    true,
                    "Windows apply aborted while configuring interface addresses.",
                )
                .await);
        }
        ctx.state.addresses.push(address.clone());
        ctx.report.push_applied_kind(
            format!("apply:address:{}/{}", address.addr, address.cidr),
            RouteApplyKind::Address,
            vec![format!(
                "Applied Windows interface address {}/{}.",
                address.addr, address.cidr
            )],
        );
        if let Some(recovery) = ctx.state.recovery.as_mut() {
            if let Err(error) = recovery.record_address(&address) {
                return Err(apply_stage_failed(
                    "apply:recovery",
                    RouteApplyKind::RecoveryJournal,
                    Some(RouteApplyFailureKind::Persistence),
                    vec![format!(
                        "failed to persist Windows recovery journal after adding address {}/{}: {error}",
                        address.addr, address.cidr
                    )],
                    error,
                    ctx.report,
                    ctx.route_plan,
                    0,
                    0,
                    0,
                    false,
                    true,
                    true,
                    true,
                    "Windows apply aborted while persisting recovery journal.",
                ));
            }
        }
    }

    Ok(ctx)
}

async fn apply_interface_metrics<'a>(
    mut ctx: ApplyContext<'a>,
) -> Result<ApplyContext<'a>, NetworkApplyError> {
    for (index, op) in ctx.route_plan.metric_ops.iter().cloned().enumerate() {
        let metric_result = match op.family {
            RoutePlanFamily::Ipv4 => set_interface_metric(ctx.state.adapter, AF_INET, op.metric),
            RoutePlanFamily::Ipv6 => set_interface_metric(ctx.state.adapter, AF_INET6, op.metric),
        };
        match metric_result {
            Ok(metric_state) => {
                ctx.report.push_applied_kind(
                    RoutePlan::metric_item_id(&op),
                    RouteApplyKind::Metric,
                    vec![format!(
                        "Applied Windows {} interface metric {}.",
                        op.family.label(),
                        op.metric
                    )],
                );
                match op.family {
                    RoutePlanFamily::Ipv4 => log_net::interface_metric_set_v4(op.metric),
                    RoutePlanFamily::Ipv6 => log_net::interface_metric_set_v6(op.metric),
                }
                ctx.state.iface_metrics.push(metric_state);
                if let Some(recovery) = ctx.state.recovery.as_mut() {
                    if let Err(error) = recovery.record_metric(metric_state) {
                        return Err(ctx
                            .abort_error_with_cleanup(
                                "apply:recovery",
                                RouteApplyKind::RecoveryJournal,
                                Some(RouteApplyFailureKind::Persistence),
                                vec![format!(
                                    "failed to persist Windows recovery journal after setting {:?} metric {}: {error}",
                                    op.family, op.metric
                                )],
                                error,
                                index + 1,
                                0,
                                0,
                                false,
                                true,
                                true,
                                true,
                                "Windows apply aborted while persisting recovery journal.",
                            )
                            .await);
                    }
                }
            }
            Err(error) => {
                match op.family {
                    RoutePlanFamily::Ipv4 => log_net::interface_metric_set_failed_v4(&error),
                    RoutePlanFamily::Ipv6 => log_net::interface_metric_set_failed_v6(&error),
                }
                return Err(ctx
                    .abort_error_with_cleanup(
                        RoutePlan::metric_item_id(&op),
                        RouteApplyKind::Metric,
                        Some(RouteApplyFailureKind::System),
                        vec![format!(
                            "failed to set Windows {:?} interface metric {}: {error}",
                            op.family, op.metric
                        )],
                        NetworkError::UnsafeRouting(format!(
                            "failed to set {:?} interface metric: {error}",
                            op.family
                        )),
                        index + 1,
                        0,
                        0,
                        false,
                        true,
                        true,
                        true,
                        "Windows apply aborted after interface metric configuration failed.",
                    )
                    .await);
            }
        }
    }

    Ok(ctx)
}

async fn resolve_bypass_targets<'a>(
    mut ctx: ApplyContext<'a>,
) -> Result<(ApplyContext<'a>, Vec<ResolvedBypassOp>), NetworkApplyError> {
    let metric_count = ctx.route_plan.metric_ops.len();
    let mut resolved_bypass_ops = Vec::with_capacity(ctx.route_plan.bypass_ops.len());

    for (index, op) in ctx.route_plan.bypass_ops.iter().cloned().enumerate() {
        let endpoint_ips = match resolve_bypass_ips_for_op(&op).await {
            Ok(endpoint_ips) => endpoint_ips,
            Err(error) => {
                for prior_op in ctx.route_plan.bypass_ops.iter().take(index) {
                    ctx.report.push_skipped_kind(
                        RoutePlan::bypass_item_id(prior_op),
                        RouteApplyKind::BypassRoute,
                        vec![String::from(
                            "Windows apply aborted before bypass routes could be applied.",
                        )],
                    );
                }
                return Err(ctx
                    .abort_error_with_cleanup(
                        RoutePlan::bypass_item_id(&op),
                        RouteApplyKind::BypassRoute,
                        Some(RouteApplyFailureKind::Lookup),
                        vec![format!(
                            "failed to resolve Windows endpoint bypass {}:{}: {error}",
                            op.host, op.port
                        )],
                        error,
                        metric_count,
                        index + 1,
                        0,
                        false,
                        true,
                        true,
                        true,
                        "Windows apply aborted while resolving endpoint bypass routes.",
                    )
                    .await);
            }
        };
        resolved_bypass_ops.push(ResolvedBypassOp { op, endpoint_ips });
    }

    Ok((ctx, resolved_bypass_ops))
}

async fn apply_bypass_routes<'a>(
    mut ctx: ApplyContext<'a>,
    resolved_bypass_ops: Vec<ResolvedBypassOp>,
) -> Result<ApplyContext<'a>, NetworkApplyError> {
    let metric_count = ctx.route_plan.metric_ops.len();

    for (bypass_index, resolved) in resolved_bypass_ops.iter().enumerate() {
        let mut applicable_endpoints = 0usize;
        let mut routable_endpoints = 0usize;
        let mut applied_bypass_routes = 0usize;

        for ip in &resolved.endpoint_ips {
            if ip.is_ipv4() {
                if !ctx.full_v4 {
                    continue;
                }
            } else if !ctx.full_v6 {
                continue;
            }
            applicable_endpoints += 1;

            let route = match best_route_to(*ip) {
                Ok(route) => route,
                Err(error) => {
                    log_net::bypass_route_failed(*ip, &error);
                    continue;
                }
            };
            routable_endpoints += 1;

            log_net::bypass_route_add(route.dest, route.next_hop, route.if_index);
            if let Err(error) = add_route(&route) {
                log_net::bypass_route_add_failed(route.dest, &error);
                return Err(ctx
                    .abort_error_with_cleanup(
                        RoutePlan::bypass_item_id(&resolved.op),
                        RouteApplyKind::BypassRoute,
                        Some(RouteApplyFailureKind::System),
                        vec![format!(
                            "failed to add Windows endpoint bypass route {}/{} for {}:{}: {error}",
                            route.dest, route.prefix, resolved.op.host, resolved.op.port
                        )],
                        error,
                        metric_count,
                        bypass_index + 1,
                        0,
                        false,
                        true,
                        true,
                        true,
                        "Windows apply aborted while adding endpoint bypass routes.",
                    )
                    .await);
            }
            ctx.state.bypass_routes.push(route.clone());
            if let Some(recovery) = ctx.state.recovery.as_mut() {
                if let Err(error) = recovery.record_bypass_route(&route) {
                    return Err(ctx
                        .abort_error_with_cleanup(
                            "apply:recovery",
                            RouteApplyKind::RecoveryJournal,
                            Some(RouteApplyFailureKind::Persistence),
                            vec![format!(
                                "failed to persist Windows recovery journal after adding bypass route {}/{} for {}:{}: {error}",
                                route.dest, route.prefix, resolved.op.host, resolved.op.port
                            )],
                            error,
                            metric_count,
                            bypass_index + 1,
                            0,
                            false,
                            true,
                            true,
                            true,
                            "Windows apply aborted while persisting recovery journal.",
                        )
                        .await);
                }
            }

            applied_bypass_routes += 1;
        }

        if applied_bypass_routes > 0 {
            ctx.report.push_applied_kind(
                RoutePlan::bypass_item_id(&resolved.op),
                RouteApplyKind::BypassRoute,
                vec![format!(
                    "Applied Windows endpoint bypass route handling for {}:{}.",
                    resolved.op.host, resolved.op.port
                )],
            );
        } else if applicable_endpoints == 0 {
            ctx.report.push_skipped_kind(
                RoutePlan::bypass_item_id(&resolved.op),
                RouteApplyKind::BypassRoute,
                vec![format!(
                    "No resolved endpoint IP for {}:{} matched an active full-tunnel address family.",
                    resolved.op.host, resolved.op.port
                )],
            );
        } else if routable_endpoints == 0 {
            ctx.report.push_skipped_kind(
                RoutePlan::bypass_item_id(&resolved.op),
                RouteApplyKind::BypassRoute,
                vec![format!(
                    "Resolved endpoint IPs for {}:{} had no usable Windows underlay route.",
                    resolved.op.host, resolved.op.port
                )],
            );
        }
    }

    Ok(ctx)
}

async fn apply_route_entries<'a>(
    mut ctx: ApplyContext<'a>,
) -> Result<ApplyContext<'a>, NetworkApplyError> {
    let metric_count = ctx.route_plan.metric_ops.len();
    let bypass_count = ctx.route_plan.bypass_ops.len();

    for (index, op) in ctx.route_plan.route_ops.iter().cloned().enumerate() {
        let entry = RouteEntry {
            dest: op.route.addr,
            prefix: op.route.cidr,
            next_hop: None,
            if_index: ctx.state.adapter.if_index,
            luid: ctx.state.adapter.luid,
        };
        match op.kind {
            RoutePlanRouteKind::Allowed => log_net::route_add_windows(
                entry.dest,
                entry.prefix,
                entry.next_hop,
                entry.if_index,
                TUNNEL_METRIC,
            ),
            RoutePlanRouteKind::DnsHost => log_net::dns_route_add_windows(
                entry.dest,
                entry.prefix,
                entry.if_index,
                TUNNEL_METRIC,
            ),
        }
        if let Err(error) = add_route(&entry) {
            return Err(ctx
                .abort_error_with_cleanup(
                    RoutePlan::route_item_id(&op),
                    RouteApplyKind::Route,
                    Some(RouteApplyFailureKind::System),
                    vec![format!(
                        "failed to add Windows route {}/{}: {error}",
                        op.route.addr, op.route.cidr
                    )],
                    error,
                    metric_count,
                    bypass_count,
                    index + 1,
                    false,
                    true,
                    true,
                    true,
                    "Windows apply aborted while adding route entries.",
                )
                .await);
        }
        ctx.report.push_applied_kind(
            RoutePlan::route_item_id(&op),
            RouteApplyKind::Route,
            vec![match op.kind {
                RoutePlanRouteKind::Allowed => format!(
                    "Applied Windows route {}/{} on the tunnel adapter.",
                    op.route.addr, op.route.cidr
                ),
                RoutePlanRouteKind::DnsHost => format!(
                    "Applied Windows DNS host route {}/{} on the tunnel adapter.",
                    op.route.addr, op.route.cidr
                ),
            }],
        );
        ctx.state.routes.push(entry.clone());
        if let Some(recovery) = ctx.state.recovery.as_mut() {
            if let Err(error) = recovery.record_route(&entry) {
                return Err(ctx
                    .abort_error_with_cleanup(
                        "apply:recovery",
                        RouteApplyKind::RecoveryJournal,
                        Some(RouteApplyFailureKind::Persistence),
                        vec![format!(
                            "failed to persist Windows recovery journal after adding route {}/{}: {error}",
                            op.route.addr, op.route.cidr
                        )],
                        error,
                        metric_count,
                        bypass_count,
                        index + 1,
                        false,
                        true,
                        true,
                        true,
                        "Windows apply aborted while persisting recovery journal.",
                    )
                    .await);
            }
        }
    }

    Ok(ctx)
}

async fn apply_dns_stage<'a>(
    mut ctx: ApplyContext<'a>,
) -> Result<ApplyContext<'a>, NetworkApplyError> {
    let metric_count = ctx.route_plan.metric_ops.len();
    let bypass_count = ctx.route_plan.bypass_ops.len();
    let route_count = ctx.route_plan.route_ops.len();

    if ctx.config.interface.dns_servers.is_empty() && ctx.config.interface.dns_search.is_empty() {
        return Ok(ctx);
    }

    log_dns::apply_summary(
        ctx.config.interface.dns_servers.len(),
        ctx.config.interface.dns_search.len(),
    );
    match apply_dns(
        ctx.state.adapter,
        &ctx.config.interface.dns_servers,
        &ctx.config.interface.dns_search,
    ) {
        Ok(dns_state) => {
            ctx.report.push_applied_kind(
                "apply:dns",
                RouteApplyKind::Dns,
                vec![format!(
                    "Applied Windows DNS settings (servers={}, search={}).",
                    ctx.config.interface.dns_servers.len(),
                    ctx.config.interface.dns_search.len()
                )],
            );
            ctx.state.dns = Some(dns_state);
            if let Some(recovery) = ctx.state.recovery.as_mut() {
                if let Some(dns_state) = ctx.state.dns.as_ref() {
                    if let Err(error) = recovery.record_dns(dns_state) {
                        return Err(ctx
                            .abort_error_with_cleanup(
                                "apply:recovery",
                                RouteApplyKind::RecoveryJournal,
                                Some(RouteApplyFailureKind::Persistence),
                                vec![format!(
                                    "failed to persist Windows recovery journal after applying DNS: {error}"
                                )],
                                error,
                                metric_count,
                                bypass_count,
                                route_count,
                                false,
                                false,
                                true,
                                true,
                                "Windows apply aborted while persisting recovery journal.",
                            )
                            .await);
                    }
                }
            }
        }
        Err(error) => {
            log_dns::apply_failed(&error);
            return Err(ctx
                .abort_error_with_cleanup(
                    "apply:dns",
                    RouteApplyKind::Dns,
                    Some(RouteApplyFailureKind::System),
                    vec![format!("failed to apply Windows DNS settings: {error}")],
                    error,
                    metric_count,
                    bypass_count,
                    route_count,
                    false,
                    false,
                    true,
                    true,
                    "Windows apply aborted after DNS configuration failed.",
                )
                .await);
        }
    }

    Ok(ctx)
}

async fn apply_guard_stages<'a>(
    mut ctx: ApplyContext<'a>,
) -> Result<ApplyContext<'a>, NetworkApplyError> {
    let metric_count = ctx.route_plan.metric_ops.len();
    let bypass_count = ctx.route_plan.bypass_ops.len();
    let route_count = ctx.route_plan.route_ops.len();

    if ctx.config.interface.dns_servers.is_empty()
        || ctx.config.interface.table == Some(RouteTable::Off)
        || (!ctx.full_v4 && !ctx.full_v6)
    {
        return Ok(ctx);
    }

    match apply_nrpt_guard(ctx.state.adapter, &ctx.config.interface.dns_servers) {
        Ok(nrpt_state) => {
            ctx.report.push_applied_kind(
                "apply:nrpt",
                RouteApplyKind::Nrpt,
                vec![format!(
                    "Applied Windows NRPT guard for {} tunnel DNS server(s).",
                    ctx.config.interface.dns_servers.len()
                )],
            );
            ctx.state.nrpt = nrpt_state;
            if let Some(recovery) = ctx.state.recovery.as_mut() {
                if let Some(nrpt_state) = ctx.state.nrpt.as_ref() {
                    if let Err(error) = recovery.record_nrpt(nrpt_state) {
                        return Err(ctx
                            .abort_error_with_cleanup(
                                "apply:recovery",
                                RouteApplyKind::RecoveryJournal,
                                Some(RouteApplyFailureKind::Persistence),
                                vec![format!(
                                    "failed to persist Windows recovery journal after applying NRPT: {error}"
                                )],
                                error,
                                metric_count,
                                bypass_count,
                                route_count,
                                false,
                                false,
                                false,
                                true,
                                "Windows apply aborted while persisting recovery journal.",
                            )
                            .await);
                    }
                }
            }
        }
        Err(error) => {
            return Err(ctx
                .abort_error_with_cleanup(
                    "apply:nrpt",
                    RouteApplyKind::Nrpt,
                    Some(RouteApplyFailureKind::System),
                    vec![format!("failed to apply Windows NRPT guard: {error}")],
                    error,
                    metric_count,
                    bypass_count,
                    route_count,
                    false,
                    false,
                    false,
                    true,
                    "Windows apply aborted after NRPT configuration failed.",
                )
                .await);
        }
    }

    match apply_dns_guard(
        ctx.state.adapter,
        ctx.full_v4,
        ctx.full_v6,
        &ctx.config.interface.dns_servers,
    ) {
        Ok(guard_state) => {
            ctx.report.push_applied_kind(
                "apply:dns_guard",
                RouteApplyKind::DnsGuard,
                vec![format!(
                    "Applied Windows DNS guard for {} tunnel DNS server(s).",
                    ctx.config.interface.dns_servers.len()
                )],
            );
            ctx.state.dns_guard = guard_state;
            if let Some(recovery) = ctx.state.recovery.as_mut() {
                if let Some(guard_state) = ctx.state.dns_guard.as_ref() {
                    if let Err(error) = recovery.record_dns_guard(guard_state) {
                        return Err(ctx
                            .abort_error_with_cleanup(
                                "apply:recovery",
                                RouteApplyKind::RecoveryJournal,
                                Some(RouteApplyFailureKind::Persistence),
                                vec![format!(
                                    "failed to persist Windows recovery journal after applying DNS guard: {error}"
                                )],
                                error,
                                metric_count,
                                bypass_count,
                                route_count,
                                false,
                                false,
                                false,
                                false,
                                "Windows apply aborted while persisting recovery journal.",
                            )
                            .await);
                    }
                }
            }
        }
        Err(error) => {
            return Err(ctx
                .abort_error_with_cleanup(
                    "apply:dns_guard",
                    RouteApplyKind::DnsGuard,
                    Some(RouteApplyFailureKind::System),
                    vec![format!("failed to apply Windows DNS guard: {error}")],
                    error,
                    metric_count,
                    bypass_count,
                    route_count,
                    false,
                    false,
                    false,
                    false,
                    "Windows apply aborted after DNS guard configuration failed.",
                )
                .await);
        }
    }

    Ok(ctx)
}

async fn mark_recovery_running<'a>(
    mut ctx: ApplyContext<'a>,
) -> Result<ApplyContext<'a>, NetworkApplyError> {
    let metric_count = ctx.route_plan.metric_ops.len();
    let bypass_count = ctx.route_plan.bypass_ops.len();
    let route_count = ctx.route_plan.route_ops.len();

    if let Some(recovery) = ctx.state.recovery.as_mut() {
        if let Err(error) = recovery.mark_running() {
            return Err(ctx
                .abort_error_with_cleanup(
                    "apply:recovery",
                    RouteApplyKind::RecoveryJournal,
                    Some(RouteApplyFailureKind::Persistence),
                    vec![format!(
                        "failed to mark Windows recovery journal running: {error}"
                    )],
                    error,
                    metric_count,
                    bypass_count,
                    route_count,
                    false,
                    false,
                    false,
                    false,
                    "Windows apply aborted while marking the recovery journal running.",
                )
                .await);
        }
    }
    Ok(ctx)
}
