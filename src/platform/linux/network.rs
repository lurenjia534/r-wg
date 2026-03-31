//! Linux 网络配置入口模块。
//!
//! 负责地址/路由/DNS/policy routing 的系统配置，
//! WireGuard 设备本身由 gotatun 创建与维护。

mod dns;
mod logging;
mod netlink;
mod policy;
mod recovery;
mod routes;

use rtnetlink::{LinkMessageBuilder, LinkUnspec};

use crate::core::config::{InterfaceAddress, RouteTable, WireGuardConfig};
use crate::core::route_plan::{
    RouteApplyFailureKind, RouteApplyKind, RouteApplyReport, RoutePlan, RoutePlanFamily,
    RoutePlanPlatform, RoutePlanRouteKind, RoutePlanRouteOp,
};
use crate::platform::{NetworkApplyError, NetworkApplyResult};

use dns::{apply_dns, cleanup_dns, DnsState};
use logging::{log_default_routes, log_privileges};
use netlink::{build_route_message, delete_address, delete_route, link_index, netlink_handle};
use policy::{
    apply_policy_rules, cleanup_policy_rules_once, cleanup_stale_default_routes_once,
    PolicyRoutingState,
};
use recovery::{
    attempt_startup_repair_sync, cleanup_policy_state, clear_persisted_apply_report,
    clear_recovery_journal, load_persisted_apply_report as load_persisted_apply_report_from_disk,
    write_applying_journal, write_persisted_apply_report, write_running_journal,
};

use crate::log::events::{dns as log_dns, net as log_net};

// 默认 policy routing 表号：不干扰 main table，且便于排查。
const DEFAULT_POLICY_TABLE: u32 = 200;

#[derive(Debug)]
pub struct AppliedNetworkState {
    tun_name: String,
    addresses: Vec<InterfaceAddress>,
    routes: Vec<RoutePlanRouteOp>,
    table: Option<RouteTable>,
    dns: Option<DnsState>,
    policy: Option<PolicyRoutingState>,
}

#[derive(Debug)]
pub enum NetworkError {
    Io(std::io::Error),
    Netlink(rtnetlink::Error),
    CommandFailed {
        command: String,
        status: Option<i32>,
        stderr: String,
    },
    DnsVerifyFailed(String),
    DnsNotSupported,
    LinkNotFound(String),
    MissingFwmark,
}

impl std::fmt::Display for NetworkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetworkError::Io(err) => write!(f, "io error: {err}"),
            NetworkError::CommandFailed {
                command,
                status,
                stderr,
            } => write!(f, "command failed: {command} (status={status:?}) {stderr}"),
            NetworkError::DnsVerifyFailed(message) => {
                write!(f, "dns verification failed: {message}")
            }
            NetworkError::DnsNotSupported => write!(f, "no supported DNS backend found"),
            NetworkError::Netlink(err) => write!(f, "netlink error: {err}"),
            NetworkError::LinkNotFound(name) => write!(f, "link not found: {name}"),
            NetworkError::MissingFwmark => write!(f, "missing fwmark for policy routing"),
        }
    }
}

impl std::error::Error for NetworkError {}

impl From<std::io::Error> for NetworkError {
    fn from(err: std::io::Error) -> Self {
        NetworkError::Io(err)
    }
}

impl From<rtnetlink::Error> for NetworkError {
    fn from(err: rtnetlink::Error) -> Self {
        NetworkError::Netlink(err)
    }
}

