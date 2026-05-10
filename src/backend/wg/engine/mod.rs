//! WireGuard 后端引擎：
//! - 在独立线程中创建 tokio runtime，避免占用 UI 线程或依赖外部运行时。
//! - 通过 MPSC 命令通道驱动 gotatun 设备的生命周期，确保串行执行。
//! - 对外提供同步 API（start/stop/status），内部异步执行并做清理回滚。
mod backend_strategy;
mod command;
mod error;
mod runner;
mod snapshot;
mod start_pipeline;
mod state;
mod stop_pipeline;
mod types;

use std::sync::Arc;

use tokio::sync::{mpsc, oneshot};

use crate::core::route_plan::RouteApplyReport;

use self::command::Command;
pub use self::error::EngineError;
use self::runner::run;
pub use self::types::{
    ActiveBackendStatus, DaitaStats, EngineRuntimeSnapshot, EngineStats, EngineStatus, PeerStats,
    RelayInventoryStatusSnapshot, StartRequest, WireGuardBackendPreference,
};

/// 前端/调用方持有的引擎句柄。
///
/// 内部通过 Arc 共享命令发送端，避免多处 clone 造成状态分裂。
#[derive(Clone)]
pub struct Engine {
    inner: Arc<EngineInner>,
}

/// 引擎内部共享数据，目前只有命令发送端。
///
/// 未来如需添加全局配置或诊断信息，可扩展到这里。
struct EngineInner {
    tx: mpsc::Sender<Command>,
}

impl Engine {
    /// 创建引擎：初始化通道并启动后台线程 + tokio runtime。
    ///
    /// 后台线程只处理引擎命令，避免阻塞 UI 线程或依赖外部 runtime。
    pub fn new() -> Self {
        // 后台命令通道。
        let (tx, rx) = mpsc::channel(16);
        std::thread::Builder::new()
            .name("wg-backend".to_string())
            .spawn(move || {
                // 独立 runtime，避免与 UI 线程/其它 runtime 交叉干扰。
                let runtime =
                    tokio::runtime::Runtime::new().expect("failed to create backend runtime");
                runtime.block_on(async move {
                    run(rx).await;
                });
            })
            .expect("failed to spawn backend thread");

        Self {
            inner: Arc::new(EngineInner { tx }),
        }
    }

    fn request<T>(
        &self,
        command: impl FnOnce(oneshot::Sender<T>) -> Command,
    ) -> Result<T, EngineError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.inner
            .tx
            .blocking_send(command(reply_tx))
            .map_err(|_| EngineError::ChannelClosed)?;
        reply_rx
            .blocking_recv()
            .map_err(|_| EngineError::ChannelClosed)
    }

    /// 同步启动接口：
    /// - 通过 blocking_send 把命令送入后台。
    /// - 用 oneshot 等待后台完成并回传结果。
    ///
    /// 上层无需持有 runtime，也不需要 async/await。
    pub fn start(&self, request: StartRequest) -> Result<(), EngineError> {
        self.request(|reply_tx| Command::Start(request, reply_tx))?
    }

    /// 同步停止接口。
    ///
    /// 停止会触发网络配置回滚，并释放 gotatun 设备句柄。
    pub fn stop(&self) -> Result<(), EngineError> {
        self.request(Command::Stop)?
    }

    /// 同步状态查询接口。
    ///
    /// 返回的是“已启动/未启动”，不包含更细粒度的统计信息。
    pub fn status(&self) -> Result<EngineStatus, EngineError> {
        self.request(Command::Status)
    }

    /// 同步获取 gotatun 运行时统计信息。
    pub fn stats(&self) -> Result<EngineStats, EngineError> {
        self.request(Command::Stats)?
    }

    pub fn apply_report(&self) -> Result<Option<RouteApplyReport>, EngineError> {
        self.request(Command::ApplyReport)
    }

    pub fn runtime_snapshot(&self) -> Result<EngineRuntimeSnapshot, EngineError> {
        self.request(Command::RuntimeSnapshot)
    }

    pub fn relay_inventory_status(&self) -> Result<RelayInventoryStatusSnapshot, EngineError> {
        self.request(Command::RelayInventoryStatus)?
    }

    pub fn refresh_relay_inventory(&self) -> Result<RelayInventoryStatusSnapshot, EngineError> {
        self.request(Command::RefreshRelayInventory)?
    }

    #[cfg(all(not(target_os = "windows"), not(target_os = "linux")))]
    pub fn log_snapshot(&self) -> Result<Vec<String>, EngineError> {
        Ok(crate::log::snapshot())
    }

    #[cfg(all(not(target_os = "windows"), not(target_os = "linux")))]
    pub fn log_clear(&self) -> Result<(), EngineError> {
        crate::log::clear();
        Ok(())
    }
}
