use crate::dns::{DnsMode, DnsPreset};

use super::super::config::{AllowedIp, InterfaceConfig, Key, PeerConfig, RouteTable, WireGuardConfig};
use super::*;

fn key(value: &str) -> Key {
    value.parse().unwrap()
}

fn interface(table: Option<RouteTable>) -> InterfaceConfig {
    InterfaceConfig {
        private_key: key("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="),
        listen_port: None,
        fwmark: None,
        addresses: Vec::new(),
        dns_servers: Vec::new(),
        dns_search: Vec::new(),
        mtu: None,
        table,
    }
}

fn peer(allowed: &[&str], endpoint: Option<&str>) -> PeerConfig {
    PeerConfig {
        public_key: key("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="),
        preshared_key: None,
        allowed_ips: allowed
            .iter()
            .map(|value| value.parse::<AllowedIp>().unwrap())
            .collect(),
        endpoint: endpoint.map(|value| value.parse().unwrap()),
        persistent_keepalive: None,
    }
}

fn config(table: Option<RouteTable>, peers: Vec<PeerConfig>) -> WireGuardConfig {
    WireGuardConfig {
        interface: interface(table),
        peers,
    }
}

#[test]
fn collect_allowed_routes_deduplicates_preserving_order() {
    let peers = vec![
        peer(&["10.0.0.0/24", "10.0.1.0/24"], Some("example.com:51820")),
        peer(&["10.0.1.0/24", "0.0.0.0/0"], None),
    ];

    let routes = collect_allowed_routes(&peers);

    assert_eq!(
        routes,
        vec![
            "10.0.0.0/24".parse::<AllowedIp>().unwrap(),
            "10.0.1.0/24".parse::<AllowedIp>().unwrap(),
            "0.0.0.0/0".parse::<AllowedIp>().unwrap(),
        ]
    );
}

#[test]
fn detect_full_tunnel_reports_each_family_independently() {
    let routes = vec![
        "0.0.0.0/0".parse::<AllowedIp>().unwrap(),
        "192.168.0.0/16".parse::<AllowedIp>().unwrap(),
        "::/0".parse::<AllowedIp>().unwrap(),
    ];

    let status = detect_full_tunnel(&routes);

    assert!(status.ipv4);
    assert!(status.ipv6);
    assert!(status.any());
}

#[test]
fn normalize_config_for_runtime_applies_dns_selection_and_fwmark() {
    let mut cfg = config(None, vec![peer(&["0.0.0.0/0"], Some("example.com:51820"))]);
    cfg.interface.dns_servers = vec!["10.0.0.53".parse().unwrap()];

    let normalized = normalize_config_for_runtime(
        cfg,
        crate::dns::DnsSelection::new(DnsMode::UseSystemDns, DnsPreset::CloudflareStandard),
    );

    assert!(normalized.interface.dns_servers.is_empty());
    assert_eq!(
        normalized.interface.fwmark,
        Some(DEFAULT_FULL_TUNNEL_FWMARK)
    );
}

#[test]
fn linux_policy_table_id_prefers_explicit_table() {
    let status = FullTunnelStatus {
        ipv4: true,
        ipv6: false,
    };

    assert_eq!(
        linux_policy_table_id(RoutePlanPlatform::Linux, Some(RouteTable::Id(321)), status),
        Some(321)
    );
    assert_eq!(
        linux_policy_table_id(RoutePlanPlatform::Linux, None, status),
        Some(LINUX_DEFAULT_POLICY_TABLE_ID)
    );
    assert_eq!(
        linux_policy_table_id(RoutePlanPlatform::Linux, Some(RouteTable::Off), status),
        None
    );
    assert_eq!(
        linux_policy_table_id(RoutePlanPlatform::Windows, Some(RouteTable::Id(321)), status),
        None
    );
}

#[test]
fn linux_route_table_for_uses_policy_table_for_matching_family() {
    let status = FullTunnelStatus {
        ipv4: true,
        ipv6: false,
    };
    let route = "10.0.0.0/24".parse::<AllowedIp>().unwrap();
    let table = linux_route_table_for(
        RoutePlanPlatform::Linux,
        Some(RouteTable::Id(321)),
        Some(900),
        status,
        &route,
    );

    assert_eq!(table, Some(900));
}

