use std::net::SocketAddr;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::core::dns::DnsSelection;
use crate::core::route_plan::RouteApplyReport;

use super::super::ephemeral::{DaitaMode, EphemeralFailureKind, QuantumMode};

/// 引擎状态：是否处于“已启动并生效”状态。
///
/// Running 代表网络配置已应用且设备可用；Stopped 则表示未启动或已停止（设备可能被缓存复用）。
/// Windows helper 模式下，这个状态会回传给普通权限 UI，用来恢复开关状态与按钮文案。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EngineStatus {
    /// 未启动。
    Stopped,
    /// 已启动。
    Running,
}

/// 当前运行中的 WireGuard 数据面实现。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActiveBackendStatus {
    /// Linux kernel WireGuard interface.
    LinuxKernel,
    /// GotaTun userspace WireGuard device.
    UserspaceGotaTun,
}

impl ActiveBackendStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::LinuxKernel => "Linux Kernel",
            Self::UserspaceGotaTun => "GotaTun",
        }
    }
}

/// 引擎运行时快照。
///
/// 用于 UI/IPC 恢复展示，包含基础运行态、最近一次路由应用结果，以及 ephemeral 协商状态。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineRuntimeSnapshot {
    pub status: EngineStatus,
    #[serde(default)]
    pub active_backend: Option<ActiveBackendStatus>,
    pub apply_report: Option<RouteApplyReport>,
    pub quantum_protected: bool,
    pub last_quantum_failure: Option<EphemeralFailureKind>,
    pub daita_active: bool,
    pub last_daita_failure: Option<EphemeralFailureKind>,
}

/// DAITA 资源缓存状态快照。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayInventoryStatusSnapshot {
    pub cache_path: String,
    pub present: bool,
    pub relay_count: usize,
    pub daita_relay_count: usize,
    pub fetched_at_unix_secs: Option<u64>,
}

/// gotatun 设备的运行时统计信息。
/// 这些数据会在 Windows helper 模式下通过 IPC 返回给 UI，驱动统计面板刷新。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineStats {
    pub peers: Vec<PeerStats>,
}

/// 单个 Peer 的 DAITA 统计快照。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaitaStats {
    pub tx_padding_bytes: u64,
    pub rx_padding_bytes: u64,
    pub tx_decoy_packet_bytes: u64,
    pub rx_decoy_packet_bytes: u64,
}

/// 单个 Peer 的状态快照。
/// 字段保持简单扁平，便于直接序列化后跨进程回传。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerStats {
    pub public_key: [u8; 32],
    pub endpoint: Option<SocketAddr>,
    /// 上次握手距现在的时间。backend 必须在填充前把原生时间戳转换成 age。
    pub last_handshake: Option<Duration>,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub daita: Option<DaitaStats>,
}

/// 启动请求：包含 TUN 设备名称与配置文本。
///
/// 之所以传入完整配置文本，是为了让引擎在后台线程内完成解析与应用，
/// 避免 UI 线程阻塞或出现跨线程的生命周期管理问题。
/// 在 Windows 按需提权模式下，这个结构还会通过 IPC 从普通权限 UI 发送给管理员 helper，
/// 因此字段语义需要稳定且可序列化。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartRequest {
    /// TUN 设备名称，例如 "wg0"。
    pub tun_name: String,
    /// WireGuard 配置文本（含 wg-quick 兼容字段）。
    pub config_text: String,
    /// DNS 选择（全局 UI 状态传入）。
    pub dns: DnsSelection,
    /// 量子抗性隧道升级模式。
    pub quantum_mode: QuantumMode,
    /// DAITA 模式。
    pub daita_mode: DaitaMode,
    /// 是否启用 Kill switch。
    pub kill_switch_enabled: bool,
    /// WireGuard 数据面实现偏好。
    #[serde(default)]
    pub wireguard_backend_preference: WireGuardBackendPreference,
}

impl StartRequest {
    /// 便捷构造器。
    pub fn new(
        tun_name: impl Into<String>,
        config_text: impl Into<String>,
        dns: DnsSelection,
        quantum_mode: QuantumMode,
        daita_mode: DaitaMode,
        kill_switch_enabled: bool,
        wireguard_backend_preference: WireGuardBackendPreference,
    ) -> Self {
        Self {
            tun_name: tun_name.into(),
            config_text: config_text.into(),
            dns,
            quantum_mode,
            daita_mode,
            kill_switch_enabled,
            wireguard_backend_preference,
        }
    }
}

/// WireGuard 数据面实现偏好。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WireGuardBackendPreference {
    /// 自动选择；Linux 后续优先 kernel，不可用时回退 GotaTun。
    #[cfg_attr(target_os = "linux", default)]
    Auto,
    /// 明确要求 Linux kernel WireGuard。
    Kernel,
    /// 明确要求用户态 GotaTun。
    #[cfg_attr(not(target_os = "linux"), default)]
    Userspace,
}

impl WireGuardBackendPreference {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Kernel => "Kernel",
            Self::Userspace => "UserspaceGotaTun",
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::dns::{DnsMode, DnsPreset};

    use super::*;

    #[test]
    fn wireguard_backend_preference_defaults_by_platform() {
        #[cfg(target_os = "linux")]
        assert_eq!(
            WireGuardBackendPreference::default(),
            WireGuardBackendPreference::Auto
        );
        #[cfg(not(target_os = "linux"))]
        assert_eq!(
            WireGuardBackendPreference::default(),
            WireGuardBackendPreference::Userspace
        );
    }

    #[test]
    fn start_request_deserializes_missing_backend_preference() {
        let json = r#"{
            "tun_name": "wg0",
            "config_text": "[Interface]\n",
            "dns": {"mode": "FollowConfig", "preset": "CloudflareStandard"},
            "quantum_mode": "off",
            "daita_mode": "off",
            "kill_switch_enabled": true
        }"#;

        let request: StartRequest =
            serde_json::from_str(json).expect("legacy request should deserialize");

        assert_eq!(
            request.wireguard_backend_preference,
            WireGuardBackendPreference::default()
        );
    }

    #[test]
    fn start_request_constructor_carries_backend_preference() {
        let request = StartRequest::new(
            "wg0",
            "[Interface]\n",
            DnsSelection::new(DnsMode::FollowConfig, DnsPreset::CloudflareStandard),
            QuantumMode::Off,
            DaitaMode::Off,
            true,
            WireGuardBackendPreference::Kernel,
        );

        assert_eq!(
            request.wireguard_backend_preference,
            WireGuardBackendPreference::Kernel
        );
    }
}
