//! Windows 网络配置入口。
//!
//! 主要职责：
//! - 配置 TUN 接口地址与 AllowedIPs 路由；
//! - 在全隧道时生成并下发 Endpoint bypass route，避免握手被自身默认路由截断；
//! - 应用 DNS、NRPT 与 DNS 防泄露规则；
//! - 在失败或断开时按顺序回滚。

mod adapter;
mod addresses;
mod dns;
mod firewall;
mod metrics;
mod nrpt;
mod recovery;
mod routes;
mod sockaddr;

use std::fmt;

use windows::core::PWSTR;
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, ERROR_OBJECT_ALREADY_EXISTS, WIN32_ERROR};
use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6};

use crate::backend::wg::config::{InterfaceAddress, RouteTable, WireGuardConfig};
use crate::backend::wg::route_plan::{
    RouteApplyFailureKind, RouteApplyKind, RouteApplyReport, RoutePlan, RoutePlanFamily,
    RoutePlanRouteKind,
};
use crate::log::events::{dns as log_dns, net as log_net};
use crate::platform::{NetworkApplyError, NetworkApplyResult};

use adapter::AdapterInfo;
use addresses::{add_unicast_address, cleanup_stale_unicast_addresses, delete_unicast_address};
use dns::{apply_dns, cleanup_dns, DnsState};
use firewall::{apply_dns_guard, cleanup_dns_guard, DnsGuardState};
use metrics::{restore_interface_metric, set_interface_metric, InterfaceMetricState};
use nrpt::{apply_nrpt_guard, cleanup_nrpt_guard, NrptState};
use recovery::{
    clear_persisted_apply_report,
    load_persisted_apply_report as load_persisted_apply_report_from_disk,
    write_persisted_apply_report, RecoveryGuard,
};
use routes::{add_route, best_route_to, delete_route, resolve_bypass_ips_for_op, RouteEntry};

/// 隧道接口 metric。值越小优先级越高。
const TUNNEL_METRIC: u32 = 0;

/// 一次网络配置应用后的状态，用于后续清理回滚。
pub struct AppliedNetworkState {
    /// 隧道接口名（用于日志和清理）。
    tun_name: String,
    /// 目标网卡关键信息。
    adapter: AdapterInfo,
    /// 本次添加的接口地址。
    addresses: Vec<InterfaceAddress>,
    /// 本次添加的普通路由（AllowedIPs + DNS host route）。
    routes: Vec<RouteEntry>,
    /// 本次添加的 Endpoint bypass 路由。
    bypass_routes: Vec<RouteEntry>,
    /// metric 调整前快照（用于恢复）。
    iface_metrics: Vec<InterfaceMetricState>,
    /// DNS 变更状态（用于回滚）。
    dns: Option<DnsState>,
    /// NRPT 变更状态（用于回滚）。
    nrpt: Option<NrptState>,
    /// DNS Guard 状态（用于回滚）。
    dns_guard: Option<DnsGuardState>,
    /// 持久化恢复日志。
    recovery: Option<RecoveryGuard>,
}

#[derive(Debug)]
pub enum NetworkError {
    AdapterNotFound(String),
    EndpointResolve(String),
    /// 用于 fail-closed：检测到潜在泄露风险时主动失败。
    UnsafeRouting(String),
    Io(std::io::Error),
    Win32 {
        context: &'static str,
        code: WIN32_ERROR,
    },
}

impl fmt::Display for NetworkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetworkError::AdapterNotFound(name) => {
                write!(f, "adapter not found: {name}")
            }
            NetworkError::EndpointResolve(message) => {
                write!(f, "endpoint resolve failed: {message}")
            }
            NetworkError::UnsafeRouting(message) => {
                write!(f, "unsafe routing configuration: {message}")
            }
            NetworkError::Io(err) => write!(f, "io error: {err}"),
            NetworkError::Win32 { context, code } => {
                let err = std::io::Error::from_raw_os_error(code.0 as i32);
                write!(f, "{context}: {err} (code={})", code.0)
            }
        }
    }
}

