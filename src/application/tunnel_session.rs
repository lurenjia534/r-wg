//! 隧道会话服务模块
//!
//! 本模块提供隧道会话的高层抽象，封装了与后端引擎的交互逻辑。
//!
//! # 服务职责
//!
//! - 启动/停止 WireGuard 隧道
//! - 查询隧道运行状态
//! - 获取流量统计
//! - 获取路由应用报告
//!
//! # 线程安全
//!
//! TunnelSessionService 可以安全地在多个线程间共享和克隆。
//! 所有操作最终都通过 Engine 的线程安全通道执行。

use std::time::Duration;

use crate::backend::wg::{
    DaitaMode, Engine, EngineError, EngineRuntimeSnapshot, EngineStats, EngineStatus,
    EphemeralFailureKind, QuantumMode, RelayInventoryStatusSnapshot, StartRequest,
};
use crate::core::dns::DnsSelection;
use crate::core::route_plan::RouteApplyReport;

/// 隧道会话服务
///
/// 封装了与 WireGuard 后端引擎的交互，提供高层次的隧道管理接口。
#[derive(Clone)]
pub struct TunnelSessionService {
    engine: Engine,
}

impl TunnelSessionService {
    /// 创建新的隧道会话服务
    pub fn new(engine: Engine) -> Self {
        Self { engine }
    }

    /// 启动隧道
    ///
    /// # 参数
    /// * `request` - 启动请求，包含 TUN 名称、配置文本和 DNS 选择
    ///
    /// # 返回
    /// * `StartTunnelOutcome` - 包含启动结果和路由应用报告
    pub fn start(&self, request: StartTunnelRequest) -> StartTunnelOutcome {
        let result = self.engine.start(StartRequest::new(
            request.tun_name,
            request.config_text,
            request.dns_selection,
            request.quantum_mode,
            request.daita_mode,
        ));
        let runtime_snapshot = self
            .engine
            .runtime_snapshot()
            .ok()
            .map(map_runtime_snapshot);
        let apply_report = runtime_snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.apply_report.clone())
            .or_else(|| self.engine.apply_report().ok().flatten());
        StartTunnelOutcome {
            result,
            apply_report,
            runtime_snapshot,
        }
    }

    /// 停止隧道
    pub fn stop(&self) -> Result<(), EngineError> {
        self.engine.stop()
    }

    /// 查询隧道状态
    pub fn status(&self) -> Result<EngineStatus, EngineError> {
        self.engine.status()
    }

    /// 获取流量统计
    pub fn stats(&self) -> Result<EngineStats, EngineError> {
        self.engine.stats()
    }

    /// 获取最近一次路由应用报告
    pub fn apply_report(&self) -> Result<Option<RouteApplyReport>, EngineError> {
        self.engine.apply_report()
    }

    /// 获取运行时快照
    ///
    /// 包含当前状态和路由应用报告，用于 UI 恢复显示。
    pub fn runtime_snapshot(&self) -> Result<TunnelRuntimeSnapshot, EngineError> {
        self.engine.runtime_snapshot().map(map_runtime_snapshot)
    }

    pub fn relay_inventory_status(&self) -> Result<RelayInventoryStatusSnapshot, EngineError> {
        self.engine.relay_inventory_status()
    }

    pub fn refresh_relay_inventory(&self) -> Result<RelayInventoryStatusSnapshot, EngineError> {
        self.engine.refresh_relay_inventory()
    }
}

/// 启动隧道的请求参数
#[derive(Debug, Clone)]
pub struct StartTunnelRequest {
    /// TUN 设备名称
    pub tun_name: String,
    /// WireGuard 配置文件内容
    pub config_text: String,
    /// DNS 选择配置
    pub dns_selection: DnsSelection,
    /// 量子抗性隧道升级模式
    pub quantum_mode: QuantumMode,
    /// DAITA 模式
    pub daita_mode: DaitaMode,
}

impl StartTunnelRequest {
    /// 创建新的启动请求
    pub fn new(
        tun_name: impl Into<String>,
        config_text: impl Into<String>,
        dns_selection: DnsSelection,
        quantum_mode: QuantumMode,
        daita_mode: DaitaMode,
    ) -> Self {
        Self {
            tun_name: tun_name.into(),
            config_text: config_text.into(),
            dns_selection,
            quantum_mode,
            daita_mode,
        }
    }
}

