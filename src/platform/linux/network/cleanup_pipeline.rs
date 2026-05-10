use crate::core::config::RouteTable;
use crate::core::route_plan::RoutePlanRouteKind;
use crate::log::events::{dns as log_dns, net as log_net};

use super::dns::cleanup_dns;
use super::netlink::{delete_address, delete_route, link_index, netlink_handle};
use super::recovery::{cleanup_policy_state, clear_persisted_apply_report, clear_recovery_journal};
use super::{AppliedNetworkState, NetworkError};

/// 清理之前应用的网络配置。
pub(super) async fn cleanup_network_config(state: AppliedNetworkState) -> Result<(), NetworkError> {
    cleanup_network_config_impl(state, true).await
}

pub(super) async fn cleanup_network_config_impl(
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
        kill_switch.cleanup().await.inspect_err(|err| {
            log_net::kill_switch_cleanup_failed(&err);
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
