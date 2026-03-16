//! Windows 接口 metric 调整与回滚。
//!
//! 在全隧道模式下，通常需要降低隧道网卡的 metric，
//! 以提升其默认路由优先级，减少系统走物理网卡造成的泄露风险。

use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::NO_ERROR;
use windows::Win32::NetworkManagement::IpHelper::{
    GetIpInterfaceEntry, InitializeIpInterfaceEntry, SetIpInterfaceEntry, MIB_IPINTERFACE_ROW,
};
use windows::Win32::Networking::WinSock::{ADDRESS_FAMILY, AF_INET, AF_INET6};

use super::adapter::AdapterInfo;
use super::NetworkError;

/// 用于回滚 metric 的快照。
#[derive(Clone, Copy)]
pub(super) struct InterfaceMetricState {
    /// 地址族（IPv4 / IPv6）。
    family: ADDRESS_FAMILY,
    /// 原始是否自动 metric。
    use_auto: bool,
    /// 原始 metric 值。
    metric: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct InterfaceMetricSnapshot {
    family: u16,
    use_auto: bool,
    metric: u32,
}

impl From<InterfaceMetricState> for InterfaceMetricSnapshot {
    fn from(state: InterfaceMetricState) -> Self {
        Self {
            family: state.family.0,
            use_auto: state.use_auto,
            metric: state.metric,
        }
    }
}

impl InterfaceMetricSnapshot {
    pub(super) fn to_state(&self) -> InterfaceMetricState {
        InterfaceMetricState {
            family: ADDRESS_FAMILY(self.family),
            use_auto: self.use_auto,
            metric: self.metric,
        }
    }
}

/// 设置指定地址族的接口 metric，并返回“设置前”的状态快照。
pub(super) fn set_interface_metric(
    adapter: AdapterInfo,
    family: ADDRESS_FAMILY,
    metric: u32,
) -> Result<InterfaceMetricState, NetworkError> {
    // 先读取当前状态，后续用于回滚。
    let mut row: MIB_IPINTERFACE_ROW = unsafe { std::mem::zeroed() };
    unsafe {
        InitializeIpInterfaceEntry(&mut row);
    }
    row.Family = family;
    row.InterfaceLuid = adapter.luid;
    row.InterfaceIndex = adapter.if_index;

    let result = unsafe { GetIpInterfaceEntry(&mut row) };
    if result != NO_ERROR {
        return Err(NetworkError::Win32 {
            context: "GetIpInterfaceEntry",
            code: result,
        });
    }

    let previous = InterfaceMetricState {
        family,
        use_auto: row.UseAutomaticMetric,
        metric: row.Metric,
    };

    // 写入目标值：关闭自动 metric，使用固定 metric。
    row.UseAutomaticMetric = false;
    row.Metric = metric;

    // 修正结构体字段，避免 SetIpInterfaceEntry 参数校验失败。
    sanitize_site_prefix_length(family, &mut row);

    let result = unsafe { SetIpInterfaceEntry(&mut row) };
    if result != NO_ERROR {
        return Err(NetworkError::Win32 {
            context: "SetIpInterfaceEntry",
            code: result,
        });
    }

    Ok(previous)
}

/// 根据快照恢复接口 metric。
pub(super) fn restore_interface_metric(
    adapter: AdapterInfo,
    state: InterfaceMetricState,
) -> Result<(), NetworkError> {
    let mut row: MIB_IPINTERFACE_ROW = unsafe { std::mem::zeroed() };
    unsafe {
        InitializeIpInterfaceEntry(&mut row);
    }
    row.Family = state.family;
    row.InterfaceLuid = adapter.luid;
    row.InterfaceIndex = adapter.if_index;

    let result = unsafe { GetIpInterfaceEntry(&mut row) };
    if result != NO_ERROR {
        return Err(NetworkError::Win32 {
            context: "GetIpInterfaceEntry",
            code: result,
        });
    }

    row.UseAutomaticMetric = state.use_auto;
    row.Metric = state.metric;

    sanitize_site_prefix_length(state.family, &mut row);

    let result = unsafe { SetIpInterfaceEntry(&mut row) };
    if result != NO_ERROR {
        return Err(NetworkError::Win32 {
            context: "SetIpInterfaceEntry",
            code: result,
        });
    }

    Ok(())
}

/// 兼容性修正：
/// - IPv4 时 SitePrefixLength 应为 0，否则某些系统会报 87；
/// - IPv6 时做上限保护（0..=128）。
fn sanitize_site_prefix_length(family: ADDRESS_FAMILY, row: &mut MIB_IPINTERFACE_ROW) {
    if family == AF_INET {
        row.SitePrefixLength = 0;
    } else if family == AF_INET6 && row.SitePrefixLength > 128 {
        row.SitePrefixLength = 128;
    }
}
