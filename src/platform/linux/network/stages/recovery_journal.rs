use crate::core::route_plan::{RouteApplyReport, RoutePlan, RoutePlanRouteOp};

use super::super::dns::DnsState;
use super::super::killswitch::KillSwitchState;
use super::super::policy::PolicyRoutingState;
use super::super::recovery::{
    write_applying_journal, write_persisted_apply_report, write_running_journal,
};
use super::super::NetworkError;

pub(in crate::platform::linux::network) fn persist_applying_recovery_state(
    tun_name: &str,
    route_plan: &RoutePlan,
    policy: Option<&PolicyRoutingState>,
    kill_switch: Option<&KillSwitchState>,
    report: &RouteApplyReport,
) -> Result<(), NetworkError> {
    write_persisted_apply_report(report)?;
    write_applying_journal(tun_name, route_plan, policy, kill_switch)
}

pub(in crate::platform::linux::network) fn persist_running_recovery_state(
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
