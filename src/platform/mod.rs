//! 平台层统一入口。
//!
//! 目标是屏蔽各平台网络配置差异，并为上层提供一致的接口：
//! `apply_network_config` / `cleanup_network_config`。
//!
//! - Linux：真实实现（地址/路由/DNS）。
//! - macOS/Windows：当前保持空实现占位，后续补齐。

pub mod linux;
pub mod macos;
pub mod windows;

use crate::backend::wg::config::{InterfaceConfig, PeerConfig};

/// Linux 平台的真实状态与错误类型直接复用，避免重复定义。
#[cfg(target_os = "linux")]
pub use linux::{AppliedNetworkState as NetworkState, NetworkError};

/// 非 Linux 平台的占位状态类型。
///
/// 目前不会携带任何实际配置状态，仅用于保持 API 形状一致。
#[cfg(not(target_os = "linux"))]
#[derive(Debug)]
pub struct NetworkState;

/// 非 Linux 平台的占位错误类型。
///
/// 目前只有 Unsupported，用于提醒上层功能尚未实现。
#[cfg(not(target_os = "linux"))]
#[derive(Debug)]
pub enum NetworkError {
    Unsupported,
}

#[cfg(not(target_os = "linux"))]
impl std::fmt::Display for NetworkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetworkError::Unsupported => write!(f, "platform network config is not supported"),
        }
    }
}

#[cfg(not(target_os = "linux"))]
impl std::error::Error for NetworkError {}

/// 应用系统网络配置。
///
/// Linux 上执行真实配置；其他平台暂时为 no-op，返回占位状态。
#[cfg(target_os = "linux")]
pub async fn apply_network_config(
    tun_name: &str,
    interface: &InterfaceConfig,
    peers: &[PeerConfig],
) -> Result<NetworkState, NetworkError> {
    linux::apply_network_config(tun_name, interface, peers).await
}

#[cfg(not(target_os = "linux"))]
pub async fn apply_network_config(
    _tun_name: &str,
    _interface: &InterfaceConfig,
    _peers: &[PeerConfig],
) -> Result<NetworkState, NetworkError> {
    Ok(NetworkState)
}

/// 回滚系统网络配置。
///
/// Linux 上执行真实清理；其他平台暂时为 no-op。
#[cfg(target_os = "linux")]
pub async fn cleanup_network_config(state: NetworkState) -> Result<(), NetworkError> {
    linux::cleanup_network_config(state).await
}

#[cfg(not(target_os = "linux"))]
pub async fn cleanup_network_config(_state: NetworkState) -> Result<(), NetworkError> {
    Ok(())
}
