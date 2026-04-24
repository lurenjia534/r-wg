//! 跨进程控制协议 (IPC)
//!
//! 本模块定义了 UI 进程与特权后端服务之间的通信协议。
//!
//! # 设计原则
//!
//! 1. **短连接模型**: 每个请求-响应对使用独立的连接，避免维护长连接状态机
//! 2. **单请求单响应**: 每个命令都有明确的响应，便于超时处理和错误分类
//! 3. **版本隔离**: 协议版本独立管理，UI 和服务可以独立升级
//!
//! # 协议版本历史
//!
//! - v1: 初始版本
//! - v2: 初始 Windows/Linux helper 兼容版本
//! - v3: 添加 ApplyReport 支持
//! - v4: StartRequest 新增 quantum_mode
//! - v5: RuntimeSnapshot 新增量子状态与失败分类
//! - v6: StartRequest/RuntimeSnapshot 新增 DAITA 字段
//! - v7: EngineStats::PeerStats 新增 DAITA 统计字段
//! - v8: 新增 DAITA relay inventory 状态/刷新接口
//! - v9: StartRequest 新增 kill_switch_enabled
//!
//! # 消息格式
//!
//! 所有消息都是单行 JSON，便于调试和日志记录。
//! 格式: `{"type": "command_name", ...fields}`
//!
//! # 平台差异
//!
//! - Linux: 使用 Unix Domain Socket (UDS)
//! - Windows: 使用命名管道 (Named Pipe)
//!
//! 两者共用相同的消息格式，只是传输层不同。

use std::io::{self, BufRead, Write};

use serde::{Deserialize, Serialize};

use crate::core::route_plan::RouteApplyReport;

use super::engine::{
    EngineError, EngineRuntimeSnapshot, EngineStats, EngineStatus, RelayInventoryStatusSnapshot,
    StartRequest,
};

/// 当前 IPC 协议版本
///
/// 当 UI 和服务端的版本不匹配时，会返回 VersionMismatch 错误。
/// 升级时需要确保双方都支持相同的版本。
pub const IPC_PROTOCOL_VERSION: u32 = 9;

/// UI -> 特权后端的命令枚举
///
/// 这些命令由 UI 进程发送到特权后端服务执行。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BackendCommand {
    /// 心跳检测：只检查后端是否在线，不改状态
    Ping,
    /// 获取后端元信息：返回协议版本等
    Info,
    /// 启动隧道：附带完整的启动请求
    Start { request: StartRequest },
    /// 停止隧道：无参数
    Stop,
    /// 查询当前运行状态
    Status,
    /// 查询当前 Peer 统计信息
    Stats,
    /// 查询最近一次网络应用报告
    ApplyReport,
    /// 查询完整运行时快照
    RuntimeSnapshot,
    /// 查询缓存的 Mullvad relay inventory 状态
    RelayInventoryStatus,
    /// 下载并刷新缓存的 Mullvad relay inventory
    RefreshRelayInventory,
}

/// 特权后端 -> UI 的响应枚举
///
/// 每个命令都有对应的响应类型。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BackendReply {
    /// 纯成功响应：用于 Ping / Start / Stop 等不需要额外数据的命令
    Ok,
    /// 元信息响应：返回协议版本
    Info { protocol_version: u32 },
    /// 状态查询响应：返回 Running/Stopped
    Status { status: EngineStatus },
    /// 统计查询响应：返回所有 Peer 的流量统计
    Stats { stats: EngineStats },
    /// 路由报告响应：返回最近一次路由应用的结果
    ApplyReport { report: Option<RouteApplyReport> },
    /// 运行时快照响应：返回包含量子状态的完整运行态
    RuntimeSnapshot { snapshot: EngineRuntimeSnapshot },
    /// DAITA 资源缓存状态
    RelayInventoryStatus {
        snapshot: RelayInventoryStatusSnapshot,
    },
    /// 执行失败响应：包含错误分类和可读消息
    Error {
        kind: BackendErrorKind,
        message: String,
    },
}

/// 跨进程可恢复错误分类
///
/// 这些错误类型可以在进程间安全传递，用于 UI 判断错误性质并做出相应处理。
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendErrorKind {
    /// 通道已关闭：后端可能已崩溃或退出
    ChannelClosed,
    /// 重复启动：引擎已在运行
    AlreadyRunning,
    /// 未启动：请求停止但引擎未运行
    NotRunning,
    /// 权限不足：无法访问特权后端
    AccessDenied,
    /// 其他错误：需要查看 message 字段
    Other,
}

