use rtnetlink::Handle;

use crate::core::route_plan::{
    RouteApplyFailureKind, RouteApplyKind, RouteApplyReport, RoutePlan, RoutePlanFamily,
    RoutePlanPolicyRuleOp, RoutePlanRouteOp,
};

use super::super::killswitch::KillSwitchState;
use super::super::policy::{apply_policy_rules, cleanup_policy_rules_once, PolicyRoutingState};
use super::super::NetworkError;
use super::recovery_journal::persist_applying_recovery_state;
use crate::log::events::net as log_net;

pub(in crate::platform::linux::network) async fn apply_policy_stage(
    handle: &Handle,
    tun_name: &str,
    route_plan: &RoutePlan,
    policy: Option<&PolicyRoutingState>,
    kill_switch: Option<&KillSwitchState>,
    policy_rule_ops: &[RoutePlanPolicyRuleOp],
    route_ops: &[RoutePlanRouteOp],
    report: &mut RouteApplyReport,
) -> Result<(), NetworkError> {
    let Some(policy_state) = policy else {
        return Ok(());
    };

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
            for skipped_route in route_ops {
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
            return Err(err);
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
            persist_applying_recovery_state(tun_name, route_plan, policy, kill_switch, report)
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
            return Err(err);
        }
    }

    Ok(())
}