/// 应用 Linux 网络配置。
///
/// 只负责系统地址/路由/DNS，WireGuard 隧道本身由 gotatun 负责。
pub async fn apply_network_config(
    tun_name: &str,
    config: &WireGuardConfig,
    route_plan: &RoutePlan,
) -> Result<NetworkApplyResult, NetworkApplyError> {
    debug_assert_eq!(route_plan.platform, RoutePlanPlatform::Linux);

    let interface = &config.interface;
    log_net::apply_linux(
        tun_name,
        interface.mtu,
        interface.addresses.len(),
        interface.table,
        interface.dns_servers.len(),
        interface.dns_search.len(),
    );
    log_privileges();

    let route_ops: Vec<RoutePlanRouteOp> = route_plan
        .route_ops
        .iter()
        .filter(|route_op| matches!(route_op.kind, RoutePlanRouteKind::Allowed))
        .cloned()
        .collect();
    let policy_rule_ops = route_plan.policy_rule_ops.clone();
    let mut report = RouteApplyReport::new(RoutePlanPlatform::Linux);
    let policy = if interface.table != Some(RouteTable::Off) && route_plan.full_tunnel.any() {
        let table_id = route_plan
            .linux_policy_table_id
            .unwrap_or(match interface.table {
                Some(RouteTable::Id(value)) => value,
                _ => DEFAULT_POLICY_TABLE,
            });
        let fwmark = match interface.fwmark {
            Some(value) => value,
            None => {
                report.push_failed_kind(
                    "apply:linux:fwmark",
                    RouteApplyKind::PolicyRule,
                    Some(RouteApplyFailureKind::Precondition),
                    vec![
                        "Full tunnel policy routing requires a fwmark before Linux routes can be applied."
                            .to_string(),
                    ],
                );
                report.mark_failed();
                return Err(NetworkApplyError {
                    error: NetworkError::MissingFwmark,
                    report: report.clone(),
                });
            }
        };
        Some(PolicyRoutingState {
            table_id,
            fwmark,
            v4: route_plan.full_tunnel.ipv4,
            v6: route_plan.full_tunnel.ipv6,
        })
    } else {
        None
    };

    let mut state = AppliedNetworkState {
        tun_name: tun_name.to_string(),
        addresses: interface.addresses.clone(),
        routes: route_ops.clone(),
        table: interface.table,
        dns: None,
        policy,
    };

    let netlink = netlink_handle().map_err(|error| {
        report.push_failed_kind(
            "apply:linux:netlink",
            RouteApplyKind::Adapter,
            Some(RouteApplyFailureKind::System),
            vec![format!(
                "Failed to open Linux netlink handle before route apply: {error}"
            )],
        );
        report.mark_failed();
        NetworkApplyError {
            error,
            report: report.clone(),
        }
    })?;
    let handle = netlink.handle();
    persist_applying_recovery_state(tun_name, route_plan, state.policy.as_ref(), &report).map_err(
        |error| {
            report.push_failed_kind(
                "apply:linux:journal",
                RouteApplyKind::RecoveryJournal,
                Some(RouteApplyFailureKind::Persistence),
                vec![format!(
                    "Failed to persist Linux apply journal before any route operation: {error}"
                )],
            );
            report.mark_failed();
            NetworkApplyError {
                error,
                report: report.clone(),
            }
        },
    )?;

    let result: Result<NetworkApplyResult, NetworkError> = async {
        // 建立 netlink 连接并查询接口索引，后续所有操作都基于 ifindex。
        let link_index = link_index(handle, tun_name).await?;
        log_net::link_index(link_index);
        // 清理历史遗留的 TUN 默认路由，避免新隧道误复用旧出口。
        if let Err(err) = cleanup_stale_default_routes_once(handle, tun_name, link_index).await {
            log_net::stale_default_route_cleanup_failed(&err);
        }

        // 设置 MTU 与 up 状态，确保隧道可用。
        if let Some(mtu) = interface.mtu {
            let message = LinkMessageBuilder::<LinkUnspec>::default()
                .index(link_index)
                .mtu(mtu.into())
                .build();
            handle.link().set(message).execute().await?;
        }

        let message = LinkMessageBuilder::<LinkUnspec>::default()
            .index(link_index)
            .up()
            .build();
        handle.link().set(message).execute().await?;

        // 写入接口地址（IPv4/IPv6）。
        for address in &interface.addresses {
            log_net::address_add(address.addr, address.cidr);
            handle
                .address()
                .add(link_index, address.addr, address.cidr)
                .execute()
                .await?;
        }

        // 根据策略路由决策把路由写入 main 或自定义表。
        if let Some(policy_state) = state.policy.as_ref() {
            if let Err(err) = cleanup_policy_rules_once(handle, Some(policy_state)).await {
                log_net::stale_policy_rule_cleanup_failed(&err);
            }
            let fwmark = policy_state.fwmark;
            let table_id = policy_state.table_id;

            for (idx, policy_op) in policy_rule_ops.iter().enumerate() {
                let applied = match policy_op.family {
                    RoutePlanFamily::Ipv4 => {
                        apply_policy_rules(handle, fwmark, table_id, true, false).await
                    }
                    RoutePlanFamily::Ipv6 => {
                        apply_policy_rules(handle, fwmark, table_id, false, true).await
                    }
                };

                if let Err(err) = applied {
                    report.push_failed_kind(
                        RoutePlan::policy_item_id(policy_op),
                        RouteApplyKind::PolicyRule,
                        Some(RouteApplyFailureKind::System),
                        vec![format!(
                            "Failed to apply Linux policy rule for {} via table {} with fwmark 0x{:x}: {err}",
                            policy_op.family.label(),
                            policy_op.table_id,
                            policy_op.fwmark
                        )],
                    );
                    for skipped_op in policy_rule_ops.iter().skip(idx + 1) {
                        report.push_skipped_kind(
                            RoutePlan::policy_item_id(skipped_op),
                            RouteApplyKind::PolicyRule,
                            vec![format!(
                                "Skipped because policy apply aborted after {} failed.",
                                policy_op.family.label()
                            )],
                        );
                    }
                    for skipped_route in &route_ops {
                        report.push_skipped_kind(
                            RoutePlan::route_item_id(skipped_route),
                            RouteApplyKind::Route,
                            vec![format!(
                                "Skipped because policy apply aborted before route installation for {}/{}.",
                                skipped_route.route.addr, skipped_route.route.cidr
                            )],
                        );
                    }
                    report.mark_failed();
                    return abort_apply(
                        tun_name,
                        route_plan,
                        state,
                        &report,
                        err,
                    )
                    .await;
                }

                report.push_applied_kind(
                    RoutePlan::policy_item_id(policy_op),
                    RouteApplyKind::PolicyRule,
                    vec![format!(
                        "Applied Linux policy rule for {} via table {} with fwmark 0x{:x}.",
                        policy_op.family.label(),
                        policy_op.table_id,
                        policy_op.fwmark
                    )],
                );
                if let Err(err) =
                    persist_applying_recovery_state(
                        tun_name,
                        route_plan,
                        state.policy.as_ref(),
                        &report,
                    )
                {
                    report.push_failed_kind(
                        "apply:linux:journal",
                        RouteApplyKind::RecoveryJournal,
                        Some(RouteApplyFailureKind::Persistence),
                        vec![format!(
                            "Failed to persist Linux apply journal after policy rule {}: {err}",
                            policy_op.family.label()
                        )],
                    );
                    report.mark_failed();
                    return abort_apply(tun_name, route_plan, state, &report, err).await;
                }
            }
        }

        // 根据策略路由决策把路由写入 main 或自定义表。
        if interface.table != Some(RouteTable::Off) {
            for (idx, route_op) in route_ops.iter().enumerate() {
                let table = route_op.table_id;
                log_net::route_add(route_op.route.addr, route_op.route.cidr, table);
                let message = build_route_message(link_index, &route_op.route, table);
                if let Err(err) = handle.route().add(message).execute().await {
                    report.push_failed_kind(
                        RoutePlan::route_item_id(route_op),
                        RouteApplyKind::Route,
                        Some(RouteApplyFailureKind::System),
                        vec![format!(
                            "Failed to apply route {}/{} to {} on Linux: {err}",
                            route_op.route.addr,
                            route_op.route.cidr,
                            route_op
                                .table_id
                                .map(|id| format!("table {id}"))
                                .unwrap_or_else(|| "main".to_string())
                        )],
                    );
                    for skipped_route in route_ops.iter().skip(idx + 1) {
                        report.push_skipped_kind(
                            RoutePlan::route_item_id(skipped_route),
                            RouteApplyKind::Route,
                            vec![format!(
                                "Skipped because route apply aborted after {}/{} failed.",
                                route_op.route.addr, route_op.route.cidr
                            )],
                        );
                    }
                    report.mark_failed();
                    return abort_apply(
                        tun_name,
                        route_plan,
                        state,
                        &report,
                        err.into(),
                    )
                    .await;
                }
                report.push_applied_kind(
                    RoutePlan::route_item_id(route_op),
                    RouteApplyKind::Route,
                    vec![format!(
                        "Applied route {}/{} to {} on Linux.",
                        route_op.route.addr,
                        route_op.route.cidr,
                        route_op
                            .table_id
                            .map(|id| format!("table {id}"))
                            .unwrap_or_else(|| "main".to_string())
                    )],
                );
                if let Err(err) =
                    persist_applying_recovery_state(
                        tun_name,
                        route_plan,
                        state.policy.as_ref(),
                        &report,
                    )
                {
                    report.push_failed_kind(
                        "apply:linux:journal",
                        RouteApplyKind::RecoveryJournal,
                        Some(RouteApplyFailureKind::Persistence),
                        vec![format!(
                            "Failed to persist Linux apply journal after route {}/{}: {err}",
                            route_op.route.addr, route_op.route.cidr
                        )],
                    );
                    report.mark_failed();
                    return abort_apply(tun_name, route_plan, state, &report, err).await;
                }
            }
        }

        // 输出当前默认路由，便于诊断主表/策略表走向。
        log_default_routes();

        // DNS 失败视为致命错误，避免全隧道场景出现 DNS 泄漏。
        if !interface.dns_servers.is_empty() || !interface.dns_search.is_empty() {
            log_dns::apply_summary(interface.dns_servers.len(), interface.dns_search.len());
            match apply_dns(tun_name, &interface.dns_servers, &interface.dns_search).await {
                Ok(dns_state) => {
                    state.dns = Some(dns_state);
                    report.push_applied_kind(
                        "apply:dns",
                        RouteApplyKind::Dns,
                        vec![format!(
                            "Applied Linux tunnel DNS with {} server(s) and {} search domain(s).",
                            interface.dns_servers.len(),
                            interface.dns_search.len()
                        )],
                    );
                }
                Err(err) => {
                    log_dns::apply_failed(&err);
                    report.push_failed_kind(
                        "apply:dns",
                        RouteApplyKind::Dns,
                        Some(classify_linux_failure(&err)),
                        vec![format!("Failed to apply Linux tunnel DNS: {err}")],
                    );
                    report.mark_failed();
                    return abort_apply(tun_name, route_plan, state, &report, err).await;
                }
            }
        }

        report.mark_running();
        if let Err(err) = persist_running_recovery_state(
            tun_name,
            &route_ops,
            state.policy.as_ref(),
            state.dns.as_ref(),
            &report,
        )
        {
            report.push_failed_kind(
                "apply:linux:journal",
                RouteApplyKind::RecoveryJournal,
                Some(RouteApplyFailureKind::Persistence),
                vec![format!("Failed to persist Linux running journal: {err}")],
            );
            report.mark_failed();
            return abort_apply(tun_name, route_plan, state, &report, err).await;
        }

        Ok(NetworkApplyResult {
            state,
            report: report.clone(),
        })
    }
    .await;

    netlink.shutdown().await;
    result.map_err(|error| NetworkApplyError { error, report })
}

