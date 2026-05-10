use crate::core::config::WireGuardConfig;
use crate::core::route_plan::{
    RouteApplyFailureKind, RouteApplyKind, RouteApplyReport, RoutePlan, RoutePlanPlatform,
};
use crate::log::events::net as log_net;
use crate::platform::{NetworkApplyError, NetworkApplyResult};

use super::apply_plan::build_apply_plan;
use super::cleanup_pipeline::cleanup_network_config_impl;
use super::logging::{log_default_routes, log_privileges};
use super::netlink::{link_index, netlink_handle};
use super::policy::cleanup_stale_default_routes_once;
use super::stages::{
    apply_dns_stage, apply_kill_switch_stage, apply_policy_stage, apply_route_stage,
    configure_link, persist_applying_recovery_state, persist_running_recovery_state,
};
use super::{AppliedNetworkState, NetworkError};

/// 应用 Linux 网络配置。
///
/// 只负责系统地址/路由/DNS，WireGuard 隧道本身由 gotatun 或 kernel backend 负责。
pub(super) async fn apply_network_config(
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

    let mut report = RouteApplyReport::new(RoutePlanPlatform::Linux);
    let apply_plan = build_apply_plan(config, route_plan, &mut report)?;

    let mut state = AppliedNetworkState {
        tun_name: tun_name.to_string(),
        addresses: interface.addresses.clone(),
        routes: apply_plan.route_ops.clone(),
        table: interface.table,
        dns: None,
        policy: apply_plan.policy,
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

        configure_link(handle, link_index, interface).await?;

        if let Err(err) = apply_policy_stage(
            handle,
            tun_name,
            route_plan,
            state.policy.as_ref(),
            state.kill_switch.as_ref(),
            &apply_plan.policy_rule_ops,
            &apply_plan.route_ops,
            &mut report,
        )
        .await
        {
            return abort_apply(tun_name, route_plan, state, &report, err).await;
        }

        if let Err(err) = apply_route_stage(
            handle,
            link_index,
            tun_name,
            route_plan,
            interface.table,
            state.policy.as_ref(),
            state.kill_switch.as_ref(),
            &apply_plan.route_ops,
            &mut report,
        )
        .await
        {
            return abort_apply(tun_name, route_plan, state, &report, err).await;
        }

        match apply_kill_switch_stage(
            tun_name,
            state.policy.as_ref(),
            kill_switch_enabled,
            &mut report,
        )
        .await
        {
            Ok(Some(kill_switch_state)) => {
                state.kill_switch = Some(kill_switch_state);
                if let Err(err) = persist_applying_recovery_state(
                    tun_name,
                    route_plan,
                    state.policy.as_ref(),
                    state.kill_switch.as_ref(),
                    &report,
                ) {
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
            Ok(None) => {}
            Err(err) => return abort_apply(tun_name, route_plan, state, &report, err).await,
        }

        // 输出当前默认路由，便于诊断主表/策略表走向。
        log_default_routes();

        match apply_dns_stage(tun_name, interface, &mut report).await {
            Ok(Some(dns_state)) => state.dns = Some(dns_state),
            Ok(None) => {}
            Err(err) => return abort_apply(tun_name, route_plan, state, &report, err).await,
        }

        report.mark_running();
        if let Err(err) = persist_running_recovery_state(
            tun_name,
            &apply_plan.route_ops,
            state.policy.as_ref(),
            state.kill_switch.as_ref(),
            state.dns.as_ref(),
            &report,
        ) {
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
    match result {
        Ok(result) => {
            log_net::apply_completed();
            Ok(result)
        }
        Err(error) => {
            log_net::apply_failed(&error);
            Err(NetworkApplyError { error, report })
        }
    }
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
