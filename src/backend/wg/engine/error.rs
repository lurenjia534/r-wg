use std::fmt;

use gotatun::device;

use crate::core::config::ConfigError;
use crate::platform;

/// 引擎错误类型。
/// 仅涵盖启动/停止/状态查询相关的错误面。
#[derive(Debug)]
pub enum EngineError {
    /// 后台命令通道已关闭（通常是后台线程退出或崩溃）。
    ChannelClosed,
    /// 重复启动。
    AlreadyRunning,
    /// 未启动却请求停止或其它操作。
    NotRunning,
    /// 调用方无权访问特权后端。
    AccessDenied,
    /// gotatun 设备层错误（如 TUN 创建失败）。
    Device(device::Error),
    /// WireGuard 配置错误（解析或字段非法）。
    Config(ConfigError),
    /// 系统网络配置错误（地址/路由/DNS 应用失败）。
    Network(platform::NetworkError),
    /// Linux kernel WireGuard 控制面错误。
    KernelWireGuard(String),
    /// 请求的 WireGuard backend 当前不可用或与所选功能冲突。
    UnsupportedBackend(String),
    /// Ephemeral peer 协商或重配置失败。
    Ephemeral(String),
    /// UI 与特权后端协议版本不一致。
    VersionMismatch { expected: u32, actual: u32 },
    /// Windows 提权 helper / IPC 层返回的文本错误。
    Remote(String),
}

/// 将错误转换为可读文本，便于上层日志与提示。
impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::ChannelClosed => write!(f, "backend channel closed"),
            EngineError::AlreadyRunning => write!(f, "backend already running"),
            EngineError::NotRunning => write!(f, "backend not running"),
            EngineError::AccessDenied => write!(f, "access denied to privileged backend"),
            EngineError::Device(err) => write!(f, "device error: {err}"),
            EngineError::Config(err) => write!(f, "config error: {err}"),
            EngineError::Network(err) => write!(f, "network error: {err}"),
            EngineError::KernelWireGuard(message) => {
                write!(f, "kernel WireGuard error: {message}")
            }
            EngineError::UnsupportedBackend(message) => {
                write!(f, "unsupported WireGuard backend: {message}")
            }
            EngineError::Ephemeral(message) => write!(f, "ephemeral negotiation error: {message}"),
            EngineError::VersionMismatch { expected, actual } => write!(
                f,
                "privileged backend protocol mismatch (expected v{expected}, got v{actual})"
            ),
            EngineError::Remote(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for EngineError {}

/// 设备层错误 -> 引擎错误。
impl From<device::Error> for EngineError {
    fn from(err: device::Error) -> Self {
        EngineError::Device(err)
    }
}

impl From<ConfigError> for EngineError {
    fn from(err: ConfigError) -> Self {
        EngineError::Config(err)
    }
}

impl From<platform::NetworkError> for EngineError {
    fn from(err: platform::NetworkError) -> Self {
        EngineError::Network(err)
    }
}