async fn abort_apply(
    tun_name: &str,
    route_plan: &RoutePlan,
    state: AppliedNetworkState,
    report: &RouteApplyReport,
    error: NetworkError,
) -> Result<NetworkApplyResult, NetworkError> {
    let _ = persist_applying_recovery_state(tun_name, route_plan, state.policy.as_ref(), report);
    let _ = cleanup_network_config_impl(state, false).await;
    Err(error)
}

fn persist_applying_recovery_state(
    tun_name: &str,
    route_plan: &RoutePlan,
    policy: Option<&PolicyRoutingState>,
    report: &RouteApplyReport,
) -> Result<(), NetworkError> {
    write_persisted_apply_report(report)?;
    write_applying_journal(tun_name, route_plan, policy)
}

fn persist_running_recovery_state(
    tun_name: &str,
    route_ops: &[RoutePlanRouteOp],
    policy: Option<&PolicyRoutingState>,
    dns: Option<&DnsState>,
    report: &RouteApplyReport,
) -> Result<(), NetworkError> {
    write_persisted_apply_report(report)?;
    write_running_journal(tun_name, route_ops, policy, dns)
}

/// 清理之前应用的网络配置。
pub async fn cleanup_network_config(state: AppliedNetworkState) -> Result<(), NetworkError> {
    cleanup_network_config_impl(state, true).await
}

