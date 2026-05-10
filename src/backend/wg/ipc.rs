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
//! - v10: StartRequest 新增 wireguard_backend_preference
//! - v11: BackendErrorKind 新增 kernel/unsupported/ephemeral 结构化分类
//! - v12: 新增 LogSnapshot/LogClear 后端日志缓冲接口
//! - v13: Info 响应新增后端 capabilities，日志 IPC 使用有界脱敏快照
//! - v14: Info 响应新增 service_version 与 platform 元数据
//! - v15: 请求帧新增 transport-level request_id，便于跨日志关联
//! - v16: capabilities 声明后端日志快照最大字节数
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
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::core::route_plan::RouteApplyReport;

use super::engine::{
    EngineError, EngineRuntimeSnapshot, EngineStats, EngineStatus, RelayInventoryStatusSnapshot,
    StartRequest,
};

/// 当前 IPC 协议版本
///
/// 当 UI 和服务端的版本不匹配时，会返回 VersionMismatch 错误。
/// 升级时需要确保双方都支持相同的版本。
pub const IPC_PROTOCOL_VERSION: u32 = 16;

/// Maximum raw IPC frame size, including the trailing newline when present.
///
/// IPC uses one JSON value per line. Keeping a hard bound prevents a peer from
/// forcing the privileged service to buffer an unbounded line before JSON
/// parsing rejects it.
pub const MAX_IPC_FRAME_BYTES: usize = 1024 * 1024;

static NEXT_IPC_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

pub type IpcRequestId = u64;

#[derive(Debug, Clone)]
pub struct BackendRequest {
    pub request_id: IpcRequestId,
    pub command: BackendCommand,
}

pub fn next_ipc_request_id() -> IpcRequestId {
    NEXT_IPC_REQUEST_ID.fetch_add(1, Ordering::Relaxed)
}

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
    /// 查询后端进程当前日志缓冲
    LogSnapshot,
    /// 清空后端进程日志缓冲
    LogClear,
}

impl BackendCommand {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Ping => "ping",
            Self::Info => "info",
            Self::Start { .. } => "start",
            Self::Stop => "stop",
            Self::Status => "status",
            Self::Stats => "stats",
            Self::ApplyReport => "apply_report",
            Self::RuntimeSnapshot => "runtime_snapshot",
            Self::RelayInventoryStatus => "relay_inventory_status",
            Self::RefreshRelayInventory => "refresh_relay_inventory",
            Self::LogSnapshot => "log_snapshot",
            Self::LogClear => "log_clear",
        }
    }
}

/// 特权后端 -> UI 的响应枚举
///
/// 每个命令都有对应的响应类型。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BackendReply {
    /// 纯成功响应：用于 Ping / Start / Stop 等不需要额外数据的命令
    Ok,
    /// 元信息响应：返回协议版本与后端能力位
    Info {
        protocol_version: u32,
        #[serde(default = "backend_service_version")]
        service_version: String,
        #[serde(default = "backend_platform")]
        platform: String,
        #[serde(default = "backend_capabilities")]
        capabilities: BackendCapabilities,
    },
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
    /// 后端日志缓冲快照
    LogSnapshot { lines: Vec<String> },
    /// 执行失败响应：包含错误分类和可读消息
    Error {
        kind: BackendErrorKind,
        message: String,
    },
}

/// 后端能力声明。
///
/// Info 响应使用能力位表达功能边界，避免 UI 只能通过协议版本推断可用接口。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackendCapabilities {
    pub apply_report: bool,
    pub runtime_snapshot: bool,
    pub relay_inventory: bool,
    pub log_snapshot: bool,
    pub log_clear: bool,
    pub max_ipc_frame_bytes: usize,
    pub log_snapshot_max_lines: usize,
    #[serde(default = "default_log_snapshot_max_bytes")]
    pub log_snapshot_max_bytes: usize,
}

pub fn backend_capabilities() -> BackendCapabilities {
    BackendCapabilities {
        apply_report: true,
        runtime_snapshot: true,
        relay_inventory: true,
        log_snapshot: true,
        log_clear: true,
        max_ipc_frame_bytes: MAX_IPC_FRAME_BYTES,
        log_snapshot_max_lines: crate::log::MAX_LOG_SNAPSHOT_LINES,
        log_snapshot_max_bytes: crate::log::MAX_LOG_SNAPSHOT_BYTES,
    }
}

