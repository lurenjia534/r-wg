use std::net::IpAddr;

use super::{
    RoutePlan, RoutePlanBypassOp, RoutePlanFamily, RoutePlanMetricOp, RoutePlanPolicyRuleOp,
    RoutePlanRouteKind, RoutePlanRouteOp,
};

pub(super) fn route_item_id(route_op: &RoutePlanRouteOp) -> String {
    match route_op.kind {
        RoutePlanRouteKind::Allowed => super::util::item_id(
            "allowed",
            &super::util::route_text(route_op.route.addr, route_op.route.cidr),
        ),
        RoutePlanRouteKind::DnsHost => super::util::item_id("dns-route", &route_op.route.addr.to_string()),
    }
}

pub(super) fn policy_item_id(policy_op: &RoutePlanPolicyRuleOp) -> String {
    match policy_op.family {
        RoutePlanFamily::Ipv4 => "policy-v4".to_string(),
        RoutePlanFamily::Ipv6 => "policy-v6".to_string(),
    }
}

pub(super) fn metric_item_id(metric_op: &RoutePlanMetricOp) -> String {
    match metric_op.family {
        RoutePlanFamily::Ipv4 => "metric-v4".to_string(),
        RoutePlanFamily::Ipv6 => "metric-v6".to_string(),
    }
}

pub(super) fn bypass_item_id(bypass_op: &RoutePlanBypassOp) -> String {
    let prefix = if bypass_op.host.parse::<IpAddr>().is_ok() {
        "bypass"
    } else {
        "bypass-pending"
    };
    super::util::item_id(prefix, &format!("{}:{}", bypass_op.host, bypass_op.port))
}

impl RoutePlan {
    pub fn route_item_id(route_op: &RoutePlanRouteOp) -> String {
        route_item_id(route_op)
    }

    pub fn policy_item_id(policy_op: &RoutePlanPolicyRuleOp) -> String {
        policy_item_id(policy_op)
    }

    pub fn metric_item_id(metric_op: &RoutePlanMetricOp) -> String {
        metric_item_id(metric_op)
    }

    pub fn bypass_item_id(bypass_op: &RoutePlanBypassOp) -> String {
        bypass_item_id(bypass_op)
    }
}