#[test]
fn windows_planned_bypass_count_counts_ip_literals_and_hostnames_for_full_tunnel() {
    let peers = vec![
        peer(&["0.0.0.0/0"], Some("203.0.113.10:51820")),
        peer(&["::/0"], Some("example.com:51820")),
        peer(&["10.0.0.0/24"], Some("[2001:db8::1]:51820")),
        peer(&["10.0.0.0/24"], None),
    ];
    let status = FullTunnelStatus {
        ipv4: true,
        ipv6: true,
    };

    assert_eq!(
        windows_planned_bypass_count(RoutePlanPlatform::Windows, &peers, status),
        3
    );
    assert_eq!(
        windows_planned_bypass_count(RoutePlanPlatform::Linux, &peers, status),
        0
    );
}

#[test]
fn bypass_item_id_distinguishes_same_host_different_ports() {
    let first = RoutePlan::bypass_item_id(&RoutePlanBypassOp {
        host: "example.com".to_string(),
        port: 51820,
    });
    let second = RoutePlan::bypass_item_id(&RoutePlanBypassOp {
        host: "example.com".to_string(),
        port: 51821,
    });

    assert_ne!(first, second);
    assert!(first.contains("51820"));
    assert!(second.contains("51821"));
}

#[test]
fn route_plan_build_is_consistent() {
    let plan = RoutePlan::build(
        RoutePlanPlatform::Windows,
        &config(None, vec![peer(&["0.0.0.0/0"], Some("example.com:51820"))]),
    );

    assert_eq!(plan.platform, RoutePlanPlatform::Windows);
    assert_eq!(plan.allowed_routes.len(), 1);
    assert!(plan.full_tunnel.ipv4);
    assert_eq!(plan.windows_planned_bypass_count, 1);
    assert_eq!(plan.linux_policy_table_id, None);
    assert_eq!(plan.metric_ops.len(), 1);
    assert_eq!(plan.bypass_ops.len(), 1);
    assert_eq!(plan.route_ops.len(), 1);
    assert!(!plan.inventory_groups.is_empty());
}

#[test]
fn explain_matches_endpoint_hostname() {
    let plan = RoutePlan::build(
        RoutePlanPlatform::Windows,
        &config(None, vec![peer(&["0.0.0.0/0"], Some("example.com:51820"))]),
    );

    let explain = plan.explain("example.com");

    assert!(explain.headline.contains("configured endpoint"));
    assert!(explain.matched_item_id.is_some());
}

#[test]
fn linux_plan_builds_explicit_route_and_policy_ops() {
    let mut cfg = config(
        None,
        vec![peer(
            &["0.0.0.0/0", "10.0.0.0/24"],
            Some("example.com:51820"),
        )],
    );
    cfg.interface.fwmark = Some(DEFAULT_FULL_TUNNEL_FWMARK);

    let plan = RoutePlan::build(RoutePlanPlatform::Linux, &cfg);

    assert_eq!(plan.policy_rule_ops.len(), 1);
    assert_eq!(plan.policy_rule_ops[0].family, RoutePlanFamily::Ipv4);
    assert_eq!(
        plan.policy_rule_ops[0].table_id,
        LINUX_DEFAULT_POLICY_TABLE_ID
    );
    assert_eq!(plan.policy_rule_ops[0].fwmark, DEFAULT_FULL_TUNNEL_FWMARK);
    assert_eq!(plan.route_ops.len(), 2);
    assert!(plan
        .route_ops
        .iter()
        .all(|op| matches!(op.kind, RoutePlanRouteKind::Allowed)));
    assert!(plan
        .route_ops
        .iter()
        .all(|op| op.table_id == Some(LINUX_DEFAULT_POLICY_TABLE_ID)));
}

#[test]
fn windows_plan_includes_metric_bypass_and_dns_host_route_ops() {
    let mut cfg = config(None, vec![peer(&["0.0.0.0/0"], Some("example.com:51820"))]);
    cfg.interface.dns_servers = vec!["10.0.0.53".parse().unwrap()];

    let plan = RoutePlan::build(RoutePlanPlatform::Windows, &cfg);

    assert_eq!(plan.metric_ops.len(), 1);
    assert_eq!(plan.metric_ops[0].family, RoutePlanFamily::Ipv4);
    assert_eq!(plan.metric_ops[0].metric, 0);
    assert_eq!(plan.bypass_ops.len(), 1);
    assert_eq!(plan.bypass_ops[0].host, "example.com");
    assert_eq!(plan.route_ops.len(), 2);
    assert!(plan
        .route_ops
        .iter()
        .any(|op| matches!(op.kind, RoutePlanRouteKind::DnsHost)));
}