fn default_log_snapshot_max_bytes() -> usize {
    crate::log::MAX_LOG_SNAPSHOT_BYTES
}

pub fn backend_service_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

pub fn backend_platform() -> String {
    std::env::consts::OS.to_string()
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
    /// Linux kernel WireGuard 控制面错误
    KernelWireGuard,
    /// 请求的后端不可用或与功能冲突
    UnsupportedBackend,
    /// Quantum/DAITA ephemeral 协商或重配置失败
    Ephemeral,
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
        message: backend_error_message(&err),
    }
}

fn backend_error_message(err: &EngineError) -> String {
    match err {
        EngineError::KernelWireGuard(message)
        | EngineError::UnsupportedBackend(message)
        | EngineError::Ephemeral(message)
        | EngineError::Remote(message) => message.clone(),
        _ => err.to_string(),
    }
}

/// 将本地错误转换为可跨进程传递的错误分类
pub fn backend_error_kind(err: &EngineError) -> BackendErrorKind {
    match err {
        EngineError::ChannelClosed => BackendErrorKind::ChannelClosed,
        EngineError::AlreadyRunning => BackendErrorKind::AlreadyRunning,
        EngineError::NotRunning => BackendErrorKind::NotRunning,
        EngineError::AccessDenied => BackendErrorKind::AccessDenied,
        EngineError::KernelWireGuard(_) => BackendErrorKind::KernelWireGuard,
        EngineError::UnsupportedBackend(_) => BackendErrorKind::UnsupportedBackend,
        EngineError::Ephemeral(_) => BackendErrorKind::Ephemeral,
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
        BackendErrorKind::KernelWireGuard => EngineError::KernelWireGuard(message),
        BackendErrorKind::UnsupportedBackend => EngineError::UnsupportedBackend(message),
        BackendErrorKind::Ephemeral => EngineError::Ephemeral(message),
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

pub fn write_command_json_line(
    writer: &mut impl Write,
    request_id: IpcRequestId,
    command: &BackendCommand,
) -> io::Result<()> {
    let mut value = serde_json::to_value(command)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    match value {
        Value::Object(ref mut object) => {
            object.insert(
                "request_id".to_string(),
                Value::Number(serde_json::Number::from(request_id)),
            );
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "backend command did not serialize to a JSON object",
            ));
        }
    }
    write_json_line(writer, &value)
}

/// 读取单行 JSON 消息
///
/// 读取一行（以换行符分隔），然后解析为 JSON。
///
/// # 错误处理
/// - 返回 0 字节表示对端关闭连接
/// - JSON 解析失败返回 InvalidData 错误
pub fn read_json_line<T: for<'de> Deserialize<'de>>(reader: &mut impl BufRead) -> io::Result<T> {
    read_json_line_with_limit(reader, MAX_IPC_FRAME_BYTES)
}

pub fn read_backend_request(reader: &mut impl BufRead) -> io::Result<BackendRequest> {
    let mut line = Vec::new();
    read_bounded_line(reader, &mut line, MAX_IPC_FRAME_BYTES)?;
    let value: Value = serde_json::from_slice(&line)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    let request_id = value
        .as_object()
        .and_then(|object| object.get("request_id"))
        .and_then(Value::as_u64)
        .unwrap_or_else(next_ipc_request_id);
    let command = serde_json::from_value(value)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    Ok(BackendRequest {
        request_id,
        command,
    })
}