/// 启动结果
pub struct StartTunnelOutcome {
    /// 启动操作的结果
    pub result: Result<(), EngineError>,
    /// 路由应用报告
    pub apply_report: Option<RouteApplyReport>,
    /// 运行态快照
    pub runtime_snapshot: Option<TunnelRuntimeSnapshot>,
}

/// 运行时快照
///
/// 包含恢复 UI 显示所需的最小状态信息。
#[derive(Debug, Clone)]
pub struct TunnelRuntimeSnapshot {
    /// 当前运行状态
    pub status: EngineStatus,
    /// 最近一次路由应用报告
    pub apply_report: Option<RouteApplyReport>,
    /// 当前会话是否已经切换到量子保护态
    pub quantum_protected: bool,
    /// 最近一次量子升级失败分类
    pub last_quantum_failure: Option<EphemeralFailureKind>,
    /// 当前会话是否启用了 DAITA
    pub daita_active: bool,
    /// 最近一次 DAITA 协商失败分类
    pub last_daita_failure: Option<EphemeralFailureKind>,
}

fn map_runtime_snapshot(snapshot: EngineRuntimeSnapshot) -> TunnelRuntimeSnapshot {
    TunnelRuntimeSnapshot {
        status: snapshot.status,
        apply_report: snapshot.apply_report,
        quantum_protected: snapshot.quantum_protected,
        last_quantum_failure: snapshot.last_quantum_failure,
        daita_active: snapshot.daita_active,
        last_daita_failure: snapshot.last_daita_failure,
    }
}

/// 切换隧道的输入参数
///
/// 用于决定下一步应该执行什么操作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToggleTunnelInput {
    /// 是否正在执行操作（启动/停止中）
    pub busy: bool,
    /// 隧道是否正在运行
    pub running: bool,
    /// 当前选中的配置 ID
    pub selected_config_id: Option<u64>,
    /// 当前运行的配置 ID
    pub running_config_id: Option<u64>,
    /// 草稿是否有已保存的来源
    pub draft_has_saved_source: bool,
    /// 草稿是否有未保存的更改
    pub draft_is_dirty: bool,
    /// 重启延迟（用于快速切换场景）
    pub restart_delay: Option<Duration>,
}

/// 启动被阻止的原因
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartBlockedReason {
    /// 草稿尚未保存
    SaveDraftFirst,
    /// 草稿有未保存的更改
    SaveChangesFirst,
    /// 未选择任何配置
    SelectTunnelFirst,
}

impl StartBlockedReason {
    /// 获取错误消息
    pub fn message(self) -> &'static str {
        match self {
            Self::SaveDraftFirst => "Save this draft before starting",
            Self::SaveChangesFirst => "Save changes before starting",
            Self::SelectTunnelFirst => "Select a tunnel first",
        }
    }
}

/// 切换隧道的决策结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToggleTunnelDecision {
    /// 无操作（当前状态不支持任何操作）
    Noop,
    /// 排队等待启动（忙碌中但有新启动请求）
    QueuePendingStart { config_id: u64 },
    /// 停止当前运行的隧道
    StopRunning,
    /// 启动选中的配置
    StartSelected {
        config_id: u64,
        restart_delay: Option<Duration>,
    },
    /// 启动被阻止（显示相应错误）
    Blocked(StartBlockedReason),
}

/// 停止成功后的决策
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopSuccessDecision {
    /// 空闲状态（无待处理操作）
    Idle,
    /// 重启待处理的配置
    RestartPending { config_id: u64 },
}

/// 根据输入参数决定切换操作
///
/// 这是隧道切换逻辑的核心决策函数。
pub fn decide_toggle(input: ToggleTunnelInput) -> ToggleTunnelDecision {
    if input.busy {
        // 忙碌状态：只能排队新的启动请求
        return match pending_start_target(input.selected_config_id, input.running_config_id) {
            Some(config_id) if input.running => {
                ToggleTunnelDecision::QueuePendingStart { config_id }
            }
            _ => ToggleTunnelDecision::Noop,
        };
    }

    if input.running {
        // 空闲且运行中：请求停止
        return ToggleTunnelDecision::StopRunning;
    }

    // 空闲且未运行：尝试启动
    if !input.draft_has_saved_source {
        return ToggleTunnelDecision::Blocked(StartBlockedReason::SaveDraftFirst);
    }
    if input.draft_is_dirty {
        return ToggleTunnelDecision::Blocked(StartBlockedReason::SaveChangesFirst);
    }

    match input.selected_config_id {
        Some(config_id) => ToggleTunnelDecision::StartSelected {
            config_id,
            restart_delay: input.restart_delay,
        },
        None => ToggleTunnelDecision::Blocked(StartBlockedReason::SelectTunnelFirst),
    }
}

