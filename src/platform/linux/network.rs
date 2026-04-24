//! Linux 网络配置入口模块
//!
//! 本模块负责在 Linux 系统上应用 WireGuard 隧道所需的网络配置。
//!
//! # 配置内容
//!
//! - **接口地址**: 为 TUN 设备分配 IP 地址
//! - **路由**: 添加AllowedIPs路由，决定哪些流量经过隧道
//! - **DNS**: 配置隧道内的 DNS 服务器和搜索域
//! - **策略路由**: 全隧道模式下使用 fwmark + 策略路由强制所有流量走隧道
//!
//! # 恢复机制
//!
//! 崩溃恢复：应用配置前会写入 journal，崩溃后可自动回滚或修复。
//!
//! # 错误处理
//!
//! 配置失败会立即回滚所有已应用的更改，确保系统状态不会停留在不一致状态。

mod dns;
mod killswitch;
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
use killswitch::{apply_kill_switch, KillSwitchState};
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
    kill_switch: Option<KillSwitchState>,
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
    KillSwitchUnavailable(String),
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
            NetworkError::KillSwitchUnavailable(message) => {
                write!(f, "kill switch unavailable: {message}")
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
    kill_switch_enabled: bool,
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
        kill_switch: None,
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
    persist_applying_recovery_state(
        tun_name,
        route_plan,
        state.policy.as_ref(),
        state.kill_switch.as_ref(),
        &report,
    )
    .map_err(|error| {
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
    })?;

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
                        state.kill_switch.as_ref(),
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
                        state.kill_switch.as_ref(),
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

        if let Some((kill_switch_fwmark, kill_switch_v4, kill_switch_v6)) = state
            .policy
            .as_ref()
            .map(|policy_state| (policy_state.fwmark, policy_state.v4, policy_state.v6))
        {
            let kill_switch_item_ids = [
                (kill_switch_v4, "apply:linux:kill_switch_ipv4", "IPv4"),
                (kill_switch_v6, "apply:linux:kill_switch_ipv6", "IPv6"),
            ];

            if kill_switch_enabled {
                match apply_kill_switch(tun_name, kill_switch_fwmark, kill_switch_v4, kill_switch_v6)
                    .await
                {
                    Ok(kill_switch_state) => {
                        for (enabled, item_id, family_label) in kill_switch_item_ids {
                            if !enabled {
                                continue;
                            }
                            report.push_applied_kind(
                                item_id,
                                RouteApplyKind::KillSwitch,
                                vec![format!(
                                    "Applied Linux kill switch for {family_label} traffic using fwmark 0x{:x}.",
                                    kill_switch_fwmark
                                )],
                            );
                        }
                        state.kill_switch = Some(kill_switch_state);
                        if let Err(err) =
                            persist_applying_recovery_state(
                                tun_name,
                                route_plan,
                                state.policy.as_ref(),
                                state.kill_switch.as_ref(),
                                &report,
                            )
                        {
                            report.push_failed_kind(
                                "apply:linux:journal",
                                RouteApplyKind::RecoveryJournal,
                                Some(RouteApplyFailureKind::Persistence),
                                vec![format!(
                                    "Failed to persist Linux apply journal after kill switch setup: {err}"
                                )],
                            );
                            report.mark_failed();
                            return abort_apply(tun_name, route_plan, state, &report, err).await;
                        }
                    }
                    Err(err) => {
                        for (enabled, item_id, family_label) in kill_switch_item_ids {
                            if !enabled {
                                continue;
                            }
                            report.push_failed_kind(
                                item_id,
                                RouteApplyKind::KillSwitch,
                                Some(RouteApplyFailureKind::System),
                                vec![format!(
                                    "Failed to apply Linux kill switch for {family_label} traffic with fwmark 0x{:x}: {err}",
                                    kill_switch_fwmark
                                )],
                            );
                        }
                        report.mark_failed();
                        return abort_apply(tun_name, route_plan, state, &report, err).await;
                    }
                }
            } else {
                for (enabled, item_id, family_label) in kill_switch_item_ids {
                    if !enabled {
                        continue;
                    }
                    report.push_skipped_kind(
                        item_id,
                        RouteApplyKind::KillSwitch,
                        vec![format!(
                            "Linux kill switch is disabled in Settings, so {family_label} traffic is not blocked outside the tunnel."
                        )],
                    );
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
            state.kill_switch.as_ref(),
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
    let _ = persist_applying_recovery_state(
        tun_name,
        route_plan,
        state.policy.as_ref(),
        state.kill_switch.as_ref(),
        report,
    );
    let _ = cleanup_network_config_impl(state, false).await;
    Err(error)
}

fn persist_applying_recovery_state(
    tun_name: &str,
    route_plan: &RoutePlan,
    policy: Option<&PolicyRoutingState>,
    kill_switch: Option<&KillSwitchState>,
    report: &RouteApplyReport,
) -> Result<(), NetworkError> {
    write_persisted_apply_report(report)?;
    write_applying_journal(tun_name, route_plan, policy, kill_switch)
}

fn persist_running_recovery_state(
    tun_name: &str,
    route_ops: &[RoutePlanRouteOp],
    policy: Option<&PolicyRoutingState>,
    kill_switch: Option<&KillSwitchState>,
    dns: Option<&DnsState>,
    report: &RouteApplyReport,
) -> Result<(), NetworkError> {
    write_persisted_apply_report(report)?;
    write_running_journal(tun_name, route_ops, policy, kill_switch, dns)
}

/// 清理之前应用的网络配置。
pub async fn cleanup_network_config(state: AppliedNetworkState) -> Result<(), NetworkError> {
    cleanup_network_config_impl(state, true).await
}

async fn cleanup_network_config_impl(
    state: AppliedNetworkState,
    clear_journal: bool,
) -> Result<(), NetworkError> {
    let AppliedNetworkState {
        tun_name,
        addresses,
        routes,
        table,
        dns,
        policy,
        kill_switch,
    } = state;

    log_net::cleanup_linux(
        &tun_name,
        addresses.len(),
        routes.len(),
        table,
        dns.is_some(),
    );
    let result = match netlink_handle() {
        Ok(netlink) => {
            let handle = netlink.handle();
            let result = async {
                let link_index = match link_index(handle, &tun_name).await {
                    Ok(index) => index,
                    Err(err) => {
                        log_net::link_lookup_failed(&err);
                        return Ok(());
                    }
                };

                // 先删除接口地址，避免残留地址影响后续路由决策。
                for address in &addresses {
                    log_net::address_del(address.addr, address.cidr);
                    if let Err(err) = delete_address(handle, link_index, address).await {
                        log_net::address_del_failed(&err);
                    }
                }

                // 删除该隧道添加的路由（包含策略表或主表路由）。
                if table != Some(RouteTable::Off) {
                    for route_op in &routes {
                        if !matches!(route_op.kind, RoutePlanRouteKind::Allowed) {
                            continue;
                        }
                        let table = route_op.table_id;
                        log_net::route_del(route_op.route.addr, route_op.route.cidr, table);
                        if let Err(err) =
                            delete_route(handle, link_index, &route_op.route, table).await
                        {
                            log_net::route_del_failed(&err);
                        }
                    }
                }

                // 按记录的 DNS 状态回滚。
                let dns_cleanup_result = if let Some(dns) = dns {
                    log_dns::revert_start();
                    cleanup_dns(tun_name.as_str(), dns).await
                } else {
                    Ok(())
                };

                // 清理 policy rule，恢复系统默认路由策略。
                if let Some(policy) = policy.as_ref() {
                    if let Err(err) = cleanup_policy_state(handle, policy).await {
                        log_net::policy_rule_cleanup_failed(&err);
                    }
                }

                dns_cleanup_result
            }
            .await;

            netlink.shutdown().await;
            result
        }
        Err(err) => Err(err),
    };

    let kill_switch_result = if let Some(kill_switch) = kill_switch {
        kill_switch.cleanup().await.map_err(|err| {
            log_net::kill_switch_cleanup_failed(&err);
            err
        })
    } else {
        Ok(())
    };

    merge_cleanup_results(result, kill_switch_result)?;
    if clear_journal {
        clear_recovery_journal()?;
        clear_persisted_apply_report()?;
    }
    Ok(())
}

fn merge_cleanup_results(
    link_dependent_result: Result<(), NetworkError>,
    kill_switch_result: Result<(), NetworkError>,
) -> Result<(), NetworkError> {
    link_dependent_result?;
    kill_switch_result
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
        NetworkError::CommandFailed { .. }
        | NetworkError::DnsNotSupported
        | NetworkError::KillSwitchUnavailable(_) => RouteApplyFailureKind::System,
        NetworkError::Io(_) => RouteApplyFailureKind::Persistence,
        NetworkError::Netlink(_) | NetworkError::LinkNotFound(_) => RouteApplyFailureKind::Lookup,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_result_propagates_kill_switch_failure() {
        let error = merge_cleanup_results(
            Ok(()),
            Err(NetworkError::KillSwitchUnavailable("boom".to_string())),
        )
        .unwrap_err();

        assert!(matches!(error, NetworkError::KillSwitchUnavailable(message) if message == "boom"));
    }
}
