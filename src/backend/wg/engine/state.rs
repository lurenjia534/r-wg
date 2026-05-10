use gotatun::device::{DefaultDeviceTransports, Device};

use crate::core::route_plan::RouteApplyReport;
use crate::platform;

use super::super::ephemeral::EphemeralFailureKind;
#[cfg(target_os = "linux")]
use super::super::linux_kernel::KernelWireGuardDevice;
use super::ActiveBackendStatus;

/// 后台线程维护的运行态状态。
///
/// 该状态只在后台线程内访问，保证串行一致性。
#[derive(Default)]
pub(super) struct EngineState {
    /// 暂停复用的 GotaTun 设备；只在未运行时缓存。
    pub(super) cached_userspace_device: Option<DeviceSlot>,
    /// 当前运行中的 WireGuard 数据面实现。
    pub(super) active_backend: Option<ActiveWireGuardBackend>,
    /// 系统网络配置状态，用于停止时回滚。
    pub(super) net_state: Option<platform::NetworkState>,
    /// 最近一次成功应用的结构化报告。
    pub(super) route_apply_report: Option<RouteApplyReport>,
    /// 是否处于“已启动并生效”的状态。
    pub(super) running: bool,
    /// 当前运行态是否使用了量子升级后的临时本地密钥。
    pub(super) quantum_active: bool,
    /// 当前运行态是否启用了 DAITA。
    pub(super) daita_active: bool,
    /// 当前运行态是否使用了 ephemeral 临时私钥。
    pub(super) ephemeral_key_active: bool,
    /// 最近一次量子升级失败分类。
    pub(super) last_quantum_failure: Option<EphemeralFailureKind>,
    /// 最近一次 DAITA 协商失败分类。
    pub(super) last_daita_failure: Option<EphemeralFailureKind>,
}

/// 缓存的 gotatun 设备与其 TUN 名称。
pub(super) struct DeviceSlot {
    pub(super) device: Device<DefaultDeviceTransports>,
    pub(super) tun_name: String,
}

/// 当前运行中的后端，确保运行态只由一个具体数据面持有。
pub(super) enum ActiveWireGuardBackend {
    Userspace(DeviceSlot),
    #[cfg(target_os = "linux")]
    LinuxKernel(KernelWireGuardDevice),
}

impl ActiveWireGuardBackend {
    pub(super) fn status(&self) -> ActiveBackendStatus {
        match self {
            Self::Userspace(_) => ActiveBackendStatus::UserspaceGotaTun,
            #[cfg(target_os = "linux")]
            Self::LinuxKernel(_) => ActiveBackendStatus::LinuxKernel,
        }
    }
}
