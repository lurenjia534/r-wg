//! 路由规划模块
//!
//! 本模块负责根据 WireGuard 配置计算需要应用的网络配置操作。
//!
//! # 核心概念
//!
//! - **AllowedIPs**: WireGuard 配置中的 AllowedIPs 决定哪些目标地址的流量应该经过隧道
//! - **全隧道模式**: 当 AllowedIPs 包含 `0.0.0.0/0` 或 `::/0` 时，所有流量都经过隧道
//! - **策略路由**: Linux 全隧道模式使用 fwmark + 策略路由确保所有流量走隧道
//!
//! # RoutePlan 结构
//!
//! 解析后的配置被转换为一系列可执行的操作：
//! - `route_ops`: 需要添加的路由
//! - `policy_rule_ops`: 需要添加的策略路由规则
//! - `metric_ops`: 路由度量设置
//! - `bypass_ops`: 旁路规则（Windows）

mod apply_report;
mod ids;
mod normalize;
mod planner;
#[cfg(test)]
mod tests;
mod util;

use std::net::IpAddr;

use serde::{Deserialize, Serialize};

use crate::core::config::{AllowedIp, RouteTable};

pub use apply_report::{
    RouteApplyAttemptState, RouteApplyEntry, RouteApplyFailureKind, RouteApplyKind,
    RouteApplyPhase, RouteApplyReport, RouteApplyReportSource, RouteApplyStatus,
};
pub use normalize::{
    collect_allowed_routes, detect_full_tunnel, linux_default_policy_table_id,
    linux_policy_table_id, linux_route_table_for, normalize_config_for_runtime,
};
pub use planner::windows_planned_bypass_count;

/// 全隧道场景下自动补齐的默认 fwmark
///
/// 当配置中没有指定 fwmark 但需要全隧道时，会自动使用此值。
pub const DEFAULT_FULL_TUNNEL_FWMARK: u32 = 0x5257;

/// Linux 全隧道时默认使用的 policy table ID
pub const LINUX_DEFAULT_POLICY_TABLE_ID: u32 = 200;

/// 路由规划的目标平台
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoutePlanPlatform {
    /// Linux 平台（使用 netlink）
    Linux,
    /// Windows 平台（使用 Windows API）
    Windows,
    /// 其他平台（不支持）
    Other,
}

impl RoutePlanPlatform {
    /// 获取当前运行平台
    pub fn current() -> Self {
        if cfg!(target_os = "linux") {
            Self::Linux
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Other
        }
    }

    /// 获取平台名称字符串
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Windows => "windows",
            Self::Other => "other",
        }
    }
}

/// 全隧道判定结果
///
/// 记录 IPv4 和 IPv6 是否为全隧道模式。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FullTunnelStatus {
    pub ipv4: bool,
    pub ipv6: bool,
}

impl FullTunnelStatus {
    /// 是否任一协议为全隧道
    pub fn any(self) -> bool {
        self.ipv4 || self.ipv6
    }

    /// 判断给定地址是否匹配全隧道
    pub fn matches(self, addr: IpAddr) -> bool {
        match addr {
            IpAddr::V4(_) => self.ipv4,
            IpAddr::V6(_) => self.ipv6,
        }
    }
}

/// 路由类型
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoutePlanRouteKind {
    /// 正常的 AllowedIPs 路由
    Allowed,
    /// DNS 主机路由
    DnsHost,
}

/// IP 协议族
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoutePlanFamily {
    Ipv4,
    Ipv6,
}

impl RoutePlanFamily {
    /// 获取标签字符串
    pub fn label(self) -> &'static str {
        match self {
            Self::Ipv4 => "IPv4",
            Self::Ipv6 => "IPv6",
        }
    }
}

/// 单个路由操作
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutePlanRouteOp {
    /// 路由目标（IP/CIDR）
    pub route: AllowedIp,
    /// 路由表 ID（Linux 策略路由）
    pub table_id: Option<u32>,
    /// 路由类型
    pub kind: RoutePlanRouteKind,
}

/// 策略路由规则操作
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutePlanPolicyRuleOp {
    /// 协议族
    pub family: RoutePlanFamily,
    /// 目标路由表 ID
    pub table_id: u32,
    /// fwmark 值
    pub fwmark: u32,
}

/// 路由度量操作
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutePlanMetricOp {
    pub family: RoutePlanFamily,
    pub metric: u32,
}

/// 旁路操作（Windows 特有）
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutePlanBypassOp {
    pub host: String,
    pub port: u16,
}

/// 完整的路由规划
///
/// 包含将 WireGuard 配置转换为系统网络操作所需的所有信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutePlan {
    /// 目标平台
    pub platform: RoutePlanPlatform,
    /// 用户请求的路由表（wg-quick Table 字段）
    pub requested_table: Option<RouteTable>,
    /// 解析后的 AllowedIPs 列表
    pub allowed_routes: Vec<AllowedIp>,
    /// 全隧道状态
    pub full_tunnel: FullTunnelStatus,
    /// Linux 策略路由表 ID
    pub linux_policy_table_id: Option<u32>,
    /// 需要添加的路由操作
    pub route_ops: Vec<RoutePlanRouteOp>,
    /// 需要添加的策略路由规则操作
    pub policy_rule_ops: Vec<RoutePlanPolicyRuleOp>,
    /// 路由度量操作
    pub metric_ops: Vec<RoutePlanMetricOp>,
    /// 旁路操作（Windows）
    pub bypass_ops: Vec<RoutePlanBypassOp>,
    /// Windows 旁路数量统计
    pub windows_planned_bypass_count: usize,
}

/// RoutePlan 的别名（用于语义区分）
pub type OperationalRoutePlan = RoutePlan;
