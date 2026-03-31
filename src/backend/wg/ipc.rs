//! 跨进程控制协议：
//! - Windows 提权 helper 与 Linux system service 共用同一套消息形状；
//! - 保持“短连接 + 单请求单响应”，避免长连接状态机；
//! - 协议版本单独暴露，便于 UI/service 做兼容性检查。
use std::io::{self, BufRead, Write};

use serde::{Deserialize, Serialize};

use crate::core::route_plan::RouteApplyReport;

use super::engine::{EngineError, EngineStats, EngineStatus, StartRequest};

/// 当前 IPC 协议版本。
pub const IPC_PROTOCOL_VERSION: u32 = 3;

/// UI -> 特权后端的命令集合。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BackendCommand {
    /// 只检查后端是否在线，不改状态。
    Ping,
    /// 返回协议版本等元信息。
    Info,
    /// 启动隧道。
    Start { request: StartRequest },
    /// 停止隧道。
    Stop,
    /// 查询当前运行状态。
    Status,
    /// 查询当前 peer 统计。
    Stats,
    /// 查询最近一次网络应用报告。
    ApplyReport,
}

/// 特权后端 -> UI 的响应集合。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BackendReply {
    /// 纯成功响应：用于 Ping / Start / Stop 这类不需要额外载荷的命令。
    Ok,
    /// 协议/元信息握手响应：当前只暴露协议版本，后续也可继续扩展。
    Info { protocol_version: u32 },
    /// 运行状态查询结果。
    Status { status: EngineStatus },
    /// 统计信息查询结果。
    Stats { stats: EngineStats },
    /// 最近一次网络应用报告。
    ApplyReport { report: Option<RouteApplyReport> },
    /// 远端执行失败；`kind` 负责分类，`message` 保留可读细节。
    Error {
        kind: BackendErrorKind,
        message: String,
    },
}

/// 跨进程可恢复错误分类。
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendErrorKind {
    ChannelClosed,
    AlreadyRunning,
    NotRunning,
    AccessDenied,
    Other,
}

/// `Result<()>` -> 通用 IPC 响应。
pub fn unit_reply(result: Result<(), EngineError>) -> BackendReply {
    match result {
        Ok(()) => BackendReply::Ok,
        Err(err) => error_reply(err),
    }
}

/// `Result<Option<T>>` -> 通用 IPC 响应。
pub fn option_reply(
    result: Result<Option<RouteApplyReport>, EngineError>,
) -> BackendReply {
    match result {
        Ok(report) => BackendReply::ApplyReport { report },
        Err(err) => error_reply(err),
    }
}

/// 本地错误 -> IPC 错误响应。
pub fn error_reply(err: EngineError) -> BackendReply {
    BackendReply::Error {
        kind: backend_error_kind(&err),
        message: err.to_string(),
    }
}

/// 本地错误 -> 远端可识别错误种类。
pub fn backend_error_kind(err: &EngineError) -> BackendErrorKind {
    match err {
        EngineError::ChannelClosed => BackendErrorKind::ChannelClosed,
        EngineError::AlreadyRunning => BackendErrorKind::AlreadyRunning,
        EngineError::NotRunning => BackendErrorKind::NotRunning,
        EngineError::AccessDenied => BackendErrorKind::AccessDenied,
        _ => BackendErrorKind::Other,
    }
}

/// 把远端错误映射回 UI 可处理的 `EngineError`。
pub fn map_backend_error(kind: BackendErrorKind, message: String) -> EngineError {
    match kind {
        BackendErrorKind::ChannelClosed => EngineError::ChannelClosed,
        BackendErrorKind::AlreadyRunning => EngineError::AlreadyRunning,
        BackendErrorKind::NotRunning => EngineError::NotRunning,
        BackendErrorKind::AccessDenied => EngineError::AccessDenied,
        BackendErrorKind::Other => EngineError::Remote(message),
    }
}

/// 协议结构与预期不一致时统一报错。
pub fn unexpected_reply(reply: BackendReply) -> EngineError {
    EngineError::Remote(format!("unexpected backend reply: {reply:?}"))
}

/// 把版本不兼容转换成统一错误。
pub fn protocol_mismatch(expected: u32, actual: u32) -> EngineError {
    EngineError::VersionMismatch { expected, actual }
}

/// 以“单行 JSON”的形式写出一条消息。
pub fn write_json_line<T: Serialize>(writer: &mut impl Write, value: &T) -> io::Result<()> {
    // IPC 刻意保持“一请求一行 JSON”，这样 Windows TCP helper 与 Linux UDS service
    // 可以共用同一套编解码逻辑，而不必再引入 framing 协议或流式解析状态机。
    let payload = serde_json::to_string(value)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    writer.write_all(payload.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()
}

/// 读取一条“单行 JSON”消息。
pub fn read_json_line<T: for<'de> Deserialize<'de>>(reader: &mut impl BufRead) -> io::Result<T> {
    let mut line = String::new();
    let read = reader.read_line(&mut line)?;
    if read == 0 {
        // 这里统一视为“对端提前断开”，让上层把它映射成 ChannelClosed /
        // backend unavailable，而不是继续拿空字符串做 JSON 解析。
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "backend closed the connection",
        ));
    }
    serde_json::from_str(line.trim_end())
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}
