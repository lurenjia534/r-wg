use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use serde::{Deserialize, Serialize};

use crate::dns::{apply_dns_selection, DnsSelection};

use super::config::{AllowedIp, PeerConfig, RouteTable, WireGuardConfig};

/// 全隧道场景下自动补齐的默认 fwmark。
pub const DEFAULT_FULL_TUNNEL_FWMARK: u32 = 0x5257;
/// Linux 全隧道时默认使用的 policy table。
pub const LINUX_DEFAULT_POLICY_TABLE_ID: u32 = 200;

/// 路由规划运行平台。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoutePlanPlatform {
    Linux,
    Windows,
    Other,
}

impl RoutePlanPlatform {
    pub fn current() -> Self {
        if cfg!(target_os = "linux") {
            Self::Linux
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Other
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Windows => "windows",
            Self::Other => "other",
        }
    }
}

/// 全隧道判定结果。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FullTunnelStatus {
    pub ipv4: bool,
    pub ipv6: bool,
}

impl FullTunnelStatus {
    pub fn any(self) -> bool {
        self.ipv4 || self.ipv6
    }

    pub fn matches(self, addr: IpAddr) -> bool {
        match addr {
            IpAddr::V4(_) => self.ipv4,
            IpAddr::V6(_) => self.ipv6,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoutePlanTone {
    Secondary,
    Info,
    Warning,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoutePlanStepKind {
    Interface,
    Dns,
    Policy,
    Peer,
    Endpoint,
    Guardrail,
    Destination,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoutePlanStaticStatus {
    Skipped,
    Warning,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoutePlanRouteKind {
    Allowed,
    DnsHost,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoutePlanFamily {
    Ipv4,
    Ipv6,
}

impl RoutePlanFamily {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ipv4 => "IPv4",
            Self::Ipv6 => "IPv6",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutePlanChip {
    pub label: String,
    pub tone: RoutePlanTone,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutePlanGraphStep {
    pub kind: RoutePlanStepKind,
    pub label: String,
    pub value: String,
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutePlanInspector {
    pub title: String,
    pub subtitle: String,
    pub why_match: Vec<String>,
    pub platform_details: Vec<String>,
    pub risk_assessment: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutePlanRouteRow {
    pub destination: String,
    pub family: String,
    pub kind: String,
    pub peer: String,
    pub endpoint: String,
    pub table: String,
    pub note: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutePlanMatchTarget {
    pub addr: IpAddr,
    pub cidr: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutePlanRouteOp {
    pub route: AllowedIp,
    pub table_id: Option<u32>,
    pub kind: RoutePlanRouteKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutePlanPolicyRuleOp {
    pub family: RoutePlanFamily,
    pub table_id: u32,
    pub fwmark: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutePlanMetricOp {
    pub family: RoutePlanFamily,
    pub metric: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutePlanBypassOp {
    pub host: String,
    pub port: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteApplyStatus {
    Applied,
    Skipped,
    Failed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteApplyReportSource {
    #[default]
    Live,
    Persisted,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteApplyAttemptState {
    #[default]
    Applying,
    Running,
    Failed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteApplyPhase {
    #[default]
    Apply,
    Cleanup,
    Recovery,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteApplyKind {
    #[default]
    Other,
    Adapter,
    RecoveryJournal,
    Address,
    PolicyRule,
    Route,
    Metric,
    BypassRoute,
    Dns,
    Nrpt,
    DnsGuard,
}

impl RouteApplyKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Other => "operation",
            Self::Adapter => "adapter",
            Self::RecoveryJournal => "recovery journal",
            Self::Address => "address",
            Self::PolicyRule => "policy rule",
            Self::Route => "route",
            Self::Metric => "metric",
            Self::BypassRoute => "bypass route",
            Self::Dns => "DNS",
            Self::Nrpt => "NRPT",
            Self::DnsGuard => "DNS guard",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteApplyFailureKind {
    Precondition,
    Lookup,
    Persistence,
    Verification,
    System,
    Cleanup,
}

impl RouteApplyFailureKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Precondition => "precondition",
            Self::Lookup => "lookup",
            Self::Persistence => "persistence",
            Self::Verification => "verification",
            Self::System => "system",
            Self::Cleanup => "cleanup",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteApplyEntry {
    pub item_id: String,
    pub status: RouteApplyStatus,
    #[serde(default)]
    pub phase: RouteApplyPhase,
    #[serde(default)]
    pub kind: RouteApplyKind,
    #[serde(default)]
    pub failure_kind: Option<RouteApplyFailureKind>,
    pub evidence: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteApplyReport {
    pub platform: Option<RoutePlanPlatform>,
    #[serde(default)]
    pub source: RouteApplyReportSource,
    #[serde(default)]
    pub attempt_state: RouteApplyAttemptState,
    pub entries: Vec<RouteApplyEntry>,
}

impl RouteApplyReport {
    pub fn new(platform: RoutePlanPlatform) -> Self {
        Self {
            platform: Some(platform),
            source: RouteApplyReportSource::Live,
            attempt_state: RouteApplyAttemptState::Applying,
            entries: Vec::new(),
        }
    }

    pub fn push_applied(&mut self, item_id: impl Into<String>, evidence: Vec<String>) {
        self.push_applied_kind(item_id, RouteApplyKind::Other, evidence);
    }

    pub fn push_applied_kind(
        &mut self,
        item_id: impl Into<String>,
        kind: RouteApplyKind,
        evidence: Vec<String>,
    ) {
        let item_id = item_id.into();
        let inferred_phase = infer_apply_phase(&item_id);
        let inferred_kind = infer_apply_kind(&item_id).unwrap_or(kind);
        self.entries.push(RouteApplyEntry {
            phase: inferred_phase,
            item_id,
            status: RouteApplyStatus::Applied,
            kind: inferred_kind,
            failure_kind: None,
            evidence,
        });
    }

    pub fn push_skipped(&mut self, item_id: impl Into<String>, evidence: Vec<String>) {
        self.push_skipped_kind(item_id, RouteApplyKind::Other, evidence);
    }

    pub fn push_skipped_kind(
        &mut self,
        item_id: impl Into<String>,
        kind: RouteApplyKind,
        evidence: Vec<String>,
    ) {
        let item_id = item_id.into();
        let inferred_phase = infer_apply_phase(&item_id);
        let inferred_kind = infer_apply_kind(&item_id).unwrap_or(kind);
        self.entries.push(RouteApplyEntry {
            phase: inferred_phase,
            item_id,
            status: RouteApplyStatus::Skipped,
            kind: inferred_kind,
            failure_kind: None,
            evidence,
        });
    }

    pub fn push_failed(&mut self, item_id: impl Into<String>, evidence: Vec<String>) {
        self.push_failed_kind(item_id, RouteApplyKind::Other, None, evidence);
    }

    pub fn push_failed_kind(
        &mut self,
        item_id: impl Into<String>,
        kind: RouteApplyKind,
        failure_kind: Option<RouteApplyFailureKind>,
        evidence: Vec<String>,
    ) {
        let item_id = item_id.into();
        let inferred_phase = infer_apply_phase(&item_id);
        let inferred_kind = infer_apply_kind(&item_id).unwrap_or(kind);
        let inferred_failure_kind = failure_kind.or_else(|| infer_failure_kind(&item_id));
        self.entries.push(RouteApplyEntry {
            phase: inferred_phase,
            item_id,
            status: RouteApplyStatus::Failed,
            kind: inferred_kind,
            failure_kind: inferred_failure_kind,
            evidence,
        });
    }

    pub fn mark_running(&mut self) {
        self.attempt_state = RouteApplyAttemptState::Running;
    }

    pub fn mark_failed(&mut self) {
        self.attempt_state = RouteApplyAttemptState::Failed;
    }

    pub fn mark_persisted(&mut self) {
        self.source = RouteApplyReportSource::Persisted;
    }
}

fn infer_apply_phase(item_id: &str) -> RouteApplyPhase {
    if item_id == "apply:recovery" || item_id == "apply:recovery_init" {
        RouteApplyPhase::Recovery
    } else if item_id.starts_with("cleanup:") {
        RouteApplyPhase::Cleanup
    } else {
        RouteApplyPhase::Apply
    }
}

fn infer_apply_kind(item_id: &str) -> Option<RouteApplyKind> {
    if item_id == "policy-v4" || item_id == "policy-v6" || item_id.starts_with("policy:") {
        Some(RouteApplyKind::PolicyRule)
    } else if item_id == "metric-v4" || item_id == "metric-v6" || item_id.starts_with("metric:") {
        Some(RouteApplyKind::Metric)
    } else if item_id.starts_with("bypass:") || item_id.starts_with("bypass-pending:") {
        Some(RouteApplyKind::BypassRoute)
    } else if item_id.starts_with("allowed:")
        || item_id.starts_with("dns-route:")
        || item_id.starts_with("route:")
    {
        Some(RouteApplyKind::Route)
    } else if item_id == "apply:adapter_lookup" || item_id == "apply:linux:netlink" {
        Some(RouteApplyKind::Adapter)
    } else if item_id.starts_with("apply:address:") || item_id == "apply:addresses" {
        Some(RouteApplyKind::Address)
    } else if item_id == "apply:dns" {
        Some(RouteApplyKind::Dns)
    } else if item_id == "apply:nrpt" {
        Some(RouteApplyKind::Nrpt)
    } else if item_id == "apply:dns_guard" {
        Some(RouteApplyKind::DnsGuard)
    } else if item_id == "apply:recovery" || item_id == "apply:recovery_init" {
        Some(RouteApplyKind::RecoveryJournal)
    } else {
        None
    }
}

fn infer_failure_kind(item_id: &str) -> Option<RouteApplyFailureKind> {
    if item_id == "apply:adapter_lookup" {
        Some(RouteApplyFailureKind::Lookup)
    } else if item_id == "apply:recovery" || item_id == "apply:recovery_init" {
        Some(RouteApplyFailureKind::Persistence)
    } else if item_id == "apply:stale_address_cleanup" {
        Some(RouteApplyFailureKind::Cleanup)
    } else if item_id.starts_with("apply:")
        || item_id.starts_with("allowed:")
        || item_id == "policy-v4"
        || item_id == "policy-v6"
        || item_id == "metric-v4"
        || item_id == "metric-v6"
        || item_id.starts_with("bypass:")
        || item_id.starts_with("bypass-pending:")
        || item_id.starts_with("dns-route:")
    {
        Some(RouteApplyFailureKind::System)
    } else {
        None
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutePlanItem {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub family: Option<RoutePlanFamily>,
    pub static_status: Option<RoutePlanStaticStatus>,
    pub event_patterns: Vec<String>,
    pub chips: Vec<RoutePlanChip>,
    pub inspector: RoutePlanInspector,
    pub graph_steps: Vec<RoutePlanGraphStep>,
    pub route_row: Option<RoutePlanRouteRow>,
    pub match_target: Option<RoutePlanMatchTarget>,
    pub endpoint_host: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutePlanGroup {
    pub id: String,
    pub label: String,
    pub empty_note: String,
    pub items: Vec<RoutePlanItem>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutePlanExplainResult {
    pub query: String,
    pub headline: String,
    pub summary: String,
    pub steps: Vec<String>,
    pub risk: Vec<String>,
    pub matched_item_id: Option<String>,
}

/// 路由规划的共享模型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutePlan {
    pub platform: RoutePlanPlatform,
    pub requested_table: Option<RouteTable>,
    pub allowed_routes: Vec<AllowedIp>,
    pub full_tunnel: FullTunnelStatus,
    pub linux_policy_table_id: Option<u32>,
    pub route_ops: Vec<RoutePlanRouteOp>,
    pub policy_rule_ops: Vec<RoutePlanPolicyRuleOp>,
    pub metric_ops: Vec<RoutePlanMetricOp>,
    pub bypass_ops: Vec<RoutePlanBypassOp>,
    pub windows_planned_bypass_count: usize,
    pub plan_status: String,
    pub summary_chips: Vec<RoutePlanChip>,
    pub inventory_groups: Vec<RoutePlanGroup>,
}

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
        let full_tunnel_active = full_tunnel.any();
        let table_off = parsed.interface.table == Some(RouteTable::Off);
        let dns_guard_text = dns_guard_label(
            platform,
            full_tunnel_active,
            parsed.interface.dns_servers.is_empty(),
        );
        let inventory_groups = build_inventory_groups(
            platform,
            parsed,
            &allowed_routes,
            full_tunnel,
            linux_policy_table_id,
        );
        let warning_count = inventory_groups
            .iter()
            .find(|group| group.id == "group-warnings")
            .map(|group| group.items.len())
            .unwrap_or(0);
        let route_table_text = format_route_table(parsed.interface.table);

        let mut summary_chips = vec![
            chip(
                if full_tunnel_active {
                    "Full Tunnel"
                } else {
                    "Split Tunnel"
                },
                RoutePlanTone::Info,
            ),
            chip(
                if full_tunnel.ipv4 {
                    "IPv4 Full"
                } else {
                    "IPv4 Split"
                },
                RoutePlanTone::Secondary,
            ),
            chip(
                if full_tunnel.ipv6 {
                    "IPv6 Full"
                } else {
                    "IPv6 Split"
                },
                RoutePlanTone::Secondary,
            ),
            chip(
                dns_guard_text,
                if parsed.interface.dns_servers.is_empty() {
                    RoutePlanTone::Warning
                } else if full_tunnel_active {
                    RoutePlanTone::Info
                } else {
                    RoutePlanTone::Secondary
                },
            ),
            chip(
                format!("Guardrails {warning_count}"),
                if warning_count > 0 {
                    RoutePlanTone::Warning
                } else {
                    RoutePlanTone::Secondary
                },
            ),
            chip(
                format!("Bypass {windows_planned_bypass_count}"),
                if platform == RoutePlanPlatform::Windows && full_tunnel_active {
                    RoutePlanTone::Info
                } else {
                    RoutePlanTone::Secondary
                },
            ),
            chip(
                format!("Table {route_table_text}"),
                RoutePlanTone::Secondary,
            ),
        ];
        if table_off {
            summary_chips.insert(1, chip("Table Off", RoutePlanTone::Warning));
        }

        let plan_status = if table_off {
            "Plan generated, but route apply is disabled by Table=Off.".to_string()
        } else if full_tunnel_active {
            "Decision Path combines effective routes, full-tunnel guardrails, and recent runtime evidence.".to_string()
        } else {
            "Decision Path combines effective routes and recent runtime evidence for the current config.".to_string()
        };

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
            plan_status,
            summary_chips,
            inventory_groups,
        }
    }

    pub fn explain(&self, search_query: &str) -> RoutePlanExplainResult {
        build_explain(&self.inventory_groups, search_query)
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
        effective_route_table_label(
            self.platform,
            self.requested_table,
            self.linux_policy_table_id,
            route,
            self.full_tunnel,
        )
    }

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

pub fn normalize_config_for_runtime(
    mut config: WireGuardConfig,
    dns_selection: DnsSelection,
) -> WireGuardConfig {
    apply_dns_selection(
        &mut config.interface.dns_servers,
        &mut config.interface.dns_search,
        dns_selection,
    );
    if wants_full_tunnel(&config.peers) && config.interface.fwmark.is_none() {
        config.interface.fwmark = Some(DEFAULT_FULL_TUNNEL_FWMARK);
    }
    config
}

/// 从所有 peer 收集 AllowedIPs 并去重。
pub fn collect_allowed_routes(peers: &[PeerConfig]) -> Vec<AllowedIp> {
    let mut seen = HashSet::new();
    let mut routes = Vec::new();
    for peer in peers {
        for allowed in &peer.allowed_ips {
            if seen.insert((allowed.addr, allowed.cidr)) {
                routes.push(AllowedIp {
                    addr: allowed.addr,
                    cidr: allowed.cidr,
                });
            }
        }
    }
    routes
}

/// 判断是否包含 IPv4 / IPv6 全隧道路由。
pub fn detect_full_tunnel(routes: &[AllowedIp]) -> FullTunnelStatus {
    let mut full_tunnel = FullTunnelStatus::default();
    for route in routes {
        match route.addr {
            IpAddr::V4(addr) if addr.is_unspecified() && route.cidr == 0 => {
                full_tunnel.ipv4 = true;
            }
            IpAddr::V6(addr) if addr.is_unspecified() && route.cidr == 0 => {
                full_tunnel.ipv6 = true;
            }
            _ => {}
        }
    }
    full_tunnel
}

/// Linux 全隧道时使用的默认 policy table。
pub fn linux_default_policy_table_id() -> u32 {
    LINUX_DEFAULT_POLICY_TABLE_ID
}

/// Linux policy table 决策。
pub fn linux_policy_table_id(
    platform: RoutePlanPlatform,
    table: Option<RouteTable>,
    full_tunnel: FullTunnelStatus,
) -> Option<u32> {
    if platform != RoutePlanPlatform::Linux || table == Some(RouteTable::Off) || !full_tunnel.any()
    {
        return None;
    }

    Some(match table {
        Some(RouteTable::Id(id)) => id,
        _ => LINUX_DEFAULT_POLICY_TABLE_ID,
    })
}

/// Linux 路由表选择。
pub fn linux_route_table_for(
    platform: RoutePlanPlatform,
    table: Option<RouteTable>,
    policy_table_id: Option<u32>,
    full_tunnel: FullTunnelStatus,
    route: &AllowedIp,
) -> Option<u32> {
    if platform == RoutePlanPlatform::Linux {
        if let Some(policy_table_id) = policy_table_id {
            match route.addr {
                IpAddr::V4(_) if full_tunnel.ipv4 => return Some(policy_table_id),
                IpAddr::V6(_) if full_tunnel.ipv6 => return Some(policy_table_id),
                _ => {}
            }
        }
    }

    route_table_id(table)
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
                RoutePlanPlatform::Other => route_table_id(parsed.interface.table),
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

fn route_item_id(route_op: &RoutePlanRouteOp) -> String {
    match route_op.kind {
        RoutePlanRouteKind::Allowed => item_id(
            "allowed",
            &route_text(route_op.route.addr, route_op.route.cidr),
        ),
        RoutePlanRouteKind::DnsHost => item_id("dns-route", &route_op.route.addr.to_string()),
    }
}

fn policy_item_id(policy_op: &RoutePlanPolicyRuleOp) -> String {
    match policy_op.family {
        RoutePlanFamily::Ipv4 => "policy-v4".to_string(),
        RoutePlanFamily::Ipv6 => "policy-v6".to_string(),
    }
}

fn metric_item_id(metric_op: &RoutePlanMetricOp) -> String {
    match metric_op.family {
        RoutePlanFamily::Ipv4 => "metric-v4".to_string(),
        RoutePlanFamily::Ipv6 => "metric-v6".to_string(),
    }
}

fn bypass_item_id(bypass_op: &RoutePlanBypassOp) -> String {
    let prefix = if bypass_op.host.parse::<IpAddr>().is_ok() {
        "bypass"
    } else {
        "bypass-pending"
    };
    item_id(prefix, &format!("{}:{}", bypass_op.host, bypass_op.port))
}

/// Windows 全隧道时计划下发的 endpoint bypass 数量。
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

fn build_inventory_groups(
    platform: RoutePlanPlatform,
    parsed: &WireGuardConfig,
    routes: &[AllowedIp],
    full_tunnel: FullTunnelStatus,
    linux_policy_table_id: Option<u32>,
) -> Vec<RoutePlanGroup> {
    let table_off = parsed.interface.table == Some(RouteTable::Off);
    let interface_label = interface_label(parsed);
    let dns_label = dns_label(parsed);
    let route_table_text = format_route_table(parsed.interface.table);

    let mut full_tunnel_items = Vec::new();
    let mut split_route_items = Vec::new();
    let mut seen = HashSet::new();

    for (peer_index, peer) in parsed.peers.iter().enumerate() {
        let peer_label = format!("Peer {}", peer_index + 1);
        let endpoint_text = peer
            .endpoint
            .as_ref()
            .map(|endpoint| format!("{}:{}", endpoint.host, endpoint.port))
            .unwrap_or_else(|| "No endpoint".to_string());

        for allowed in &peer.allowed_ips {
            if !seen.insert((allowed.addr, allowed.cidr)) {
                continue;
            }

            let destination_text = route_text(allowed.addr, allowed.cidr);
            let family = family_from_ip(allowed.addr);
            let is_full = is_full_tunnel_route(allowed);
            let effective_table = effective_route_table_label(
                platform,
                parsed.interface.table,
                linux_policy_table_id,
                allowed,
                full_tunnel,
            );
            let route_kind = if is_full {
                "allowed / full tunnel"
            } else {
                "allowed / split"
            };
            let note = if table_off {
                "Table=Off keeps the match in config but skips route installation.".to_string()
            } else if is_full {
                "This prefix turns the family into full tunnel routing.".to_string()
            } else {
                "This prefix stays split and only captures matching destinations.".to_string()
            };

            let item = RoutePlanItem {
                id: item_id("allowed", &destination_text),
                title: destination_text.clone(),
                subtitle: format!("{peer_label} via {endpoint_text}"),
                family: Some(family),
                static_status: table_off.then_some(RoutePlanStaticStatus::Skipped),
                event_patterns: vec![destination_text.clone()],
                chips: vec![
                    chip(
                        if is_full {
                            "Full Tunnel"
                        } else {
                            "Split Route"
                        },
                        if is_full {
                            RoutePlanTone::Info
                        } else {
                            RoutePlanTone::Secondary
                        },
                    ),
                    chip(family.label(), RoutePlanTone::Secondary),
                    chip(peer_label.clone(), RoutePlanTone::Secondary),
                ],
                inspector: RoutePlanInspector {
                    title: destination_text.clone(),
                    subtitle: format!("{peer_label} advertises this prefix."),
                    why_match: vec![
                        format!("Matched by AllowedIPs on {peer_label}."),
                        format!("Endpoint for this peer is {endpoint_text}."),
                    ],
                    platform_details: allowed_platform_details(
                        platform,
                        allowed,
                        &effective_table,
                        parsed.interface.table,
                        parsed.interface.fwmark,
                        full_tunnel,
                    ),
                    risk_assessment: allowed_risks(platform, parsed, allowed, is_full),
                },
                graph_steps: allowed_graph_steps(
                    &interface_label,
                    &route_table_text,
                    &peer_label,
                    &endpoint_text,
                    &destination_text,
                    &dns_label,
                    is_full,
                ),
                route_row: Some(RoutePlanRouteRow {
                    destination: destination_text.clone(),
                    family: family.label().to_string(),
                    kind: route_kind.to_string(),
                    peer: peer_label.clone(),
                    endpoint: endpoint_text.clone(),
                    table: effective_table,
                    note,
                }),
                match_target: Some(RoutePlanMatchTarget {
                    addr: allowed.addr,
                    cidr: allowed.cidr,
                }),
                endpoint_host: peer.endpoint.as_ref().map(|endpoint| endpoint.host.clone()),
            };

            if is_full {
                full_tunnel_items.push(item);
            } else {
                split_route_items.push(item);
            }
        }
    }

    let endpoint_bypass_items =
        build_endpoint_bypass_items(platform, parsed, full_tunnel, &interface_label);
    let dns_route_items = build_dns_route_items(platform, parsed, full_tunnel, &interface_label);
    let policy_items = build_policy_items(
        platform,
        parsed,
        full_tunnel,
        table_off,
        linux_policy_table_id,
    );
    let warning_items = build_warning_items(platform, parsed, routes, full_tunnel);

    vec![
        group(
            "group-full-tunnel",
            "Full Tunnel",
            full_tunnel_items,
            if full_tunnel.any() {
                "Default-route prefixes that take over an address family."
            } else {
                "No 0/0 prefixes in the current config."
            },
        ),
        group(
            "group-split-routes",
            "Split Routes",
            split_route_items,
            "Specific prefixes that stay routed through the selected peer.",
        ),
        group(
            "group-endpoint-bypass",
            "Endpoint Bypass",
            endpoint_bypass_items,
            if platform == RoutePlanPlatform::Windows {
                "Endpoint escape routes only exist when full tunnel needs them."
            } else {
                "Linux uses policy routing instead of explicit endpoint bypass routes."
            },
        ),
        group(
            "group-dns-routes",
            "DNS Routes",
            dns_route_items,
            if platform == RoutePlanPlatform::Windows {
                "Host routes that keep tunnel DNS reachable under full tunnel."
            } else {
                "No dedicated DNS host routes are planned on this platform."
            },
        ),
        group(
            "group-policy-rules",
            "Policy Rules",
            policy_items,
            if platform == RoutePlanPlatform::Linux {
                "Linux full tunnel relies on fwmark and suppress-main rules."
            } else {
                "Windows full tunnel relies on interface metrics and explicit bypass routes."
            },
        ),
        group(
            "group-warnings",
            "Warnings / Guardrails",
            warning_items,
            "Potential leak risks or disabled routing behaviour.",
        ),
    ]
}

fn build_endpoint_bypass_items(
    platform: RoutePlanPlatform,
    parsed: &WireGuardConfig,
    full_tunnel: FullTunnelStatus,
    interface_label: &str,
) -> Vec<RoutePlanItem> {
    if platform != RoutePlanPlatform::Windows || !full_tunnel.any() {
        return Vec::new();
    }

    let mut items = Vec::new();
    for (peer_index, peer) in parsed.peers.iter().enumerate() {
        let Some(endpoint) = peer.endpoint.as_ref() else {
            continue;
        };
        let peer_label = format!("Peer {}", peer_index + 1);
        let endpoint_text = format!("{}:{}", endpoint.host, endpoint.port);
        let endpoint_ip = endpoint.host.parse::<IpAddr>().ok();
        let applies = match endpoint_ip {
            Some(ip) => full_tunnel.matches(ip),
            None => full_tunnel.any(),
        };
        if !applies {
            continue;
        }

        match endpoint_ip {
            Some(ip) => {
                let destination_text = route_text(ip, if ip.is_ipv4() { 32 } else { 128 });
                let bypass_op = RoutePlanBypassOp {
                    host: endpoint.host.clone(),
                    port: endpoint.port,
                };
                items.push(RoutePlanItem {
                    id: bypass_item_id(&bypass_op),
                    title: endpoint.host.clone(),
                    subtitle: format!("{peer_label} endpoint bypass"),
                    family: Some(family_from_ip(ip)),
                    static_status: None,
                    event_patterns: vec![
                        ip.to_string(),
                        format!("bypass route add: {ip}"),
                        destination_text.clone(),
                    ],
                    chips: vec![
                        chip("Bypass", RoutePlanTone::Info),
                        chip(family_from_ip(ip).label(), RoutePlanTone::Secondary),
                        chip(peer_label.clone(), RoutePlanTone::Secondary),
                    ],
                    inspector: RoutePlanInspector {
                        title: endpoint.host.clone(),
                        subtitle: "Endpoint must stay on the underlay path.".to_string(),
                        why_match: vec![
                            format!(
                                "Full tunnel would otherwise capture traffic to {endpoint_text}."
                            ),
                            "Windows creates a host route outside the tunnel for endpoint reachability.".to_string(),
                        ],
                        platform_details: vec![
                            "Windows resolves or reuses endpoint IPs before installing bypass routes.".to_string(),
                            "The bypass route follows the system best route instead of the tunnel adapter.".to_string(),
                        ],
                        risk_assessment: vec![
                            "If this bypass route cannot be installed, full-tunnel apply aborts to avoid leaks.".to_string(),
                        ],
                    },
                    graph_steps: vec![
                        graph_step(
                            RoutePlanStepKind::Interface,
                            "Local Interface",
                            interface_label,
                            None,
                        ),
                        graph_step(
                            RoutePlanStepKind::Guardrail,
                            "Guardrail",
                            "Full tunnel requires endpoint escape",
                            Some("The endpoint must not recurse into the tunnel."),
                        ),
                        graph_step(
                            RoutePlanStepKind::Destination,
                            "Bypass Route",
                            &destination_text,
                            Some("Uses the current system best route."),
                        ),
                        graph_step(
                            RoutePlanStepKind::Endpoint,
                            "Endpoint",
                            &endpoint_text,
                            None,
                        ),
                    ],
                    route_row: Some(RoutePlanRouteRow {
                        destination: destination_text.clone(),
                        family: family_from_ip(ip).label().to_string(),
                        kind: "bypass".to_string(),
                        peer: peer_label.clone(),
                        endpoint: endpoint_text.clone(),
                        table: "system route".to_string(),
                        note: "Host route keeps the endpoint outside the tunnel.".to_string(),
                    }),
                    match_target: Some(RoutePlanMatchTarget {
                        addr: ip,
                        cidr: if ip.is_ipv4() { 32 } else { 128 },
                    }),
                    endpoint_host: Some(endpoint.host.clone()),
                });
            }
            None => {
                let bypass_op = RoutePlanBypassOp {
                    host: endpoint.host.clone(),
                    port: endpoint.port,
                };
                items.push(RoutePlanItem {
                    id: bypass_item_id(&bypass_op),
                    title: endpoint.host.clone(),
                    subtitle: format!("{peer_label} endpoint hostname"),
                    family: None,
                    static_status: Some(RoutePlanStaticStatus::Warning),
                    event_patterns: vec![endpoint.host.clone()],
                    chips: vec![
                        chip("Bypass Pending", RoutePlanTone::Warning),
                        chip(peer_label.clone(), RoutePlanTone::Secondary),
                    ],
                    inspector: RoutePlanInspector {
                        title: endpoint.host.clone(),
                        subtitle: "Needs runtime resolution before bypass exists.".to_string(),
                        why_match: vec![format!(
                            "Endpoint hostname {endpoint_text} cannot be resolved from config alone."
                        )],
                        platform_details: vec![
                            "Windows resolves endpoint hostnames inside the privileged backend during apply.".to_string(),
                            "The UI keeps this item visible so the user can verify the leak guard path.".to_string(),
                        ],
                        risk_assessment: vec![
                            "Until endpoint resolution succeeds, the bypass route is not concrete.".to_string(),
                            "The backend will abort full-tunnel apply if resolution/bypass fails.".to_string(),
                        ],
                    },
                    graph_steps: vec![
                        graph_step(
                            RoutePlanStepKind::Interface,
                            "Local Interface",
                            interface_label,
                            None,
                        ),
                        graph_step(
                            RoutePlanStepKind::Guardrail,
                            "Guardrail",
                            "Endpoint bypass pending",
                            Some("Runtime DNS resolution is required first."),
                        ),
                        graph_step(
                            RoutePlanStepKind::Endpoint,
                            "Endpoint Host",
                            &endpoint_text,
                            None,
                        ),
                    ],
                    route_row: None,
                    match_target: None,
                    endpoint_host: Some(endpoint.host.clone()),
                });
            }
        }
    }

    items
}

fn build_dns_route_items(
    platform: RoutePlanPlatform,
    parsed: &WireGuardConfig,
    full_tunnel: FullTunnelStatus,
    interface_label: &str,
) -> Vec<RoutePlanItem> {
    if platform != RoutePlanPlatform::Windows || parsed.interface.table == Some(RouteTable::Off) {
        return Vec::new();
    }

    let mut items = Vec::new();
    for dns_server in &parsed.interface.dns_servers {
        if !full_tunnel.matches(*dns_server) {
            continue;
        }

        let destination_text = route_text(*dns_server, if dns_server.is_ipv4() { 32 } else { 128 });
        items.push(RoutePlanItem {
            id: item_id("dns-route", &dns_server.to_string()),
            title: dns_server.to_string(),
            subtitle: "Tunnel DNS host route".to_string(),
            family: Some(family_from_ip(*dns_server)),
            static_status: None,
            event_patterns: vec![
                destination_text.clone(),
                format!("dns route add: {destination_text}"),
                dns_server.to_string(),
            ],
            chips: vec![
                chip("DNS Route", RoutePlanTone::Info),
                chip(family_from_ip(*dns_server).label(), RoutePlanTone::Secondary),
            ],
            inspector: RoutePlanInspector {
                title: dns_server.to_string(),
                subtitle: "Keeps tunnel DNS reachable under full tunnel.".to_string(),
                why_match: vec![
                    "This DNS server belongs to the tunnel config.".to_string(),
                    "Windows adds a host route so tunnel DNS does not get pre-empted by other routes.".to_string(),
                ],
                platform_details: vec![
                    "The host route points back to the tunnel adapter with tunnel metric."
                        .to_string(),
                ],
                risk_assessment: vec![
                    "Without the host route, a more specific underlay route could bypass tunnel DNS.".to_string(),
                ],
            },
            graph_steps: vec![
                graph_step(
                    RoutePlanStepKind::Interface,
                    "Local Interface",
                    interface_label,
                    None,
                ),
                graph_step(
                    RoutePlanStepKind::Dns,
                    "Tunnel DNS",
                    &dns_server.to_string(),
                    Some("Pinned with a host route under full tunnel."),
                ),
                graph_step(
                    RoutePlanStepKind::Destination,
                    "Route",
                    &destination_text,
                    Some("Installed on the tunnel adapter."),
                ),
            ],
            route_row: Some(RoutePlanRouteRow {
                destination: destination_text.clone(),
                family: family_from_ip(*dns_server).label().to_string(),
                kind: "dns".to_string(),
                peer: "Interface DNS".to_string(),
                endpoint: "-".to_string(),
                table: "tunnel adapter".to_string(),
                note: "Host route protects tunnel DNS reachability.".to_string(),
            }),
            match_target: Some(RoutePlanMatchTarget {
                addr: *dns_server,
                cidr: if dns_server.is_ipv4() { 32 } else { 128 },
            }),
            endpoint_host: None,
        });
    }

    items
}

fn build_policy_items(
    platform: RoutePlanPlatform,
    parsed: &WireGuardConfig,
    full_tunnel: FullTunnelStatus,
    table_off: bool,
    linux_policy_table_id: Option<u32>,
) -> Vec<RoutePlanItem> {
    let route_table_text = format_route_table(parsed.interface.table);
    let mut items = Vec::new();

    if platform == RoutePlanPlatform::Linux {
        let Some(table_id) = linux_policy_table_id else {
            return items;
        };

        if full_tunnel.ipv4 {
            items.push(RoutePlanItem {
                id: "policy-v4".to_string(),
                title: "IPv4 policy routing".to_string(),
                subtitle: format!("fwmark -> main, not fwmark -> table {table_id}"),
                family: Some(RoutePlanFamily::Ipv4),
                static_status: table_off.then_some(RoutePlanStaticStatus::Skipped),
                event_patterns: vec!["policy rule add: v4".to_string()],
                chips: vec![
                    chip("Policy", RoutePlanTone::Info),
                    chip("IPv4", RoutePlanTone::Secondary),
                    chip(format!("table {table_id}"), RoutePlanTone::Secondary),
                ],
                inspector: RoutePlanInspector {
                    title: "IPv4 policy routing".to_string(),
                    subtitle: "Linux full tunnel keeps engine traffic on main.".to_string(),
                    why_match: vec![
                        "Full tunnel is active for IPv4.".to_string(),
                        "Linux uses fwmark rules instead of endpoint bypass routes.".to_string(),
                    ],
                    platform_details: vec![
                        format!(
                            "Traffic with fwmark 0x{:x} stays on main; unmarked traffic enters table {}.",
                            parsed.interface.fwmark.unwrap_or_default(),
                            table_id
                        ),
                        "A suppress-main rule prevents business traffic from falling back to the default route.".to_string(),
                    ],
                    risk_assessment: vec![
                        "Missing fwmark or failed rule install would make full-tunnel routing unsafe.".to_string(),
                    ],
                },
                graph_steps: vec![
                    graph_step(
                        RoutePlanStepKind::Interface,
                        "Local Interface",
                        &interface_label(parsed),
                        None,
                    ),
                    graph_step(
                        RoutePlanStepKind::Guardrail,
                        "fwmark",
                        &format!("0x{:x}", parsed.interface.fwmark.unwrap_or_default()),
                        Some("Engine traffic stays on main."),
                    ),
                    graph_step(
                        RoutePlanStepKind::Policy,
                        "Policy Table",
                        &format!("table {table_id}"),
                        Some("Business traffic enters the tunnel table."),
                    ),
                    graph_step(
                        RoutePlanStepKind::Guardrail,
                        "Suppress Main",
                        "pref rule",
                        Some("Prevents unmarked traffic from falling back to main."),
                    ),
                ],
                route_row: None,
                match_target: None,
                endpoint_host: None,
            });
        }

        if full_tunnel.ipv6 {
            items.push(RoutePlanItem {
                id: "policy-v6".to_string(),
                title: "IPv6 policy routing".to_string(),
                subtitle: format!("fwmark -> main, not fwmark -> table {table_id}"),
                family: Some(RoutePlanFamily::Ipv6),
                static_status: table_off.then_some(RoutePlanStaticStatus::Skipped),
                event_patterns: vec!["policy rule add: v6".to_string()],
                chips: vec![
                    chip("Policy", RoutePlanTone::Info),
                    chip("IPv6", RoutePlanTone::Secondary),
                    chip(format!("table {table_id}"), RoutePlanTone::Secondary),
                ],
                inspector: RoutePlanInspector {
                    title: "IPv6 policy routing".to_string(),
                    subtitle: "Linux full tunnel keeps engine traffic on main.".to_string(),
                    why_match: vec![
                        "Full tunnel is active for IPv6.".to_string(),
                        "Linux uses fwmark rules instead of endpoint bypass routes.".to_string(),
                    ],
                    platform_details: vec![
                        format!(
                            "Traffic with fwmark 0x{:x} stays on main; unmarked traffic enters table {}.",
                            parsed.interface.fwmark.unwrap_or_default(),
                            table_id
                        ),
                        "A suppress-main rule prevents business traffic from falling back to the default route.".to_string(),
                    ],
                    risk_assessment: vec![
                        "Missing fwmark or failed rule install would make full-tunnel routing unsafe.".to_string(),
                    ],
                },
                graph_steps: vec![
                    graph_step(
                        RoutePlanStepKind::Interface,
                        "Local Interface",
                        &interface_label(parsed),
                        None,
                    ),
                    graph_step(
                        RoutePlanStepKind::Guardrail,
                        "fwmark",
                        &format!("0x{:x}", parsed.interface.fwmark.unwrap_or_default()),
                        Some("Engine traffic stays on main."),
                    ),
                    graph_step(
                        RoutePlanStepKind::Policy,
                        "Policy Table",
                        &format!("table {table_id}"),
                        Some("Business traffic enters the tunnel table."),
                    ),
                    graph_step(
                        RoutePlanStepKind::Guardrail,
                        "Suppress Main",
                        "pref rule",
                        Some("Prevents unmarked traffic from falling back to main."),
                    ),
                ],
                route_row: None,
                match_target: None,
                endpoint_host: None,
            });
        }
    } else if platform == RoutePlanPlatform::Windows && full_tunnel.any() {
        if full_tunnel.ipv4 {
            items.push(RoutePlanItem {
                id: "metric-v4".to_string(),
                title: "IPv4 full-tunnel metric".to_string(),
                subtitle: "Windows lowers interface metric for the tunnel".to_string(),
                family: Some(RoutePlanFamily::Ipv4),
                static_status: table_off.then_some(RoutePlanStaticStatus::Skipped),
                event_patterns: vec!["interface metric set: v4".to_string()],
                chips: vec![
                    chip("Metric", RoutePlanTone::Info),
                    chip("IPv4", RoutePlanTone::Secondary),
                    chip(format!("table {route_table_text}"), RoutePlanTone::Secondary),
                ],
                inspector: RoutePlanInspector {
                    title: "IPv4 full-tunnel metric".to_string(),
                    subtitle: "Windows uses interface priority rather than policy rules.".to_string(),
                    why_match: vec!["Full tunnel is active for IPv4.".to_string()],
                    platform_details: vec![
                        "The tunnel adapter metric is lowered so 0.0.0.0/0 prefers the tunnel.".to_string(),
                        "Endpoint and DNS exceptions are handled with host routes.".to_string(),
                    ],
                    risk_assessment: vec![
                        "If metric programming fails, apply aborts rather than leaving a half-configured full tunnel.".to_string(),
                    ],
                },
                graph_steps: vec![
                    graph_step(
                        RoutePlanStepKind::Interface,
                        "Local Interface",
                        &interface_label(parsed),
                        None,
                    ),
                    graph_step(
                        RoutePlanStepKind::Policy,
                        "Interface Metric",
                        "50",
                        Some("Lower metric makes full tunnel preferred."),
                    ),
                    graph_step(
                        RoutePlanStepKind::Policy,
                        "Config Table",
                        &route_table_text,
                        Some("RouteTable IDs are ignored on Windows."),
                    ),
                ],
                route_row: None,
                match_target: None,
                endpoint_host: None,
            });
        }

        if full_tunnel.ipv6 {
            items.push(RoutePlanItem {
                id: "metric-v6".to_string(),
                title: "IPv6 full-tunnel metric".to_string(),
                subtitle: "Windows lowers interface metric for the tunnel".to_string(),
                family: Some(RoutePlanFamily::Ipv6),
                static_status: table_off.then_some(RoutePlanStaticStatus::Skipped),
                event_patterns: vec!["interface metric set: v6".to_string()],
                chips: vec![
                    chip("Metric", RoutePlanTone::Info),
                    chip("IPv6", RoutePlanTone::Secondary),
                    chip(format!("table {route_table_text}"), RoutePlanTone::Secondary),
                ],
                inspector: RoutePlanInspector {
                    title: "IPv6 full-tunnel metric".to_string(),
                    subtitle: "Windows uses interface priority rather than policy rules.".to_string(),
                    why_match: vec!["Full tunnel is active for IPv6.".to_string()],
                    platform_details: vec![
                        "The tunnel adapter metric is lowered so ::/0 prefers the tunnel.".to_string(),
                        "Endpoint and DNS exceptions are handled with host routes.".to_string(),
                    ],
                    risk_assessment: vec![
                        "If metric programming fails, apply aborts rather than leaving a half-configured full tunnel.".to_string(),
                    ],
                },
                graph_steps: vec![
                    graph_step(
                        RoutePlanStepKind::Interface,
                        "Local Interface",
                        &interface_label(parsed),
                        None,
                    ),
                    graph_step(
                        RoutePlanStepKind::Policy,
                        "Interface Metric",
                        "50",
                        Some("Lower metric makes full tunnel preferred."),
                    ),
                    graph_step(
                        RoutePlanStepKind::Policy,
                        "Config Table",
                        &route_table_text,
                        Some("RouteTable IDs are ignored on Windows."),
                    ),
                ],
                route_row: None,
                match_target: None,
                endpoint_host: None,
            });
        }
    }

    items
}

fn build_warning_items(
    platform: RoutePlanPlatform,
    parsed: &WireGuardConfig,
    routes: &[AllowedIp],
    full_tunnel: FullTunnelStatus,
) -> Vec<RoutePlanItem> {
    let mut items = Vec::new();

    if parsed.peers.is_empty() {
        items.push(warning_item(
            "warning-no-peers",
            "No peers configured",
            "The route plan cannot carry traffic without at least one peer.",
            vec![
                "No [Peer] sections exist in the current config.".to_string(),
                "Route Map can still preview interface settings, but no destination will reach a tunnel peer.".to_string(),
            ],
        ));
    }

    if parsed.interface.table == Some(RouteTable::Off) && !routes.is_empty() {
        items.push(warning_item(
            "warning-table-off",
            "Table=Off disables route apply",
            "Matches still exist logically, but the backend skips route installation.",
            vec![
                "AllowedIPs remain useful for explanation, but they will not become kernel routes."
                    .to_string(),
                "This is safe only if the user expects to manage routes out-of-band.".to_string(),
            ],
        ));
    }

    if full_tunnel.any() && parsed.interface.dns_servers.is_empty() {
        items.push(warning_item(
            "warning-no-dns",
            "Full tunnel without tunnel DNS",
            "Traffic may be protected, but DNS intent is ambiguous.",
            vec![
                "No DNS servers are configured inside the tunnel.".to_string(),
                "Users should verify whether system DNS is acceptable for this profile."
                    .to_string(),
            ],
        ));
    }

    if platform == RoutePlanPlatform::Linux
        && full_tunnel.any()
        && parsed.interface.fwmark.is_none()
    {
        items.push(warning_item(
            "warning-fwmark",
            "Full tunnel needs fwmark",
            "Linux policy routing depends on fwmark to keep endpoint traffic on main.",
            vec![
                "The engine currently auto-fills a default fwmark when full tunnel is detected.".to_string(),
                "Route Map surfaces this because a mismatch here breaks trust in the route decision.".to_string(),
            ],
        ));
    }

    items
}

fn warning_item(
    id: &str,
    title: &str,
    subtitle: &str,
    risk_assessment: Vec<String>,
) -> RoutePlanItem {
    RoutePlanItem {
        id: id.to_string(),
        title: title.to_string(),
        subtitle: subtitle.to_string(),
        family: None,
        static_status: Some(RoutePlanStaticStatus::Warning),
        event_patterns: Vec::new(),
        chips: vec![chip("Guardrail", RoutePlanTone::Warning)],
        inspector: RoutePlanInspector {
            title: title.to_string(),
            subtitle: subtitle.to_string(),
            why_match: vec![
                "This item is derived from config semantics rather than a single route."
                    .to_string(),
            ],
            platform_details: vec![
                "Use this section to spot leak-prone or intentionally disabled behaviour."
                    .to_string(),
            ],
            risk_assessment,
        },
        graph_steps: vec![graph_step(
            RoutePlanStepKind::Guardrail,
            "Guardrail",
            title,
            Some(subtitle),
        )],
        route_row: None,
        match_target: None,
        endpoint_host: None,
    }
}

fn group(id: &str, label: &str, items: Vec<RoutePlanItem>, empty_note: &str) -> RoutePlanGroup {
    RoutePlanGroup {
        id: id.to_string(),
        label: label.to_string(),
        empty_note: empty_note.to_string(),
        items,
    }
}

fn build_explain(groups: &[RoutePlanGroup], search_query: &str) -> RoutePlanExplainResult {
    let query = search_query.trim().to_string();
    if query.is_empty() {
        return RoutePlanExplainResult {
            query,
            headline: "Explain a target".to_string(),
            summary: "Search for an IP, CIDR, or endpoint hostname to see how the current plan resolves it.".to_string(),
            steps: vec![
                "IP queries pick the most specific planned route or bypass rule.".to_string(),
                "Hostname queries match configured peer endpoint hosts and call out when runtime resolution is still needed.".to_string(),
            ],
            risk: Vec::new(),
            matched_item_id: None,
        };
    }

    let items = groups
        .iter()
        .flat_map(|group| group.items.iter())
        .collect::<Vec<_>>();

    if let Ok(ip) = query.parse::<IpAddr>() {
        let best = items
            .iter()
            .filter_map(|item| {
                let target = item.match_target.as_ref()?;
                cidr_contains(target.addr, target.cidr, ip).then_some((item, target.cidr))
            })
            .max_by_key(|(_, cidr)| *cidr)
            .map(|(item, _)| item);

        if let Some(item) = best {
            return RoutePlanExplainResult {
                query: query.clone(),
                headline: format!("{ip} matches {}", item.title),
                summary: item.subtitle.clone(),
                steps: explain_steps_from_item(item, Some(ip)),
                risk: item.inspector.risk_assessment.clone(),
                matched_item_id: Some(item.id.clone()),
            };
        }

        return RoutePlanExplainResult {
            query: query.clone(),
            headline: format!("{ip} has no planned match"),
            summary: "The current config does not advertise a concrete route for this destination.".to_string(),
            steps: vec![
                "No AllowedIPs, DNS host route, or endpoint bypass rule contains this IP.".to_string(),
                "If the expectation differs, inspect the inventory for missing prefixes or family filters.".to_string(),
            ],
            risk: vec![
                "Unmatched traffic follows the platform default routing policy, not the tunnel plan."
                    .to_string(),
            ],
            matched_item_id: None,
        };
    }

    if let Some(item) = items.iter().find(|item| {
        item.endpoint_host
            .as_ref()
            .map(|host| host.eq_ignore_ascii_case(&query))
            .unwrap_or(false)
    }) {
        return RoutePlanExplainResult {
            query: query.clone(),
            headline: format!("{query} is a configured endpoint"),
            summary: item.subtitle.clone(),
            steps: explain_steps_from_item(item, None),
            risk: item.inspector.risk_assessment.clone(),
            matched_item_id: Some(item.id.clone()),
        };
    }

    RoutePlanExplainResult {
        query: query.clone(),
        headline: format!("No direct explanation for {query}"),
        summary: "Only configured endpoint hosts can be explained without runtime DNS resolution.".to_string(),
        steps: vec![
            "Route Map can explain raw IPs, CIDRs, and configured endpoint hostnames.".to_string(),
            "Generic domains need a resolved IP before they can be matched against the current route plan.".to_string(),
        ],
        risk: Vec::new(),
        matched_item_id: None,
    }
}

fn explain_steps_from_item(item: &RoutePlanItem, ip: Option<IpAddr>) -> Vec<String> {
    let mut steps = Vec::new();
    if let Some(ip) = ip {
        steps.push(format!("{ip} matched the most specific visible route."));
    }
    for step in &item.graph_steps {
        match step.note.as_ref() {
            Some(note) => steps.push(format!("{} -> {} ({note})", step.label, step.value)),
            None => steps.push(format!("{} -> {}", step.label, step.value)),
        }
    }
    steps
}

fn allowed_graph_steps(
    interface_label: &str,
    route_table_text: &str,
    peer_label: &str,
    endpoint_text: &str,
    route_text: &str,
    dns_label: &str,
    full_tunnel: bool,
) -> Vec<RoutePlanGraphStep> {
    vec![
        graph_step(
            RoutePlanStepKind::Interface,
            "Local Interface",
            interface_label,
            None,
        ),
        graph_step(
            RoutePlanStepKind::Dns,
            "DNS / Guard",
            dns_label,
            if full_tunnel {
                Some("Full tunnel makes DNS behaviour part of the routing trust story.")
            } else {
                None
            },
        ),
        graph_step(
            RoutePlanStepKind::Policy,
            "Route Table",
            route_table_text,
            if full_tunnel {
                Some("Full-tunnel families may add extra guardrails or policy handling.")
            } else {
                None
            },
        ),
        graph_step(
            RoutePlanStepKind::Peer,
            "Peer",
            peer_label,
            Some(endpoint_text),
        ),
        graph_step(
            RoutePlanStepKind::Destination,
            "Destination Prefix",
            route_text,
            None,
        ),
    ]
}

fn allowed_platform_details(
    platform: RoutePlanPlatform,
    allowed: &AllowedIp,
    effective_table: &str,
    table: Option<RouteTable>,
    fwmark: Option<u32>,
    full_tunnel: FullTunnelStatus,
) -> Vec<String> {
    let mut details = vec![format!("Effective table: {effective_table}.")];
    if platform == RoutePlanPlatform::Linux {
        if linux_policy_table_id(platform, table, full_tunnel).is_some() {
            details.push(format!(
                "Linux may promote {} into the policy table when full tunnel is active.",
                route_text(allowed.addr, allowed.cidr)
            ));
        }
        if let Some(fwmark) = fwmark {
            details.push(format!("fwmark configured: 0x{fwmark:x}."));
        }
    } else if platform == RoutePlanPlatform::Windows {
        if matches!(table, Some(RouteTable::Id(_))) {
            details.push("Windows ignores explicit RouteTable IDs from the config.".to_string());
        }
        details.push(
            "Windows relies on adapter metric plus host exceptions for full tunnel.".to_string(),
        );
    }
    details
}

fn allowed_risks(
    platform: RoutePlanPlatform,
    parsed: &WireGuardConfig,
    allowed: &AllowedIp,
    is_full: bool,
) -> Vec<String> {
    let mut risks = Vec::new();
    if parsed.interface.table == Some(RouteTable::Off) {
        risks.push(
            "Table=Off means this route stays explanatory only and is not installed.".to_string(),
        );
    }
    if is_full && parsed.interface.dns_servers.is_empty() {
        risks.push("Full tunnel is active without tunnel DNS servers.".to_string());
    }
    if platform == RoutePlanPlatform::Linux && is_full && parsed.interface.fwmark.is_none() {
        risks.push(
            "Linux full tunnel depends on fwmark/policy state for safe endpoint escape."
                .to_string(),
        );
    }
    if route_is_default_family(allowed, RoutePlanFamily::Ipv4)
        || route_is_default_family(allowed, RoutePlanFamily::Ipv6)
    {
        risks.push(
            "Default-route prefixes have the highest blast radius when misapplied.".to_string(),
        );
    }
    if risks.is_empty() {
        risks.push("No elevated risk flagged for this route in the planned model.".to_string());
    }
    risks
}

fn effective_route_table_label(
    platform: RoutePlanPlatform,
    table: Option<RouteTable>,
    linux_policy_table_id: Option<u32>,
    route: &AllowedIp,
    full_tunnel: FullTunnelStatus,
) -> String {
    if platform == RoutePlanPlatform::Linux {
        if let Some(policy_table_id) = linux_policy_table_id {
            match route.addr {
                IpAddr::V4(_) if full_tunnel.ipv4 => return format!("table {policy_table_id}"),
                IpAddr::V6(_) if full_tunnel.ipv6 => return format!("table {policy_table_id}"),
                _ => {}
            }
        }
        match table {
            Some(RouteTable::Id(id)) => format!("table {id}"),
            Some(RouteTable::Off) => "off".to_string(),
            _ => "main".to_string(),
        }
    } else if platform == RoutePlanPlatform::Windows {
        match table {
            Some(RouteTable::Off) => "off".to_string(),
            _ => "tunnel adapter".to_string(),
        }
    } else {
        format_route_table(table)
    }
}

fn route_table_id(table: Option<RouteTable>) -> Option<u32> {
    match table {
        Some(RouteTable::Id(id)) => Some(id),
        _ => None,
    }
}

fn format_route_table(table: Option<RouteTable>) -> String {
    match table {
        Some(RouteTable::Auto) => "auto".to_string(),
        Some(RouteTable::Off) => "off".to_string(),
        Some(RouteTable::Id(id)) => format!("id:{id}"),
        None => "main".to_string(),
    }
}

fn dns_guard_label(
    platform: RoutePlanPlatform,
    full_tunnel: bool,
    dns_empty: bool,
) -> &'static str {
    if dns_empty {
        "No Tunnel DNS"
    } else if platform == RoutePlanPlatform::Windows && full_tunnel {
        "NRPT + Guard"
    } else if full_tunnel {
        "Tunnel DNS"
    } else {
        "Config DNS"
    }
}

fn wants_full_tunnel(peers: &[PeerConfig]) -> bool {
    peers.iter().any(|peer| {
        peer.allowed_ips
            .iter()
            .any(|allowed| allowed.addr.is_unspecified() && allowed.cidr == 0)
    })
}

fn interface_label(parsed: &WireGuardConfig) -> String {
    if parsed.interface.addresses.is_empty() {
        "No interface address".to_string()
    } else {
        parsed
            .interface
            .addresses
            .iter()
            .map(|address| route_text(address.addr, address.cidr))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn dns_label(parsed: &WireGuardConfig) -> String {
    if parsed.interface.dns_servers.is_empty() {
        "No tunnel DNS".to_string()
    } else {
        parsed
            .interface
            .dns_servers
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn route_text(addr: IpAddr, cidr: u8) -> String {
    format!("{addr}/{cidr}")
}

fn family_from_ip(ip: IpAddr) -> RoutePlanFamily {
    match ip {
        IpAddr::V4(_) => RoutePlanFamily::Ipv4,
        IpAddr::V6(_) => RoutePlanFamily::Ipv6,
    }
}

fn is_full_tunnel_route(route: &AllowedIp) -> bool {
    route.addr.is_unspecified() && route.cidr == 0
}

fn route_is_default_family(route: &AllowedIp, family: RoutePlanFamily) -> bool {
    match (family, route.addr) {
        (RoutePlanFamily::Ipv4, IpAddr::V4(addr)) => {
            addr == Ipv4Addr::UNSPECIFIED && route.cidr == 0
        }
        (RoutePlanFamily::Ipv6, IpAddr::V6(addr)) => {
            addr == Ipv6Addr::UNSPECIFIED && route.cidr == 0
        }
        _ => false,
    }
}

fn cidr_contains(network: IpAddr, cidr: u8, value: IpAddr) -> bool {
    match (network, value) {
        (IpAddr::V4(network), IpAddr::V4(value)) => {
            let mask = if cidr == 0 {
                0
            } else {
                u32::MAX << (32 - cidr)
            };
            (u32::from(network) & mask) == (u32::from(value) & mask)
        }
        (IpAddr::V6(network), IpAddr::V6(value)) => {
            let mask = if cidr == 0 {
                0
            } else {
                u128::MAX << (128 - cidr)
            };
            (u128::from_be_bytes(network.octets()) & mask)
                == (u128::from_be_bytes(value.octets()) & mask)
        }
        _ => false,
    }
}

fn graph_step(
    kind: RoutePlanStepKind,
    label: &str,
    value: &str,
    note: Option<&str>,
) -> RoutePlanGraphStep {
    RoutePlanGraphStep {
        kind,
        label: label.to_string(),
        value: value.to_string(),
        note: note.map(ToString::to_string),
    }
}

fn chip(label: impl Into<String>, tone: RoutePlanTone) -> RoutePlanChip {
    RoutePlanChip {
        label: label.into(),
        tone,
    }
}

fn item_id(prefix: &str, suffix: &str) -> String {
    format!("{prefix}:{}", suffix.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use crate::dns::{DnsMode, DnsPreset};

    use super::super::config::{InterfaceConfig, Key};
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
            DnsSelection::new(DnsMode::UseSystemDns, DnsPreset::CloudflareStandard),
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
            linux_policy_table_id(
                RoutePlanPlatform::Windows,
                Some(RouteTable::Id(321)),
                status
            ),
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
}