/// 将 `Result<(), EngineError>` 转换为通用 IPC 响应
///
/// 用于不需要返回数据的命令（Ping、Stop 等）。
pub fn unit_reply(result: Result<(), EngineError>) -> BackendReply {
    match result {
        Ok(()) => BackendReply::Ok,
        Err(err) => error_reply(err),
    }
}

/// 将 `Result<Option<RouteApplyReport>, EngineError>` 转换为 IPC 响应
///
/// 用于 ApplyReport 查询命令。
pub fn option_reply(result: Result<Option<RouteApplyReport>, EngineError>) -> BackendReply {
    match result {
        Ok(report) => BackendReply::ApplyReport { report },
        Err(err) => error_reply(err),
    }
}

pub fn runtime_snapshot_reply(result: Result<EngineRuntimeSnapshot, EngineError>) -> BackendReply {
    match result {
        Ok(snapshot) => BackendReply::RuntimeSnapshot { snapshot },
        Err(err) => error_reply(err),
    }
}

pub fn relay_inventory_status_reply(
    result: Result<RelayInventoryStatusSnapshot, EngineError>,
) -> BackendReply {
    match result {
        Ok(snapshot) => BackendReply::RelayInventoryStatus { snapshot },
        Err(err) => error_reply(err),
    }
}

/// 将本地引擎错误转换为 IPC 错误响应
///
/// 错误被分为两部分：
/// - `kind`: 可跨进程传递的错误分类
/// - `message`: 人类可读的错误详情
pub fn error_reply(err: EngineError) -> BackendReply {
    BackendReply::Error {
        kind: backend_error_kind(&err),
        message: err.to_string(),
    }
}

/// 将本地错误转换为可跨进程传递的错误分类
pub fn backend_error_kind(err: &EngineError) -> BackendErrorKind {
    match err {
        EngineError::ChannelClosed => BackendErrorKind::ChannelClosed,
        EngineError::AlreadyRunning => BackendErrorKind::AlreadyRunning,
        EngineError::NotRunning => BackendErrorKind::NotRunning,
        EngineError::AccessDenied => BackendErrorKind::AccessDenied,
        _ => BackendErrorKind::Other,
    }
}

/// 将远端错误分类和消息转换回本地 EngineError
///
/// 这是 map_backend_error 的反向操作，用于 UI 端处理来自服务的错误。
pub fn map_backend_error(kind: BackendErrorKind, message: String) -> EngineError {
    match kind {
        BackendErrorKind::ChannelClosed => EngineError::ChannelClosed,
        BackendErrorKind::AlreadyRunning => EngineError::AlreadyRunning,
        BackendErrorKind::NotRunning => EngineError::NotRunning,
        BackendErrorKind::AccessDenied => EngineError::AccessDenied,
        BackendErrorKind::Other => EngineError::Remote(message),
    }
}

/// 协议错误：当收到意外的响应类型时调用
pub fn unexpected_reply(reply: BackendReply) -> EngineError {
    EngineError::Remote(format!("unexpected backend reply: {reply:?}"))
}

/// 版本不匹配错误
///
/// 当 UI 和服务的协议版本不一致时返回此错误。
pub fn protocol_mismatch(expected: u32, actual: u32) -> EngineError {
    EngineError::VersionMismatch { expected, actual }
}

/// 写入单行 JSON 消息
///
/// # 格式
/// ```text
/// {"type":"command_name","field1":"value1",...}\n
/// ```
///
/// 每条消息以换行符结尾，便于：
/// - 调试时直接查看原始消息
/// - 日志记录
/// - 简化解析逻辑（无需复杂帧协议）
pub fn write_json_line<T: Serialize>(writer: &mut impl Write, value: &T) -> io::Result<()> {
    let payload = serde_json::to_string(value)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    writer.write_all(payload.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()
}

/// 读取单行 JSON 消息
///
/// 读取一行（以换行符分隔），然后解析为 JSON。
///
/// # 错误处理
/// - 返回 0 字节表示对端关闭连接
/// - JSON 解析失败返回 InvalidData 错误
pub fn read_json_line<T: for<'de> Deserialize<'de>>(reader: &mut impl BufRead) -> io::Result<T> {
    let mut line = String::new();
    let read = reader.read_line(&mut line)?;
    if read == 0 {
        // 对端提前断开连接
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "backend closed the connection",
        ));
    }
    serde_json::from_str(line.trim_end())
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}