impl std::error::Error for NetworkError {}

impl From<std::io::Error> for NetworkError {
    fn from(err: std::io::Error) -> Self {
        NetworkError::Io(err)
    }
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

fn abort_with_report(error: NetworkError, mut report: RouteApplyReport) -> NetworkApplyError {
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

fn finish_with_report(
    state: AppliedNetworkState,
    mut report: RouteApplyReport,
) -> Result<NetworkApplyResult, NetworkApplyError> {
    report.mark_running();
    persist_apply_report_best_effort(&report);
    finish_with_report(state, report)
}

/// 应用 Windows 侧网络配置。
///
/// 注意：任何关键步骤失败都会触发回滚，避免留下“半配置”状态。
pub async fn apply_network_config(
    tun_name: &str,
    config: &WireGuardConfig,
    route_plan: &RoutePlan,
) -> Result<NetworkApplyResult, NetworkApplyError> {
    // 1) 记录本次配置参数，便于问题排查。
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

    let mut report = RouteApplyReport::new(route_plan.platform);

    // 2) 查找目标 TUN 适配器。
    let adapter = match adapter::find_adapter_with_retry(tun_name).await {
        Ok(adapter) => adapter,
        Err(error) => {
            report.push_failed_kind(
                "apply:adapter_lookup",
                RouteApplyKind::Adapter,
                Some(RouteApplyFailureKind::Lookup),
                vec![format!(
                    "failed to locate Windows adapter {tun_name}: {error}"
                )],
            );
            skip_remaining_route_plan_ops(
                &mut report,
                route_plan,
                0,
                0,
                0,
                "Windows apply aborted before adapter lookup completed.",
            );
            skip_remaining_windows_stages(
                &mut report,
                true,
                true,
                true,
                true,
                "Windows apply aborted before adapter lookup completed.",
            );
            return Err(abort_with_report(error, report));
        }
    };

    let mut state = AppliedNetworkState {
        tun_name: tun_name.to_string(),
        adapter,
        addresses: Vec::new(),
        routes: Vec::new(),
        bypass_routes: Vec::new(),
        iface_metrics: Vec::new(),
        dns: None,
        nrpt: None,
        dns_guard: None,
        recovery: Some(match RecoveryGuard::begin(tun_name, adapter) {
            Ok(guard) => guard,
            Err(error) => {
                report.push_failed_kind(
                    "apply:recovery_init",
                    RouteApplyKind::RecoveryJournal,
                    Some(RouteApplyFailureKind::Persistence),
                    vec![format!(
                        "failed to initialize Windows recovery journal: {error}"
                    )],
                );
                skip_remaining_route_plan_ops(
                    &mut report,
                    route_plan,
                    0,
                    0,
                    0,
                    "Windows apply aborted before any planned network operation ran.",
                );
                skip_remaining_windows_stages(
                    &mut report,
                    true,
                    true,
                    true,
                    true,
                    "Windows apply aborted before any planned network operation ran.",
                );
                return Err(abort_with_report(error, report));
            }
        }),
    };

    // 3) 先清理旧地址，避免历史残留影响路由决策。
    if let Err(error) = cleanup_stale_unicast_addresses(adapter, &config.interface.addresses) {
        report.push_failed_kind(
            "apply:stale_address_cleanup",
            RouteApplyKind::Address,
            Some(RouteApplyFailureKind::Cleanup),
            vec![format!(
                "failed to clear stale Windows interface addresses before apply: {error}"
            )],
        );
        skip_remaining_route_plan_ops(
            &mut report,
            route_plan,
            0,
            0,
            0,
            "Windows apply aborted before route planning could proceed.",
        );
        skip_remaining_windows_stages(
            &mut report,
            true,
            true,
            true,
            true,
            "Windows apply aborted before route planning could proceed.",
        );
        return Err(abort_with_report(error, report));
    }

    // 4) 配置接口地址。
    for address in &config.interface.addresses {
        log_net::address_add_windows(address.addr, address.cidr);
        if let Err(error) = add_unicast_address(adapter, address) {
            report.push_failed_kind(
                format!("apply:address:{}/{}", address.addr, address.cidr),
                RouteApplyKind::Address,
                Some(RouteApplyFailureKind::System),
                vec![format!(
                    "failed to add Windows interface address {}/{}: {error}",
                    address.addr, address.cidr
                )],
            );
            skip_remaining_route_plan_ops(
                &mut report,
                route_plan,
                0,
                0,
                0,
                "Windows apply aborted while configuring interface addresses.",
            );
            skip_remaining_windows_stages(
                &mut report,
                false,
                true,
                true,
                true,
                "Windows apply aborted while configuring interface addresses.",
            );
            return abort_with_cleanup(state, error, report).await;
        }
        state.addresses.push(address.clone());
        report.push_applied_kind(
            format!("apply:address:{}/{}", address.addr, address.cidr),
            RouteApplyKind::Address,
            vec![format!(
                "Applied Windows interface address {}/{}.",
                address.addr, address.cidr
            )],
        );
        if let Some(recovery) = state.recovery.as_mut() {
            if let Err(error) = recovery.record_address(address) {
                report.push_failed_kind(
                    "apply:recovery",
                    RouteApplyKind::RecoveryJournal,
                    Some(RouteApplyFailureKind::Persistence),
                    vec![format!(
                        "failed to persist Windows recovery journal after adding address {}/{}: {error}",
                        address.addr, address.cidr
                    )],
                );
                skip_remaining_route_plan_ops(
                    &mut report,
                    route_plan,
                    0,
                    0,
                    0,
                    "Windows apply aborted while persisting recovery journal.",
                );
                skip_remaining_windows_stages(
                    &mut report,
                    false,
                    true,
                    true,
                    true,
                    "Windows apply aborted while persisting recovery journal.",
                );
                return Err(abort_with_report(error, report));
            }
        }
    }

    let full_v4 = route_plan
        .metric_ops
        .iter()
        .any(|op| matches!(op.family, RoutePlanFamily::Ipv4));
    let full_v6 = route_plan
        .metric_ops
        .iter()
        .any(|op| matches!(op.family, RoutePlanFamily::Ipv6));

    // 5) 按共享 route plan 下发接口 metric。
    for (index, op) in route_plan.metric_ops.iter().enumerate() {
        let metric_result = match op.family {
            RoutePlanFamily::Ipv4 => set_interface_metric(adapter, AF_INET, op.metric),
            RoutePlanFamily::Ipv6 => set_interface_metric(adapter, AF_INET6, op.metric),
        };
        match metric_result {
            Ok(metric_state) => {
                report.push_applied_kind(
                    RoutePlan::metric_item_id(op),
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
                state.iface_metrics.push(metric_state);
                if let Some(recovery) = state.recovery.as_mut() {
                    if let Err(error) = recovery.record_metric(metric_state) {
                        report.push_failed_kind(
                            "apply:recovery",
                            RouteApplyKind::RecoveryJournal,
                            Some(RouteApplyFailureKind::Persistence),
                            vec![format!(
                                "failed to persist Windows recovery journal after setting {:?} metric {}: {error}",
                                op.family, op.metric
                            )],
                        );
                        skip_remaining_route_plan_ops(
                            &mut report,
                            route_plan,
                            index + 1,
                            0,
                            0,
                            "Windows apply aborted while persisting recovery journal.",
                        );
                        skip_remaining_windows_stages(
                            &mut report,
                            false,
                            true,
                            true,
                            true,
                            "Windows apply aborted while persisting recovery journal.",
                        );
                        return Err(abort_with_report(error, report));
                    }
                }
            }
            Err(error) => {
                match op.family {
                    RoutePlanFamily::Ipv4 => log_net::interface_metric_set_failed_v4(&error),
                    RoutePlanFamily::Ipv6 => log_net::interface_metric_set_failed_v6(&error),
                }
                report.push_failed_kind(
                    RoutePlan::metric_item_id(op),
                    RouteApplyKind::Metric,
                    Some(RouteApplyFailureKind::System),
                    vec![format!(
                        "failed to set Windows {:?} interface metric {}: {error}",
                        op.family, op.metric
                    )],
                );
                skip_remaining_route_plan_ops(
                    &mut report,
                    route_plan,
                    index + 1,
                    0,
                    0,
                    "Windows apply aborted after interface metric configuration failed.",
                );
                skip_remaining_windows_stages(
                    &mut report,
                    false,
                    true,
                    true,
                    true,
                    "Windows apply aborted after interface metric configuration failed.",
                );
                return abort_with_cleanup(
                    state,
                    NetworkError::UnsafeRouting(format!(
                        "failed to set {:?} interface metric: {error}",
                        op.family
                    )),
                    report,
                )
                .await;
            }
        }
    }

    let mut bypass_routes = Vec::new();
    let mut endpoint_v4 = 0usize;
    let mut endpoint_v6 = 0usize;
    let mut bypass_v4 = 0usize;
    let mut bypass_v6 = 0usize;
    let mut resolved_bypass_ops = Vec::with_capacity(route_plan.bypass_ops.len());

    // 6) 先解析所有 endpoint bypass 目标，避免在中途留下半套路由。
    for (index, op) in route_plan.bypass_ops.iter().enumerate() {
        let endpoint_ips = match resolve_bypass_ips_for_op(op).await {
            Ok(endpoint_ips) => endpoint_ips,
            Err(error) => {
                for prior_op in route_plan.bypass_ops.iter().take(index) {
                    report.push_skipped_kind(
                        RoutePlan::bypass_item_id(prior_op),
                        RouteApplyKind::BypassRoute,
                        vec![String::from(
                            "Windows apply aborted before bypass routes could be applied.",
                        )],
                    );
                }
                report.push_failed_kind(
                    RoutePlan::bypass_item_id(op),
                    RouteApplyKind::BypassRoute,
                    Some(RouteApplyFailureKind::Lookup),
                    vec![format!(
                        "failed to resolve Windows endpoint bypass {}:{}: {error}",
                        op.host, op.port
                    )],
                );
                skip_remaining_route_plan_ops(
                    &mut report,
                    route_plan,
                    route_plan.metric_ops.len(),
                    index + 1,
                    0,
                    "Windows apply aborted while resolving endpoint bypass routes.",
                );
                skip_remaining_windows_stages(
                    &mut report,
                    false,
                    true,
                    true,
                    true,
                    "Windows apply aborted while resolving endpoint bypass routes.",
                );
                return Err(abort_with_report(error, report));
            }
        };
        resolved_bypass_ops.push((op, endpoint_ips));
    }

    // 7) 将解析结果映射到实际 system route，再统一下发。
    for &(op, ref endpoint_ips) in &resolved_bypass_ops {
        for ip in endpoint_ips {
            if ip.is_ipv4() {
                if !full_v4 {
                    continue;
                }
                endpoint_v4 += 1;
            } else {
                if !full_v6 {
                    continue;
                }
                endpoint_v6 += 1;
            }
            match best_route_to(*ip) {
                Ok(route) => {
                    log_net::bypass_route_add(route.dest, route.next_hop, route.if_index);
                    if ip.is_ipv4() {
                        bypass_v4 += 1;
                    } else {
                        bypass_v6 += 1;
                    }
                    bypass_routes.push(route);
                }
                Err(error) => log_net::bypass_route_failed(*ip, &error),
            }
        }
        report.push_applied_kind(
            RoutePlan::bypass_item_id(op),
            RouteApplyKind::BypassRoute,
            vec![format!(
                "Applied Windows endpoint bypass handling for {}:{}.",
                op.host, op.port
            )],
        );
    }

    // 8) 安全护栏：如果 endpoint 存在但 bypass 为 0，则拒绝继续。
    let missing_v4_bypass = endpoint_v4 > 0 && bypass_v4 == 0;
    let missing_v6_bypass = endpoint_v6 > 0 && bypass_v6 == 0;
    if missing_v4_bypass {
        log_net::skip_default_route_v4();
    }
    if missing_v6_bypass {
        log_net::skip_default_route_v6();
    }
    if missing_v4_bypass || missing_v6_bypass {
        let family = match (missing_v4_bypass, missing_v6_bypass) {
            (true, true) => "IPv4+IPv6",
            (true, false) => "IPv4",
            (false, true) => "IPv6",
            (false, false) => unreachable!(),
        };
        report.push_failed_kind(
            "apply:endpoint_bypass_guard",
            RouteApplyKind::BypassRoute,
            Some(RouteApplyFailureKind::Verification),
            vec![format!(
                "full-tunnel {family} endpoint bypass route missing; refusing to continue to avoid traffic/DNS leak"
            )],
        );
        skip_remaining_windows_stages(
            &mut report,
            false,
            true,
            true,
            true,
            "Windows apply aborted because endpoint bypass safety checks failed.",
        );
        return abort_with_cleanup(
            state,
            NetworkError::UnsafeRouting(format!(
                "full-tunnel {family} endpoint bypass route missing; refusing to continue to avoid traffic/DNS leak"
            )),
            report,
        )
        .await;
    }

    // 9) 按共享 route plan 下发所有 route ops。
    for (index, op) in route_plan.route_ops.iter().enumerate() {
        let entry = RouteEntry {
            dest: op.route.addr,
            prefix: op.route.cidr,
            next_hop: None,
            if_index: adapter.if_index,
            luid: adapter.luid,
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
            report.push_failed_kind(
                RoutePlan::route_item_id(op),
                RouteApplyKind::Route,
                Some(RouteApplyFailureKind::System),
                vec![format!(
                    "failed to add Windows route {}/{}: {error}",
                    op.route.addr, op.route.cidr
                )],
            );
            skip_remaining_route_plan_ops(
                &mut report,
                route_plan,
                route_plan.metric_ops.len(),
                route_plan.bypass_ops.len(),
                index + 1,
                "Windows apply aborted while adding route entries.",
            );
            skip_remaining_windows_stages(
                &mut report,
                false,
                true,
                true,
                true,
                "Windows apply aborted while adding route entries.",
            );
            return abort_with_cleanup(state, error, report).await;
        }
        report.push_applied_kind(
            RoutePlan::route_item_id(op),
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
        if let Some(recovery) = state.recovery.as_mut() {
            if let Err(error) = recovery.record_route(&entry) {
                report.push_failed_kind(
                    "apply:recovery",
                    RouteApplyKind::RecoveryJournal,
                    Some(RouteApplyFailureKind::Persistence),
                    vec![format!(
                        "failed to persist Windows recovery journal after adding route {}/{}: {error}",
                        op.route.addr, op.route.cidr
                    )],
                );
                skip_remaining_route_plan_ops(
                    &mut report,
                    route_plan,
                    route_plan.metric_ops.len(),
                    route_plan.bypass_ops.len(),
                    index + 1,
                    "Windows apply aborted while persisting recovery journal.",
                );
                skip_remaining_windows_stages(
                    &mut report,
                    false,
                    true,
                    true,
                    true,
                    "Windows apply aborted while persisting recovery journal.",
                );
                return Err(abort_with_report(error, report));
            }
        }
        state.routes.push(entry);
    }

    // 10) 应用 DNS。
    if !config.interface.dns_servers.is_empty() || !config.interface.dns_search.is_empty() {
        log_dns::apply_summary(
            config.interface.dns_servers.len(),
            config.interface.dns_search.len(),
        );
        match apply_dns(
            adapter,
            &config.interface.dns_servers,
            &config.interface.dns_search,
        ) {
            Ok(dns_state) => {
                report.push_applied_kind(
                    "apply:dns",
                    RouteApplyKind::Dns,
                    vec![format!(
                        "Applied Windows DNS settings (servers={}, search={}).",
                        config.interface.dns_servers.len(),
                        config.interface.dns_search.len()
                    )],
                );
                if let Some(recovery) = state.recovery.as_mut() {
                    if let Err(error) = recovery.record_dns(&dns_state) {
                        report.push_failed_kind(
                            "apply:recovery",
                            RouteApplyKind::RecoveryJournal,
                            Some(RouteApplyFailureKind::Persistence),
                            vec![format!(
                                "failed to persist Windows recovery journal after applying DNS: {error}"
                            )],
                        );
                        skip_remaining_windows_stages(
                            &mut report,
                            false,
                            false,
                            true,
                            true,
                            "Windows apply aborted while persisting recovery journal.",
                        );
                        return Err(abort_with_report(error, report));
                    }
                }
                state.dns = Some(dns_state);
            }
            Err(error) => {
                report.push_failed_kind(
                    "apply:dns",
                    RouteApplyKind::Dns,
                    Some(RouteApplyFailureKind::System),
                    vec![format!("failed to apply Windows DNS settings: {error}")],
                );
                skip_remaining_windows_stages(
                    &mut report,
                    false,
                    false,
                    true,
                    true,
                    "Windows apply aborted after DNS configuration failed.",
                );
                log_dns::apply_failed(&error);
                return abort_with_cleanup(state, error, report).await;
            }
        }
    }

    // 11) 全隧道 + DNS 场景下，启用 NRPT 与 DNS Guard。
    if !config.interface.dns_servers.is_empty()
        && config.interface.table != Some(RouteTable::Off)
        && (full_v4 || full_v6)
    {
        match apply_nrpt_guard(adapter, &config.interface.dns_servers) {
            Ok(nrpt_state) => {
                report.push_applied_kind(
                    "apply:nrpt",
                    RouteApplyKind::Nrpt,
                    vec![format!(
                        "Applied Windows NRPT guard for {} tunnel DNS server(s).",
                        config.interface.dns_servers.len()
                    )],
                );
                if let (Some(recovery), Some(nrpt_state)) =
                    (state.recovery.as_mut(), nrpt_state.as_ref())
                {
                    if let Err(error) = recovery.record_nrpt(nrpt_state) {
                        report.push_failed_kind(
                            "apply:recovery",
                            RouteApplyKind::RecoveryJournal,
                            Some(RouteApplyFailureKind::Persistence),
                            vec![format!(
                                "failed to persist Windows recovery journal after applying NRPT: {error}"
                            )],
                        );
                        skip_remaining_windows_stages(
                            &mut report,
                            false,
                            false,
                            false,
                            true,
                            "Windows apply aborted while persisting recovery journal.",
                        );
                        return Err(abort_with_report(error, report));
                    }
                }
                state.nrpt = nrpt_state;
            }
            Err(error) => {
                report.push_failed_kind(
                    "apply:nrpt",
                    RouteApplyKind::Nrpt,
                    Some(RouteApplyFailureKind::System),
                    vec![format!("failed to apply Windows NRPT guard: {error}")],
                );
                skip_remaining_windows_stages(
                    &mut report,
                    false,
                    false,
                    false,
                    true,
                    "Windows apply aborted after NRPT configuration failed.",
                );
                return abort_with_cleanup(state, error, report).await;
            }
        }

        match apply_dns_guard(adapter, full_v4, full_v6, &config.interface.dns_servers) {
            Ok(guard_state) => {
                report.push_applied_kind(
                    "apply:dns_guard",
                    RouteApplyKind::DnsGuard,
                    vec![format!(
                        "Applied Windows DNS guard for {} tunnel DNS server(s).",
                        config.interface.dns_servers.len()
                    )],
                );
                if let (Some(recovery), Some(guard_state)) =
                    (state.recovery.as_mut(), guard_state.as_ref())
                {
                    if let Err(error) = recovery.record_dns_guard(guard_state) {
                        report.push_failed_kind(
                            "apply:recovery",
                            RouteApplyKind::RecoveryJournal,
                            Some(RouteApplyFailureKind::Persistence),
                            vec![format!(
                                "failed to persist Windows recovery journal after applying DNS guard: {error}"
                            )],
                        );
                        return Err(abort_with_report(error, report));
                    }
                }
                state.dns_guard = guard_state;
            }
            Err(error) => {
                report.push_failed_kind(
                    "apply:dns_guard",
                    RouteApplyKind::DnsGuard,
                    Some(RouteApplyFailureKind::System),
                    vec![format!("failed to apply Windows DNS guard: {error}")],
                );
                return abort_with_cleanup(state, error, report).await;
            }
        }
    }

    if let Some(recovery) = state.recovery.as_mut() {
        if let Err(error) = recovery.mark_running() {
            report.push_failed_kind(
                "apply:recovery",
                RouteApplyKind::RecoveryJournal,
                Some(RouteApplyFailureKind::Persistence),
                vec![format!(
                    "failed to mark Windows recovery journal running: {error}"
                )],
            );
            return Err(abort_with_report(error, report));
        }
    }

    finish_with_report(state, report)
}

/// 回滚 Windows 网络配置。
///
/// 顺序：bypass route -> 普通 route -> 地址 -> metric -> DNS/NRPT/guard。
pub async fn cleanup_network_config(state: AppliedNetworkState) -> Result<(), NetworkError> {
    let AppliedNetworkState {
        tun_name,
        adapter,
        addresses,
        routes,
        bypass_routes,
        iface_metrics,
        dns,
        nrpt,
        dns_guard,
        mut recovery,
    } = state;

    if let Some(guard) = recovery.as_mut() {
        let _ = guard.mark_stopping();
    }

    log_net::cleanup_windows(
        &tun_name,
        addresses.len(),
        routes.len(),
        bypass_routes.len(),
    );

    for entry in bypass_routes.iter().rev() {
        if let Err(err) = delete_route(entry) {
            log_net::bypass_route_del_failed(&err);
        }
    }

    for entry in routes.iter().rev() {
        if let Err(err) = delete_route(entry) {
            log_net::route_del_failed(&err);
        }
    }

    for address in &addresses {
        if let Err(err) = delete_unicast_address(adapter, address) {
            log_net::address_del_failed(&err);
        }
    }

    for iface in iface_metrics.iter().rev() {
        if let Err(err) = restore_interface_metric(adapter, *iface) {
            log_net::interface_metric_restore_failed(&err);
        }
    }

    if let Some(dns) = dns {
        log_dns::revert_start();
        if let Err(err) = cleanup_dns(dns) {
            log_dns::revert_failed(&err);
        }
    }

    if let Some(nrpt) = nrpt {
        if let Err(err) = cleanup_nrpt_guard(nrpt) {
            log_net::nrpt_cleanup_failed(&err);
        }
    }

    if let Some(guard) = dns_guard {
        if let Err(err) = cleanup_dns_guard(guard) {
            log_net::dns_guard_cleanup_failed(&err);
        }
    }

    if let Some(guard) = recovery {
        guard.clear()?;
    }
    clear_persisted_apply_report()?;

    Ok(())
}

/// Windows service 启动期修复占位。
///
/// Phase 1 先切换 owner/transport，后续阶段在这里接入 durable journal。
pub fn attempt_startup_repair() -> Result<(), NetworkError> {
    recovery::attempt_startup_repair()
}

pub fn load_persisted_apply_report() -> Option<RouteApplyReport> {
    load_persisted_apply_report_from_disk()
        .ok()
        .flatten()
        .map(|mut report| {
            report.mark_persisted();
            report
        })
}

/// 将 Windows 宽字符串指针转换为 Rust String。
fn pwstr_to_string(ptr: PWSTR) -> String {
    if ptr.0.is_null() {
        return String::new();
    }
    unsafe {
        let mut len = 0usize;
        let mut cursor = ptr.0;
        while *cursor != 0 {
            len += 1;
            cursor = cursor.add(1);
        }
        let slice = std::slice::from_raw_parts(ptr.0, len);
        String::from_utf16_lossy(slice)
    }
}

/// 判断 Win32 错误是否属于“已存在”。
fn is_already_exists(code: WIN32_ERROR) -> bool {
    code == ERROR_OBJECT_ALREADY_EXISTS || code == ERROR_ALREADY_EXISTS
}
