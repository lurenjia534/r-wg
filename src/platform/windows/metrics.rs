//! 接口 metric 调整与回滚。
//!
//! 在全隧道场景下，降低 TUN 接口 metric 以获得更高路由优先级。

use windows::Win32::Foundation::NO_ERROR;
use windows::Win32::NetworkManagement::IpHelper::{
    GetIpInterfaceEntry, InitializeIpInterfaceEntry, SetIpInterfaceEntry, MIB_IPINTERFACE_ROW,
};
use windows::Win32::Networking::WinSock::ADDRESS_FAMILY;

use super::adapter::AdapterInfo;
use super::NetworkError;

#[derive(Clone, Copy)]
pub(super) struct InterfaceMetricState {
    /// 地址族（IPv4/IPv6）。
    family: ADDRESS_FAMILY,
    /// 是否使用系统自动 metric。
    use_auto: bool,
    /// 原始 metric 值（用于回滚）。
    metric: u32,
}

pub(super) fn set_interface_metric(
    adapter: AdapterInfo,
    family: ADDRESS_FAMILY,
    metric: u32,
) -> Result<InterfaceMetricState, NetworkError> {
    // 先读当前设置，保存旧值以便恢复。
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

    // 关闭自动 metric，设置固定优先级。
    row.UseAutomaticMetric = false;
    row.Metric = metric;

    let result = unsafe { SetIpInterfaceEntry(&mut row) };
    if result != NO_ERROR {
        return Err(NetworkError::Win32 {
            context: "SetIpInterfaceEntry",
            code: result,
        });
    }

    Ok(previous)
}

pub(super) fn restore_interface_metric(
    adapter: AdapterInfo,
    state: InterfaceMetricState,
) -> Result<(), NetworkError> {
    // 根据保存的旧值恢复接口 metric。
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

    let result = unsafe { SetIpInterfaceEntry(&mut row) };
    if result != NO_ERROR {
        return Err(NetworkError::Win32 {
            context: "SetIpInterfaceEntry",
            code: result,
        });
    }

    Ok(())
}
