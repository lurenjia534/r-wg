mod apply_report;
mod ids;
mod normalize;
mod planner;
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
}

pub type OperationalRoutePlan = RoutePlan;
