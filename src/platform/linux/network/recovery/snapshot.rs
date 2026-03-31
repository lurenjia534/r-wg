use std::net::IpAddr;

use serde::{Deserialize, Serialize};

use super::super::policy::PolicyRoutingState;
use crate::core::route_plan::{RoutePlan, RoutePlanRouteKind, RoutePlanRouteOp};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RecoveryRouteSnapshot {
    pub(crate) addr: IpAddr,
    pub(crate) cidr: u8,
    pub(crate) table_id: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RecoveryPolicySnapshot {
    pub(crate) table_id: u32,
    pub(crate) fwmark: u32,
    pub(crate) v4: bool,
    pub(crate) v6: bool,
}

pub(crate) fn route_snapshots(route_plan: &RoutePlan) -> Vec<RecoveryRouteSnapshot> {
    route_snapshots_from_ops(&route_plan.route_ops)
}

pub(crate) fn route_snapshots_from_ops(
    route_ops: &[RoutePlanRouteOp],
) -> Vec<RecoveryRouteSnapshot> {
    route_ops
        .iter()
        .filter(|route_op| matches!(route_op.kind, RoutePlanRouteKind::Allowed))
        .map(|route_op| RecoveryRouteSnapshot {
            addr: route_op.route.addr,
            cidr: route_op.route.cidr,
            table_id: route_op.table_id,
        })
        .collect()
}

pub(crate) fn policy_snapshot(policy: &PolicyRoutingState) -> RecoveryPolicySnapshot {
    RecoveryPolicySnapshot {
        table_id: policy.table_id,
        fwmark: policy.fwmark,
        v4: policy.v4,
        v6: policy.v6,
    }
}