async fn cleanup_network_config_impl(
    state: AppliedNetworkState,
    clear_journal: bool,
) -> Result<(), NetworkError> {
    log_net::cleanup_linux(
        &state.tun_name,
        state.addresses.len(),
        state.routes.len(),
        state.table,
        state.dns.is_some(),
    );
    // 清理阶段同样需要 netlink handle。
    let netlink = netlink_handle()?;
    let handle = netlink.handle();

    let result = async {
        let link_index = match link_index(handle, &state.tun_name).await {
            Ok(index) => index,
            Err(err) => {
                log_net::link_lookup_failed(&err);
                return Ok(());
            }
        };

        // 先删除接口地址，避免残留地址影响后续路由决策。
        for address in &state.addresses {
            log_net::address_del(address.addr, address.cidr);
            if let Err(err) = delete_address(handle, link_index, address).await {
                log_net::address_del_failed(&err);
            }
        }

        // 删除该隧道添加的路由（包含策略表或主表路由）。
        if state.table != Some(RouteTable::Off) {
            for route_op in &state.routes {
                if !matches!(route_op.kind, RoutePlanRouteKind::Allowed) {
                    continue;
                }
                let table = route_op.table_id;
                log_net::route_del(route_op.route.addr, route_op.route.cidr, table);
                if let Err(err) = delete_route(handle, link_index, &route_op.route, table).await {
                    log_net::route_del_failed(&err);
                }
            }
        }

        // 按记录的 DNS 状态回滚。
        let dns_cleanup_result = if let Some(dns) = state.dns {
            log_dns::revert_start();
            cleanup_dns(state.tun_name.as_str(), dns).await
        } else {
            Ok(())
        };

        // 清理 policy rule，恢复系统默认路由策略。
        if let Some(policy) = state.policy.as_ref() {
            if let Err(err) = cleanup_policy_state(handle, policy).await {
                log_net::policy_rule_cleanup_failed(&err);
            }
        }

        dns_cleanup_result
    }
    .await;

    netlink.shutdown().await;
    result?;
    if clear_journal {
        clear_recovery_journal()?;
        clear_persisted_apply_report()?;
    }
    Ok(())
}

pub fn attempt_startup_repair() -> Result<(), NetworkError> {
    attempt_startup_repair_sync()
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

fn classify_linux_failure(error: &NetworkError) -> RouteApplyFailureKind {
    match error {
        NetworkError::MissingFwmark => RouteApplyFailureKind::Precondition,
        NetworkError::DnsVerifyFailed(_) => RouteApplyFailureKind::Verification,
        NetworkError::CommandFailed { .. } | NetworkError::DnsNotSupported => {
            RouteApplyFailureKind::System
        }
        NetworkError::Io(_) => RouteApplyFailureKind::Persistence,
        NetworkError::Netlink(_) | NetworkError::LinkNotFound(_) => RouteApplyFailureKind::Lookup,
    }
}
