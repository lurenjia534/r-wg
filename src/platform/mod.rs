//! 平台网络配置的统一入口。
//!
//! 说明：
//! - 该模块只负责“按平台分发”到具体实现（linux/windows）。
//! - 上层（引擎/UI）只调用这里的 `apply_network_config` / `cleanup_network_config`。
//! - 非目标平台会编译为占位实现，避免引入不必要依赖。

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;

use crate::core::config::WireGuardConfig;
use crate::core::route_plan::{RouteApplyReport, RoutePlan};

/// Linux 平台：直接复用 linux 模块的状态与错误类型。
#[cfg(target_os = "linux")]
pub use linux::{AppliedNetworkState as NetworkState, NetworkError};

/// Windows 平台：直接复用 windows 模块的状态与错误类型。
#[cfg(target_os = "windows")]
pub use windows::{AppliedNetworkState as NetworkState, NetworkError};

/// 其他平台：占位状态类型（仅用于保持 API 形状一致）。
#[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
#[derive(Debug)]
pub struct NetworkState;

pub struct NetworkApplyResult {
    pub state: NetworkState,
    pub report: RouteApplyReport,
}

pub struct NetworkApplyError {
    pub error: NetworkError,
    pub report: RouteApplyReport,
}

#[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
impl From<NetworkError> for NetworkApplyError {
    fn from(error: NetworkError) -> Self {
        Self {
            error,
            report: RouteApplyReport::default(),
        }
    }
}

/// 其他平台：占位错误类型（提示未实现）。
#[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
#[derive(Debug)]
pub enum NetworkError {
    Unsupported,
}

#[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
impl std::fmt::Display for NetworkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetworkError::Unsupported => write!(f, "platform network config is not supported"),
        }
    }
}

#[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
impl std::error::Error for NetworkError {}

/// 应用系统网络配置。
/// - Linux/Windows：调用各自平台实现。
/// - 其他平台：no-op，占位返回。
#[cfg(target_os = "linux")]
pub async fn apply_network_config(
    tun_name: &str,
    config: &WireGuardConfig,
    route_plan: &RoutePlan,
) -> Result<NetworkApplyResult, NetworkApplyError> {
    linux::apply_network_config(tun_name, config, route_plan).await
}

/// Windows 平台的网络配置入口。
#[cfg(target_os = "windows")]
pub async fn apply_network_config(
    tun_name: &str,
    config: &WireGuardConfig,
    route_plan: &RoutePlan,
) -> Result<NetworkApplyResult, NetworkApplyError> {
    windows::apply_network_config(tun_name, config, route_plan).await
}

/// 其他平台：占位实现。
#[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
pub async fn apply_network_config(
    _tun_name: &str,
    _config: &WireGuardConfig,
    _route_plan: &RoutePlan,
) -> Result<NetworkApplyResult, NetworkApplyError> {
    Ok(NetworkApplyResult {
        state: NetworkState,
        report: RouteApplyReport::default(),
    })
}

/// 回滚系统网络配置。
/// - Linux/Windows：调用各自平台实现。
/// - 其他平台：no-op。
#[cfg(target_os = "linux")]
pub async fn cleanup_network_config(state: NetworkState) -> Result<(), NetworkError> {
    linux::cleanup_network_config(state).await
}

/// Windows 平台的回滚入口。
#[cfg(target_os = "windows")]
pub async fn cleanup_network_config(state: NetworkState) -> Result<(), NetworkError> {
    windows::cleanup_network_config(state).await
}

/// 其他平台：占位实现。
#[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
pub async fn cleanup_network_config(_state: NetworkState) -> Result<(), NetworkError> {
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn load_persisted_apply_report() -> Option<RouteApplyReport> {
    linux::load_persisted_apply_report()
}

#[cfg(target_os = "windows")]
pub fn load_persisted_apply_report() -> Option<RouteApplyReport> {
    windows::load_persisted_apply_report()
}

#[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
pub fn load_persisted_apply_report() -> Option<RouteApplyReport> {
    None
}