fn read_json_line_with_limit<T: for<'de> Deserialize<'de>>(
    reader: &mut impl BufRead,
    max_frame_bytes: usize,
) -> io::Result<T> {
    let mut line = Vec::new();
    read_bounded_line(reader, &mut line, max_frame_bytes)?;
    serde_json::from_slice(&line).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

fn read_bounded_line(
    reader: &mut impl BufRead,
    line: &mut Vec<u8>,
    max_frame_bytes: usize,
) -> io::Result<()> {
    loop {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            if line.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "backend closed the connection",
                ));
            }
            return Ok(());
        }

        let consumed = match buffer.iter().position(|byte| *byte == b'\n') {
            Some(position) => position + 1,
            None => buffer.len(),
        };
        if line.len().saturating_add(consumed) > max_frame_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("IPC frame exceeds {max_frame_bytes} bytes"),
            ));
        }

        line.extend_from_slice(&buffer[..consumed]);
        reader.consume(consumed);

        if line.last() == Some(&b'\n') {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn read_json_line_accepts_valid_line() {
        let mut reader = Cursor::new(
            br#"{"type":"ping"}
"#,
        );

        let command: BackendCommand = read_json_line(&mut reader).unwrap();

        assert!(matches!(command, BackendCommand::Ping));
    }

    #[test]
    fn read_json_line_accepts_eof_after_complete_json() {
        let mut reader = Cursor::new(br#"{"type":"ping"}"#);

        let command: BackendCommand = read_json_line(&mut reader).unwrap();

        assert!(matches!(command, BackendCommand::Ping));
    }

    #[test]
    fn read_json_line_rejects_empty_connection() {
        let mut reader = Cursor::new(Vec::<u8>::new());

        let err = read_json_line::<BackendCommand>(&mut reader).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn read_json_line_rejects_oversized_frame() {
        let mut reader = Cursor::new(
            br#"{"type":"ping"}
"#,
        );

        let err = read_json_line_with_limit::<BackendCommand>(&mut reader, 4).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("IPC frame exceeds 4 bytes"));
    }

    #[test]
    fn write_command_json_line_includes_request_id() {
        let mut out = Vec::new();

        write_command_json_line(&mut out, 42, &BackendCommand::Ping).unwrap();

        let value: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(value.get("type").and_then(Value::as_str), Some("ping"));
        assert_eq!(value.get("request_id").and_then(Value::as_u64), Some(42));
    }

    #[test]
    fn backend_command_ignores_request_id_for_legacy_server_compatibility() {
        let mut reader = Cursor::new(
            br#"{"type":"ping","request_id":42}
"#,
        );

        let command: BackendCommand = read_json_line(&mut reader).unwrap();

        assert!(matches!(command, BackendCommand::Ping));
    }

    #[test]
    fn read_backend_request_extracts_request_id() {
        let mut reader = Cursor::new(
            br#"{"type":"ping","request_id":42}
"#,
        );

        let request = read_backend_request(&mut reader).unwrap();

        assert_eq!(request.request_id, 42);
        assert!(matches!(request.command, BackendCommand::Ping));
    }

    #[test]
    fn read_backend_request_assigns_request_id_when_missing() {
        let mut reader = Cursor::new(
            br#"{"type":"ping"}
"#,
        );

        let request = read_backend_request(&mut reader).unwrap();

        assert!(request.request_id > 0);
        assert!(matches!(request.command, BackendCommand::Ping));
    }

    #[test]
    fn info_reply_deserializes_legacy_without_capabilities() {
        let reply: BackendReply =
            serde_json::from_str(r#"{"type":"info","protocol_version":12}"#).unwrap();

        match reply {
            BackendReply::Info {
                protocol_version,
                service_version,
                platform,
                capabilities,
            } => {
                assert_eq!(protocol_version, 12);
                assert_eq!(service_version, backend_service_version());
                assert_eq!(platform, backend_platform());
                assert_eq!(capabilities, backend_capabilities());
            }
            other => panic!("unexpected reply: {other:?}"),
        }
    }

    #[test]
    fn capabilities_deserialize_without_log_snapshot_max_bytes() {
        let capabilities: BackendCapabilities = serde_json::from_str(
            r#"{
                "apply_report": true,
                "runtime_snapshot": true,
                "relay_inventory": true,
                "log_snapshot": true,
                "log_clear": true,
                "max_ipc_frame_bytes": 1048576,
                "log_snapshot_max_lines": 500
            }"#,
        )
        .unwrap();

        assert_eq!(
            capabilities.log_snapshot_max_bytes,
            crate::log::MAX_LOG_SNAPSHOT_BYTES
        );
    }
}
