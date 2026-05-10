use crate::core::config::{RouteTable, WireGuardConfig};
use crate::core::route_plan::{
    RouteApplyFailureKind, RouteApplyKind, RouteApplyReport, RoutePlan, RoutePlanPolicyRuleOp,
    RoutePlanRouteKind, RoutePlanRouteOp,
};
use crate::platform::NetworkApplyError;

use super::policy::PolicyRoutingState;
use super::{NetworkError, DEFAULT_POLICY_TABLE};

pub(super) struct LinuxApplyPlan {
    pub(super) route_ops: Vec<RoutePlanRouteOp>,
    pub(super) policy_rule_ops: Vec<RoutePlanPolicyRuleOp>,
    pub(super) policy: Option<PolicyRoutingState>,
}

pub(super) fn build_apply_plan(
    config: &WireGuardConfig,
    route_plan: &RoutePlan,
    report: &mut RouteApplyReport,
) -> Result<LinuxApplyPlan, NetworkApplyError> {
    let route_ops = route_plan
        .route_ops
        .iter()
        .filter(|route_op| matches!(route_op.kind, RoutePlanRouteKind::Allowed))
        .cloned()
        .collect();
    let policy = resolve_policy_state(config, route_plan, report)?;

    Ok(LinuxApplyPlan {
        route_ops,
        policy_rule_ops: route_plan.policy_rule_ops.clone(),
        policy,
    })
}

fn resolve_policy_state(
    config: &WireGuardConfig,
    route_plan: &RoutePlan,
    report: &mut RouteApplyReport,
) -> Result<Option<PolicyRoutingState>, NetworkApplyError> {
    let interface = &config.interface;
    if interface.table == Some(RouteTable::Off) || !route_plan.full_tunnel.any() {
        return Ok(None);
    }

    let table_id = route_plan
        .linux_policy_table_id
        .unwrap_or(match interface.table {
            Some(RouteTable::Id(value)) => value,
            _ => DEFAULT_POLICY_TABLE,
        });
    let Some(fwmark) = interface.fwmark else {
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
    };

    Ok(Some(PolicyRoutingState {
        table_id,
        fwmark,
        v4: route_plan.full_tunnel.ipv4,
        v6: route_plan.full_tunnel.ipv6,
    }))
}

#[cfg(test)]
mod tests {
    use crate::core::config::{AllowedIp, InterfaceConfig, Key, PeerConfig};
    use crate::core::route_plan::{RoutePlanPlatform, DEFAULT_FULL_TUNNEL_FWMARK};

    use super::*;

    fn key() -> Key {
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
            .parse()
            .unwrap()
    }

    fn config(table: Option<RouteTable>, fwmark: Option<u32>, allowed: &[&str]) -> WireGuardConfig {
        WireGuardConfig {
            interface: InterfaceConfig {
                private_key: key(),
                listen_port: None,
                fwmark,
                addresses: Vec::new(),
                dns_servers: Vec::new(),
                dns_search: Vec::new(),
                mtu: None,
                table,
            },
            peers: vec![PeerConfig {
                public_key: key(),
                preshared_key: None,
                allowed_ips: allowed
                    .iter()
                    .map(|value| value.parse::<AllowedIp>().unwrap())
                    .collect(),
                endpoint: None,
                persistent_keepalive: None,
            }],
        }
    }

    #[test]
    fn full_tunnel_requires_fwmark_for_policy_routing() {
        let config = config(None, None, &["0.0.0.0/0"]);
        let route_plan = RoutePlan::build(RoutePlanPlatform::Linux, &config);
        let mut report = RouteApplyReport::new(RoutePlanPlatform::Linux);

        let error = match build_apply_plan(&config, &route_plan, &mut report) {
            Ok(_) => panic!("full tunnel without fwmark should fail"),
            Err(error) => error,
        };

        assert!(matches!(error.error, NetworkError::MissingFwmark));
        assert!(report.entries.iter().any(|entry| {
            entry.item_id == "apply:linux:fwmark"
                && entry.failure_kind == Some(RouteApplyFailureKind::Precondition)
        }));
    }

    #[test]
    fn full_tunnel_policy_uses_configured_fwmark() {
        let config = config(
            None,
            Some(DEFAULT_FULL_TUNNEL_FWMARK),
            &["0.0.0.0/0", "::/0"],
        );
        let route_plan = RoutePlan::build(RoutePlanPlatform::Linux, &config);
        let mut report = RouteApplyReport::new(RoutePlanPlatform::Linux);

        let plan = match build_apply_plan(&config, &route_plan, &mut report) {
            Ok(plan) => plan,
            Err(error) => panic!("apply plan should build: {}", error.error),
        };
        let policy = plan.policy.expect("full tunnel should use policy routing");

        assert_eq!(policy.fwmark, DEFAULT_FULL_TUNNEL_FWMARK);
        assert!(policy.v4);
        assert!(policy.v6);
    }

    #[test]
    fn table_off_skips_policy_and_routes() {
        let config = config(Some(RouteTable::Off), None, &["0.0.0.0/0"]);
        let route_plan = RoutePlan::build(RoutePlanPlatform::Linux, &config);
        let mut report = RouteApplyReport::new(RoutePlanPlatform::Linux);

        let plan = match build_apply_plan(&config, &route_plan, &mut report) {
            Ok(plan) => plan,
            Err(error) => panic!("apply plan should build: {}", error.error),
        };

        assert!(plan.policy.is_none());
        assert!(plan.route_ops.is_empty());
        assert!(plan.policy_rule_ops.is_empty());
    }
}