/// 停止成功后决定下一步操作
pub fn decide_after_stop_success(pending_config_id: Option<u64>) -> StopSuccessDecision {
    match pending_config_id {
        Some(config_id) => StopSuccessDecision::RestartPending { config_id },
        None => StopSuccessDecision::Idle,
    }
}

/// 获取待启动的目标配置 ID
///
/// 优先使用选中的配置，否则使用正在运行的配置。
pub fn pending_start_target(
    selected_config_id: Option<u64>,
    running_config_id: Option<u64>,
) -> Option<u64> {
    selected_config_id.or(running_config_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::wg::{EngineRuntimeSnapshot, EngineStatus, EphemeralFailureKind};

    #[test]
    fn runtime_snapshot_mapping_preserves_quantum_state() {
        let snapshot = map_runtime_snapshot(EngineRuntimeSnapshot {
            status: EngineStatus::Running,
            apply_report: None,
            quantum_protected: true,
            last_quantum_failure: Some(EphemeralFailureKind::Timeout),
            daita_active: true,
            last_daita_failure: Some(EphemeralFailureKind::Rpc),
        });

        assert!(matches!(snapshot.status, EngineStatus::Running));
        assert!(snapshot.quantum_protected);
        assert_eq!(
            snapshot.last_quantum_failure,
            Some(EphemeralFailureKind::Timeout)
        );
        assert!(snapshot.daita_active);
        assert_eq!(snapshot.last_daita_failure, Some(EphemeralFailureKind::Rpc));
    }

    #[test]
    fn busy_running_queues_selected_config() {
        let decision = decide_toggle(ToggleTunnelInput {
            busy: true,
            running: true,
            selected_config_id: Some(22),
            running_config_id: Some(11),
            draft_has_saved_source: true,
            draft_is_dirty: false,
            restart_delay: None,
        });

        assert_eq!(
            decision,
            ToggleTunnelDecision::QueuePendingStart { config_id: 22 }
        );
    }

    #[test]
    fn idle_running_requests_stop() {
        let decision = decide_toggle(ToggleTunnelInput {
            busy: false,
            running: true,
            selected_config_id: Some(22),
            running_config_id: Some(11),
            draft_has_saved_source: true,
            draft_is_dirty: false,
            restart_delay: None,
        });

        assert_eq!(decision, ToggleTunnelDecision::StopRunning);
    }

    #[test]
    fn idle_start_requires_saved_draft() {
        let decision = decide_toggle(ToggleTunnelInput {
            busy: false,
            running: false,
            selected_config_id: Some(22),
            running_config_id: None,
            draft_has_saved_source: false,
            draft_is_dirty: false,
            restart_delay: None,
        });

        assert_eq!(
            decision,
            ToggleTunnelDecision::Blocked(StartBlockedReason::SaveDraftFirst)
        );
    }

    #[test]
    fn idle_start_requires_clean_draft() {
        let decision = decide_toggle(ToggleTunnelInput {
            busy: false,
            running: false,
            selected_config_id: Some(22),
            running_config_id: None,
            draft_has_saved_source: true,
            draft_is_dirty: true,
            restart_delay: None,
        });

        assert_eq!(
            decision,
            ToggleTunnelDecision::Blocked(StartBlockedReason::SaveChangesFirst)
        );
    }

    #[test]
    fn idle_start_uses_selected_config_and_delay() {
        let delay = Some(Duration::from_millis(250));
        let decision = decide_toggle(ToggleTunnelInput {
            busy: false,
            running: false,
            selected_config_id: Some(22),
            running_config_id: None,
            draft_has_saved_source: true,
            draft_is_dirty: false,
            restart_delay: delay,
        });

        assert_eq!(
            decision,
            ToggleTunnelDecision::StartSelected {
                config_id: 22,
                restart_delay: delay,
            }
        );
    }

    #[test]
    fn stop_success_restarts_pending_config() {
        assert_eq!(
            decide_after_stop_success(Some(7)),
            StopSuccessDecision::RestartPending { config_id: 7 }
        );
        assert_eq!(decide_after_stop_success(None), StopSuccessDecision::Idle);
    }
}
