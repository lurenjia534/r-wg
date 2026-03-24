mod apply_report;
mod explain;
mod ids;
mod normalize;
mod planner;
mod presentation;
#[cfg(test)]
mod tests;
mod util;

use std::net::IpAddr;

use serde::{Deserialize, Serialize};

use super::config::{AllowedIp, RouteTable};

pub use apply_report::{
    RouteApplyAttemptState, RouteApplyEntry, RouteApplyFailureKind, RouteApplyKind,
    RouteApplyPhase, RouteApplyReport, RouteApplyReportSource, RouteApplyStatus,
};
pub use normalize::{
    collect_allowed_routes, detect_full_tunnel, linux_default_policy_table_id,
    linux_policy_table_id, linux_route_table_for, normalize_config_for_runtime,
};
pub use planner::windows_planned_bypass_count;

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
