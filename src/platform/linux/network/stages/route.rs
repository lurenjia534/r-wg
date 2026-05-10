use crate::core::config::RouteTable;
use crate::core::route_plan::{
    RouteApplyFailureKind, RouteApplyKind, RouteApplyReport, RoutePlan, RoutePlanRouteOp,
};
use rtnetlink::Handle;

use super::super::killswitch::KillSwitchState;
use super::super::netlink::build_route_message;
use super::super::policy::PolicyRoutingState;
use super::super::NetworkError;
use super::recovery_journal::persist_applying_recovery_state;
use crate::log::events::net as log_net;

pub(in crate::platform::linux::network) async fn apply_route_stage(
    handle: &Handle,
    link_index: u32,
    tun_name: &str,
    route_plan: &RoutePlan,
    table: Option<RouteTable>,
    policy: Option<&PolicyRoutingState>,
    kill_switch: Option<&KillSwitchState>,
    route_ops: &[RoutePlanRouteOp],
    report: &mut RouteApplyReport,
) -> Result<(), NetworkError> {
    if table == Some(RouteTable::Off) {
        return Ok(());
    }

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
            return Err(err.into());
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
            persist_applying_recovery_state(tun_name, route_plan, policy, kill_switch, report)
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
            return Err(err);
        }
    }

    Ok(())
}
