use tokio::sync::oneshot;

use crate::core::route_plan::RouteApplyReport;

use super::{
    EngineError, EngineRuntimeSnapshot, EngineStats, EngineStatus, RelayInventoryStatusSnapshot,
    StartRequest,
};

/// Backend thread command protocol.
///
/// `Engine` owns the synchronous request bridge while the backend thread owns
/// command execution. Keeping the command envelope separate makes later engine
/// pipeline splits less coupled to the public `Engine` facade.
pub(super) enum Command {
    /// 启动引擎并回传结果（成功/失败）。
    Start(StartRequest, oneshot::Sender<Result<(), EngineError>>),
    /// 停止引擎并回传结果（成功/失败）。
    Stop(oneshot::Sender<Result<(), EngineError>>),
    /// 查询状态（仅返回 Running/Stopped）。
    Status(oneshot::Sender<EngineStatus>),
    /// 查询 gotatun 运行时统计信息。
    Stats(oneshot::Sender<Result<EngineStats, EngineError>>),
    /// 查询最近一次结构化路由应用报告。
    ApplyReport(oneshot::Sender<Option<RouteApplyReport>>),
    /// 查询包含量子状态的完整运行时快照。
    RuntimeSnapshot(oneshot::Sender<EngineRuntimeSnapshot>),
    /// 查询缓存的 Mullvad relay inventory 状态。
    RelayInventoryStatus(oneshot::Sender<Result<RelayInventoryStatusSnapshot, EngineError>>),
    /// 下载并刷新缓存的 Mullvad relay inventory。
    RefreshRelayInventory(oneshot::Sender<Result<RelayInventoryStatusSnapshot, EngineError>>),
}
