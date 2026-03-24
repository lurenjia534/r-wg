use std::net::IpAddr;

use super::super::config::{AllowedIp, PeerConfig, RouteTable, WireGuardConfig};
use super::{
    collect_allowed_routes, detect_full_tunnel, linux_policy_table_id, linux_route_table_for,
    FullTunnelStatus, RoutePlan, RoutePlanBypassOp, RoutePlanFamily, RoutePlanMetricOp,
    RoutePlanPlatform, RoutePlanPolicyRuleOp, RoutePlanRouteKind, RoutePlanRouteOp,
};

impl RoutePlan {
    pub fn build(platform: RoutePlanPlatform, parsed: &WireGuardConfig) -> Self {
        let allowed_routes = collect_allowed_routes(&parsed.peers);
        let full_tunnel = detect_full_tunnel(&allowed_routes);
        let linux_policy_table_id =
            linux_policy_table_id(platform, parsed.interface.table, full_tunnel);
        let route_ops = build_route_ops(
            platform,
            parsed,
            &allowed_routes,
            full_tunnel,
            linux_policy_table_id,
        );
        let policy_rule_ops =
            build_policy_rule_ops(platform, parsed, full_tunnel, linux_policy_table_id);
        let metric_ops = build_metric_ops(platform, parsed, full_tunnel);
        let bypass_ops = build_bypass_ops(platform, parsed, full_tunnel);
        let windows_planned_bypass_count = bypass_ops.len();

        Self {
            platform,
            requested_table: parsed.interface.table,
            allowed_routes,
            full_tunnel,
            linux_policy_table_id,
            route_ops,
            policy_rule_ops,
            metric_ops,
            bypass_ops,
            windows_planned_bypass_count,
        }
    }

    pub fn linux_route_table_for(&self, route: &AllowedIp) -> Option<u32> {
        linux_route_table_for(
            self.platform,
            self.requested_table,
            self.linux_policy_table_id,
            self.full_tunnel,
            route,
        )
    }

    pub fn effective_route_table_label(&self, route: &AllowedIp) -> String {
        super::util::effective_route_table_label(
            self.platform,
            self.requested_table,
            self.linux_policy_table_id,
            route,
            self.full_tunnel,
        )
    }
}

fn build_route_ops(
    platform: RoutePlanPlatform,
    parsed: &WireGuardConfig,
    allowed_routes: &[AllowedIp],
    full_tunnel: FullTunnelStatus,
    linux_policy_table_id: Option<u32>,
) -> Vec<RoutePlanRouteOp> {
    if parsed.interface.table == Some(RouteTable::Off) {
        return Vec::new();
    }

    let mut ops = allowed_routes
        .iter()
        .cloned()
        .map(|route| RoutePlanRouteOp {
            table_id: match platform {
                RoutePlanPlatform::Linux => linux_route_table_for(
                    platform,
                    parsed.interface.table,
                    linux_policy_table_id,
                    full_tunnel,
                    &route,
                ),
                RoutePlanPlatform::Windows => None,
                RoutePlanPlatform::Other => super::util::route_table_id(parsed.interface.table),
            },
            route,
            kind: RoutePlanRouteKind::Allowed,
        })
        .collect::<Vec<_>>();

    if platform == RoutePlanPlatform::Windows {
        for dns_server in &parsed.interface.dns_servers {
            let applies = (dns_server.is_ipv4() && full_tunnel.ipv4)
                || (dns_server.is_ipv6() && full_tunnel.ipv6);
            if !applies {
                continue;
            }

            ops.push(RoutePlanRouteOp {
                route: AllowedIp {
                    addr: *dns_server,
                    cidr: if dns_server.is_ipv4() { 32 } else { 128 },
                },
                table_id: None,
                kind: RoutePlanRouteKind::DnsHost,
            });
        }
    }

    ops
}

fn build_policy_rule_ops(
    platform: RoutePlanPlatform,
    parsed: &WireGuardConfig,
    full_tunnel: FullTunnelStatus,
    linux_policy_table_id: Option<u32>,
) -> Vec<RoutePlanPolicyRuleOp> {
    if platform != RoutePlanPlatform::Linux || parsed.interface.table == Some(RouteTable::Off) {
        return Vec::new();
    }

    let Some(table_id) = linux_policy_table_id else {
        return Vec::new();
    };
    let Some(fwmark) = parsed.interface.fwmark else {
        return Vec::new();
    };

    let mut ops = Vec::new();
    if full_tunnel.ipv4 {
        ops.push(RoutePlanPolicyRuleOp {
            family: RoutePlanFamily::Ipv4,
            table_id,
            fwmark,
        });
    }
    if full_tunnel.ipv6 {
        ops.push(RoutePlanPolicyRuleOp {
            family: RoutePlanFamily::Ipv6,
            table_id,
            fwmark,
        });
    }
    ops
}

fn build_metric_ops(
    platform: RoutePlanPlatform,
    parsed: &WireGuardConfig,
    full_tunnel: FullTunnelStatus,
) -> Vec<RoutePlanMetricOp> {
    if platform != RoutePlanPlatform::Windows || parsed.interface.table == Some(RouteTable::Off) {
        return Vec::new();
    }

    let mut ops = Vec::new();
    if full_tunnel.ipv4 {
        ops.push(RoutePlanMetricOp {
            family: RoutePlanFamily::Ipv4,
            metric: 0,
        });
    }
    if full_tunnel.ipv6 {
        ops.push(RoutePlanMetricOp {
            family: RoutePlanFamily::Ipv6,
            metric: 0,
        });
    }
    ops
}

fn build_bypass_ops(
    platform: RoutePlanPlatform,
    parsed: &WireGuardConfig,
    full_tunnel: FullTunnelStatus,
) -> Vec<RoutePlanBypassOp> {
    if platform != RoutePlanPlatform::Windows
        || parsed.interface.table == Some(RouteTable::Off)
        || !full_tunnel.any()
    {
        return Vec::new();
    }

    let mut ops = Vec::new();
    for peer in &parsed.peers {
        let Some(endpoint) = peer.endpoint.as_ref() else {
            continue;
        };

        match endpoint.host.parse::<IpAddr>() {
            Ok(IpAddr::V4(_)) if full_tunnel.ipv4 => ops.push(RoutePlanBypassOp {
                host: endpoint.host.clone(),
                port: endpoint.port,
            }),
            Ok(IpAddr::V6(_)) if full_tunnel.ipv6 => ops.push(RoutePlanBypassOp {
                host: endpoint.host.clone(),
                port: endpoint.port,
            }),
            Err(_) if full_tunnel.any() => ops.push(RoutePlanBypassOp {
                host: endpoint.host.clone(),
                port: endpoint.port,
            }),
            _ => {}
        }
    }

    ops
}

pub fn windows_planned_bypass_count(
    platform: RoutePlanPlatform,
    peers: &[PeerConfig],
    full_tunnel: FullTunnelStatus,
) -> usize {
    if platform != RoutePlanPlatform::Windows || !full_tunnel.any() {
        return 0;
    }

    let mut count = 0usize;
    for peer in peers {
        let Some(endpoint) = peer.endpoint.as_ref() else {
            continue;
        };

        match endpoint.host.parse::<IpAddr>() {
            Ok(IpAddr::V4(_)) if full_tunnel.ipv4 => count += 1,
            Ok(IpAddr::V6(_)) if full_tunnel.ipv6 => count += 1,
            Err(_) if full_tunnel.any() => count += 1,
            _ => {}
        }
    }

    count
}
