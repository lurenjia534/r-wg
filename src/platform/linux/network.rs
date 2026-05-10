//! Linux 网络配置入口模块
//!
//! 本模块负责在 Linux 系统上应用 WireGuard 隧道所需的网络配置。
//!
//! # 配置内容
//!
//! - **接口地址**: 为 TUN 设备分配 IP 地址
//! - **路由**: 添加AllowedIPs路由，决定哪些流量经过隧道
//! - **DNS**: 配置隧道内的 DNS 服务器和搜索域
//! - **策略路由**: 全隧道模式下使用 fwmark + 策略路由强制所有流量走隧道
//!
//! # 恢复机制
//!
//! 崩溃恢复：应用配置前会写入 journal，崩溃后可自动回滚或修复。
//!
//! # 错误处理
//!
//! 配置失败会立即回滚所有已应用的更改，确保系统状态不会停留在不一致状态。

mod apply_pipeline;
mod apply_plan;
mod cleanup_pipeline;
mod dns;
mod killswitch;
mod logging;
mod netlink;
mod policy;
mod recovery;
mod routes;
mod stages;

use crate::core::config::{InterfaceAddress, RouteTable, WireGuardConfig};
use crate::core::route_plan::{RouteApplyReport, RoutePlan, RoutePlanRouteOp};
use crate::platform::{NetworkApplyError, NetworkApplyResult};

use dns::DnsState;
pub use killswitch::QuantumNegotiationGuardState;
use killswitch::{
    apply_quantum_negotiation_guard, cleanup_stale_quantum_negotiation_guard, KillSwitchState,
};
use policy::PolicyRoutingState;
use recovery::{
    attempt_startup_repair_sync,
    load_persisted_apply_report as load_persisted_apply_report_from_disk,
};

// 默认 policy routing 表号：不干扰 main table，且便于排查。
const DEFAULT_POLICY_TABLE: u32 = 200;

#[derive(Debug)]
pub struct AppliedNetworkState {
    tun_name: String,
    addresses: Vec<InterfaceAddress>,
    routes: Vec<RoutePlanRouteOp>,
    table: Option<RouteTable>,
    dns: Option<DnsState>,
    policy: Option<PolicyRoutingState>,
    kill_switch: Option<KillSwitchState>,
}

#[derive(Debug)]
pub enum NetworkError {
    Io(std::io::Error),
    Netlink(rtnetlink::Error),
    CommandFailed {
        command: String,
        status: Option<i32>,
        stderr: String,
    },
    DnsVerifyFailed(String),
    DnsNotSupported,
    LinkNotFound(String),
    MissingFwmark,
    KillSwitchUnavailable(String),
}

impl std::fmt::Display for NetworkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetworkError::Io(err) => write!(f, "io error: {err}"),
            NetworkError::CommandFailed {
                command,
                status,
                stderr,
            } => write!(f, "command failed: {command} (status={status:?}) {stderr}"),
            NetworkError::DnsVerifyFailed(message) => {
                write!(f, "dns verification failed: {message}")
            }
            NetworkError::DnsNotSupported => write!(f, "no supported DNS backend found"),
            NetworkError::Netlink(err) => write!(f, "netlink error: {err}"),
            NetworkError::LinkNotFound(name) => write!(f, "link not found: {name}"),
            NetworkError::MissingFwmark => write!(f, "missing fwmark for policy routing"),
            NetworkError::KillSwitchUnavailable(message) => {
                write!(f, "kill switch unavailable: {message}")
            }
        }
    }
}

impl std::error::Error for NetworkError {}

impl From<std::io::Error> for NetworkError {
    fn from(err: std::io::Error) -> Self {
        NetworkError::Io(err)
    }
}

impl From<rtnetlink::Error> for NetworkError {
    fn from(err: rtnetlink::Error) -> Self {
        NetworkError::Netlink(err)
    }
}

pub async fn apply_network_config(
    tun_name: &str,
    config: &WireGuardConfig,
    route_plan: &RoutePlan,
    kill_switch_enabled: bool,
) -> Result<NetworkApplyResult, NetworkApplyError> {
    apply_pipeline::apply_network_config(tun_name, config, route_plan, kill_switch_enabled).await
}

/// 清理之前应用的网络配置。
pub async fn cleanup_network_config(state: AppliedNetworkState) -> Result<(), NetworkError> {
    cleanup_pipeline::cleanup_network_config(state).await
}

pub async fn apply_quantum_negotiation_traffic_guard(
    tun_name: &str,
    block_ipv6: bool,
) -> Result<QuantumNegotiationGuardState, NetworkError> {
    apply_quantum_negotiation_guard(tun_name, block_ipv6).await
}

pub async fn cleanup_quantum_negotiation_traffic_guard(
    state: QuantumNegotiationGuardState,
) -> Result<(), NetworkError> {
    state.cleanup().await
}

pub fn cleanup_stale_quantum_negotiation_traffic_guard_sync() -> Result<(), NetworkError> {
    let runtime = tokio::runtime::Runtime::new().map_err(NetworkError::Io)?;
    runtime.block_on(async { cleanup_stale_quantum_negotiation_guard().await })
}

pub fn attempt_startup_repair() -> Result<(), NetworkError> {
    attempt_startup_repair_sync()
}

pub fn load_persisted_apply_report() -> Option<RouteApplyReport> {
    load_persisted_apply_report_from_disk()
        .ok()
        .flatten()
        .map(|mut report| {
            report.mark_persisted();
            report
        })
}
