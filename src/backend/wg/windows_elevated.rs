//! Windows 按需提权引擎门面。
//!
//! 设计目标：
//! - 普通权限启动主 UI；
//! - 用户点击启动隧道时，再通过 UAC 拉起管理员 helper；
//! - helper 进程持有 gotatun 设备与 Windows 网络配置生命周期；
//! - 已经管理员启动时，直接复用现有本地引擎，不额外走 IPC。
//!
//! Linux 现已切到 systemd privileged service + UDS；这里仍然只负责 Windows 路径。

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, BufReader};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use windows::core::PCWSTR;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Security::Cryptography::{BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

use super::engine::Engine as LocalEngine;
use super::ipc::{
    error_reply, map_backend_error, read_json_line, unexpected_reply, unit_reply, write_json_line,
    BackendCommand, BackendReply,
};
use super::{EngineError, EngineStats, EngineStatus, StartRequest};

/// helper 首次拉起后的最长等待时间。
const HELPER_START_TIMEOUT: Duration = Duration::from_secs(10);
/// 单次 IPC 连接/读写允许的最长时间。
const HELPER_IO_TIMEOUT: Duration = Duration::from_secs(30);
/// helper 在“无隧道运行 + 无新请求”时的空闲退出时间。
const HELPER_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
/// 轮询 session 文件 / helper 存活状态的间隔。
const HELPER_POLL_INTERVAL: Duration = Duration::from_millis(200);
/// 同一个 exe 进入管理员 helper 模式时使用的参数。
const HELPER_ARG: &str = "--elevated-helper";

/// 对 UI 暴露的统一引擎门面。
///
/// - 已提权：直接使用原本的本地引擎；
/// - 未提权：使用远端 helper 代理。
#[derive(Clone)]
pub struct Engine {
    inner: std::sync::Arc<EngineMode>,
}

/// Windows 下两种运行模式：本地直连或远端 helper。
enum EngineMode {
    Local(LocalEngine),
    Remote(RemoteEngine),
}

/// 非提权 UI 侧持有的 helper 客户端状态。
///
/// 这里只保存 session 文件路径；真正的端口与 secret 在文件里动态读取，
/// 这样 UI 重启后也能重新发现仍在运行的 helper。
#[derive(Clone)]
struct RemoteEngine {
    session_file: std::sync::Arc<PathBuf>,
}

/// helper 运行信息。
///
/// - `port`：监听在 127.0.0.1 上的临时端口；
/// - `secret`：防止同机其它意外连接误发控制命令。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct HelperSession {
    port: u16,
    secret: String,
}

/// 每次 IPC 请求的外层封包。
///
/// 所有请求都要携带 secret，helper 先鉴权再执行命令。
#[derive(Debug, Serialize, Deserialize)]
struct HelperEnvelope {
    secret: String,
    command: BackendCommand,
}

/// 进程入口早期分流：当前 exe 是否应进入管理员 helper 模式。
///
/// 返回 `true` 代表已经处理完 helper 生命周期，主进程不应继续创建 UI。
pub fn maybe_run_elevated_helper() -> bool {
    let mut args = env::args_os();
    let _ = args.next();
    let Some(first) = args.next() else {
        return false;
    };
    if first != HELPER_ARG {
        return false;
    }

    // Windows 下真正持有隧道设备的是 helper，而不是普通权限 UI，
    // 因此 MtuWatcher 也必须在 helper 进程里保活。
    let _mtu = gotatun::tun::MtuWatcher::new(1500);
    crate::log::init();
    let session_file = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(helper_session_path);
    let _ = run_elevated_helper(session_file);
    true
}

impl Engine {
    /// 根据当前进程是否已提权，选择本地模式或 helper 代理模式。
    pub fn new() -> Self {
        let inner = if is_process_elevated() {
            EngineMode::Local(LocalEngine::new())
        } else {
            EngineMode::Remote(RemoteEngine::new())
        };
        Self {
            inner: std::sync::Arc::new(inner),
        }
    }

    /// 启动隧道。
    pub fn start(&self, request: StartRequest) -> Result<(), EngineError> {
        match &*self.inner {
            EngineMode::Local(engine) => engine.start(request),
            EngineMode::Remote(engine) => engine.start(request),
        }
    }

    /// 停止隧道。
    pub fn stop(&self) -> Result<(), EngineError> {
        match &*self.inner {
            EngineMode::Local(engine) => engine.stop(),
            EngineMode::Remote(engine) => engine.stop(),
        }
    }

    /// 查询运行状态。
    pub fn status(&self) -> Result<EngineStatus, EngineError> {
        match &*self.inner {
            EngineMode::Local(engine) => engine.status(),
            EngineMode::Remote(engine) => engine.status(),
        }
    }

    /// 查询运行时统计。
    pub fn stats(&self) -> Result<EngineStats, EngineError> {
        match &*self.inner {
            EngineMode::Local(engine) => engine.stats(),
            EngineMode::Remote(engine) => engine.stats(),
        }
    }
}

impl RemoteEngine {
    /// UI 侧只持有 session 文件位置；helper 具体端口由运行时决定。
    fn new() -> Self {
        Self {
            session_file: std::sync::Arc::new(helper_session_path()),
        }
    }

    /// 启动隧道。
    ///
    /// 逻辑分两步：
    /// 1. 确保 helper 已存在且可连通；
    /// 2. 发送 Start 命令。
    ///
    /// 如果第一次发送失败，通常意味着 session 文件陈旧或 helper 刚好重启，
    /// 这里会清理陈旧 session 并重试一次。
    fn start(&self, request: StartRequest) -> Result<(), EngineError> {
        let session = self.ensure_helper()?;
        match self.send_command(
            &session,
            BackendCommand::Start {
                request: request.clone(),
            },
        ) {
            Ok(reply) => self.expect_unit(reply),
            Err(_) => {
                self.clear_session_file();
                let session = self.ensure_helper()?;
                let reply = self.send_command(&session, BackendCommand::Start { request })?;
                self.expect_unit(reply)
            }
        }
    }

    /// 停止隧道。
    ///
    /// 没有 session 文件时直接视为未运行；存在但连接失败则视为 helper 已失联。
    fn stop(&self) -> Result<(), EngineError> {
        let Some(session) = self.load_session() else {
            return Err(EngineError::NotRunning);
        };
        match self.send_command(&session, BackendCommand::Stop) {
            Ok(reply) => self.expect_unit(reply),
            Err(err) => {
                self.clear_session_file();
                Err(err)
            }
        }
    }

    /// 查询状态。
    ///
    /// 这里把“没有 session / helper 已失联”都降级为 `Stopped`，
    /// 目的是让 UI 在启动同步阶段更稳妥，不把陈旧状态误判为异常弹窗。
    fn status(&self) -> Result<EngineStatus, EngineError> {
        let Some(session) = self.load_session() else {
            return Ok(EngineStatus::Stopped);
        };
        match self.send_command(&session, BackendCommand::Status) {
            Ok(BackendReply::Status { status }) => Ok(status),
            Ok(BackendReply::Error { kind, message }) => Err(map_backend_error(kind, message)),
            Ok(other) => Err(unexpected_reply(other)),
            Err(_) => {
                self.clear_session_file();
                Ok(EngineStatus::Stopped)
            }
        }
    }

    /// 查询统计。
    ///
    /// 统计接口保留错误语义，不把 helper 失联静默吞掉，便于 UI 明确提示。
    fn stats(&self) -> Result<EngineStats, EngineError> {
        let Some(session) = self.load_session() else {
            return Err(EngineError::NotRunning);
        };
        match self.send_command(&session, BackendCommand::Stats) {
            Ok(BackendReply::Stats { stats }) => Ok(stats),
            Ok(BackendReply::Error { kind, message }) => Err(map_backend_error(kind, message)),
            Ok(other) => Err(unexpected_reply(other)),
            Err(err) => {
                self.clear_session_file();
                Err(err)
            }
        }
    }

    /// 确保 helper 已经存在并可连通。
    ///
    /// 优先复用已有 helper；只有在没有可用 helper 时才触发新的 UAC 提权。
    fn ensure_helper(&self) -> Result<HelperSession, EngineError> {
        if let Some(session) = self.load_session() {
            if self.send_command(&session, BackendCommand::Ping).is_ok() {
                return Ok(session);
            }
            self.clear_session_file();
        }

        self.launch_helper()?;
        let start = Instant::now();
        while start.elapsed() < HELPER_START_TIMEOUT {
            if let Some(session) = self.load_session() {
                if self.send_command(&session, BackendCommand::Ping).is_ok() {
                    return Ok(session);
                }
            }
            thread::sleep(HELPER_POLL_INTERVAL);
        }

        Err(EngineError::Remote(
            "timed out waiting for elevated helper".to_string(),
        ))
    }

    /// 读取 helper session 文件。
    ///
    /// 解析失败直接当作“当前没有可用 helper”，上层会按需重新拉起。
    fn load_session(&self) -> Option<HelperSession> {
        let text = fs::read_to_string(self.session_file.as_path()).ok()?;
        let session: HelperSession = serde_json::from_str(&text).ok()?;
        if session.port == 0 || session.secret.trim().is_empty() {
            return None;
        }
        Some(session)
    }

    /// 清理陈旧 session 文件。
    fn clear_session_file(&self) {
        let _ = fs::remove_file(self.session_file.as_path());
    }

    /// 通过 `ShellExecuteW(..., "runas", ...)` 触发 UAC，拉起管理员 helper。
    fn launch_helper(&self) -> Result<(), EngineError> {
        if let Some(parent) = self.session_file.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                EngineError::Remote(format!("failed to create helper state dir: {err}"))
            })?;
        }
        self.clear_session_file();

        let exe = env::current_exe()
            .map_err(|err| EngineError::Remote(format!("failed to locate current exe: {err}")))?;
        let params = format!(
            "{HELPER_ARG} {}",
            quote_windows_arg(&self.session_file.display().to_string())
        );

        let verb_w = encode_wide(OsStr::new("runas"));
        let exe_w = encode_wide(exe.as_os_str());
        let params_w = encode_wide(OsStr::new(&params));
        let empty_w = encode_wide(OsStr::new(""));

        let result = unsafe {
            ShellExecuteW(
                None,
                PCWSTR(verb_w.as_ptr()),
                PCWSTR(exe_w.as_ptr()),
                PCWSTR(params_w.as_ptr()),
                PCWSTR(empty_w.as_ptr()),
                SW_HIDE,
            )
        };

        // Win32 约定：<= 32 代表失败。
        if result.0 as isize <= 32 {
            return Err(EngineError::Remote(format!(
                "failed to launch elevated helper via UAC (code={})",
                result.0 as isize
            )));
        }

        Ok(())
    }

    /// 建立到 helper 的一次性连接，发送命令并等待响应。
    ///
    /// 当前实现刻意保持“短连接 + 单请求单响应”，
    /// 简化 UI/重连/陈旧会话处理，不维护长连接状态机。
    fn send_command(
        &self,
        session: &HelperSession,
        command: BackendCommand,
    ) -> Result<BackendReply, EngineError> {
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, session.port));
        let connect_timeout = HELPER_IO_TIMEOUT.min(Duration::from_secs(5));
        let mut stream = TcpStream::connect_timeout(&addr, connect_timeout)
            .map_err(|_| EngineError::ChannelClosed)?;
        let _ = stream.set_read_timeout(Some(HELPER_IO_TIMEOUT));
        let _ = stream.set_write_timeout(Some(HELPER_IO_TIMEOUT));

        let envelope = HelperEnvelope {
            secret: session.secret.clone(),
            command,
        };
        write_json_line(&mut stream, &envelope).map_err(|_| EngineError::ChannelClosed)?;

        let mut reader = BufReader::new(stream);
        read_json_line(&mut reader).map_err(|_| EngineError::ChannelClosed)
    }

    /// 把“只关心成功/失败”的响应统一转换成 `Result<()>`。
    fn expect_unit(&self, reply: BackendReply) -> Result<(), EngineError> {
        match reply {
            BackendReply::Ok => Ok(()),
            BackendReply::Error { kind, message } => Err(map_backend_error(kind, message)),
            other => Err(unexpected_reply(other)),
        }
    }
}

/// 管理员 helper 主循环。
///
/// 生命周期规则：
/// - 先监听 127.0.0.1 的临时端口；
/// - 把 session 写到磁盘供 UI 发现；
/// - 进入 accept 循环处理命令；
/// - 当没有隧道运行且空闲超时后自动退出。
fn run_elevated_helper(session_file: PathBuf) -> Result<(), EngineError> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|err| {
        EngineError::Remote(format!("failed to bind elevated helper listener: {err}"))
    })?;
    listener.set_nonblocking(true).map_err(|err| {
        EngineError::Remote(format!(
            "failed to configure elevated helper listener: {err}"
        ))
    })?;

    let session = HelperSession {
        port: listener
            .local_addr()
            .map_err(|err| EngineError::Remote(format!("failed to read helper addr: {err}")))?
            .port(),
        secret: random_secret()?,
    };
    write_session_file(&session_file, &session)?;

    let engine = LocalEngine::new();
    let mut last_activity = Instant::now();

    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                last_activity = Instant::now();
                let _ = handle_helper_client(stream, &engine, &session.secret);
            }
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                let running = matches!(engine.status(), Ok(EngineStatus::Running));
                if !running && last_activity.elapsed() >= HELPER_IDLE_TIMEOUT {
                    break;
                }
                thread::sleep(HELPER_POLL_INTERVAL);
            }
            Err(_) => {
                thread::sleep(HELPER_POLL_INTERVAL);
            }
        }
    }

    let _ = fs::remove_file(session_file);
    Ok(())
}

/// 处理单个 helper 客户端请求。
///
/// 先鉴权，再串行调用本地引擎。
fn handle_helper_client(
    mut stream: TcpStream,
    engine: &LocalEngine,
    secret: &str,
) -> io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let envelope: HelperEnvelope = read_json_line(&mut reader)?;

    let reply = if envelope.secret != secret {
        BackendReply::Error {
            kind: super::ipc::BackendErrorKind::Other,
            message: "helper authentication failed".to_string(),
        }
    } else {
        match envelope.command {
            BackendCommand::Ping => BackendReply::Ok,
            BackendCommand::Info => BackendReply::Info {
                protocol_version: super::ipc::IPC_PROTOCOL_VERSION,
            },
            BackendCommand::Start { request } => unit_reply(engine.start(request)),
            BackendCommand::Stop => unit_reply(engine.stop()),
            BackendCommand::Status => match engine.status() {
                Ok(status) => BackendReply::Status { status },
                Err(err) => error_reply(err),
            },
            BackendCommand::Stats => match engine.stats() {
                Ok(stats) => BackendReply::Stats { stats },
                Err(err) => error_reply(err),
            },
        }
    };

    write_json_line(&mut stream, &reply)
}

/// 把 helper session 持久化到用户本地目录。
///
/// 这里故意用文件而不是全局静态状态：
/// - UI 重启后还能重新发现已存在的 helper；
/// - 启动期无需额外共享内存或注册表。
fn write_session_file(path: &Path, session: &HelperSession) -> Result<(), EngineError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            EngineError::Remote(format!("failed to create helper session dir: {err}"))
        })?;
    }
    let json = serde_json::to_string(session)
        .map_err(|err| EngineError::Remote(format!("failed to encode helper session: {err}")))?;
    fs::write(path, json)
        .map_err(|err| EngineError::Remote(format!("failed to write helper session: {err}")))
}

/// helper session 文件的默认位置。
///
/// 使用用户本地数据目录，避免污染工作目录，也避免多用户之间相互覆盖。
fn helper_session_path() -> PathBuf {
    let base = dirs::data_local_dir().unwrap_or_else(env::temp_dir);
    base.join("r-wg").join("windows-elevated-helper.json")
}

/// 判断当前进程是否已经是“UAC 提升后的管理员进程”。
///
/// 这里只看 `TokenElevation`，而不是“当前用户是否属于管理员组”，
/// 因为 split-token 场景下两者并不等价。
fn is_process_elevated() -> bool {
    unsafe {
        let mut token = Default::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }

        let mut elevation = TOKEN_ELEVATION::default();
        let mut out_len = 0u32;
        let result = GetTokenInformation(
            token,
            TokenElevation,
            Some((&mut elevation as *mut TOKEN_ELEVATION).cast()),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut out_len,
        );
        let _ = CloseHandle(token);
        result.is_ok() && elevation.TokenIsElevated != 0
    }
}

/// 生成 helper 通信 secret。
///
/// 使用系统随机源，避免把 secret 退化成可预测字符串。
fn random_secret() -> Result<String, EngineError> {
    let mut bytes = [0u8; 32];
    let status = unsafe { BCryptGenRandom(None, &mut bytes, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
    if status.0 < 0 {
        return Err(EngineError::Remote(
            "failed to generate helper secret".to_string(),
        ));
    }
    Ok(hex_encode(&bytes))
}

/// 把随机字节编码成十六进制，便于放进 JSON/session 文件。
fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

/// Win32 API 需要的 UTF-16 零结尾字符串。
fn encode_wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

/// Windows 命令行参数引用规则。
///
/// `ShellExecuteW` 这里仍然是“单个字符串参数”，所以路径里有空格时必须自己转义。
fn quote_windows_arg(value: &str) -> String {
    if !value.chars().any(|ch| ch.is_whitespace() || ch == '"') {
        return value.to_string();
    }

    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    let mut backslashes = 0usize;
    for ch in value.chars() {
        match ch {
            '\\' => backslashes += 1,
            '"' => {
                quoted.extend(std::iter::repeat('\\').take(backslashes * 2 + 1));
                quoted.push('"');
                backslashes = 0;
            }
            _ => {
                quoted.extend(std::iter::repeat('\\').take(backslashes));
                backslashes = 0;
                quoted.push(ch);
            }
        }
    }
    quoted.extend(std::iter::repeat('\\').take(backslashes * 2));
    quoted.push('"');
    quoted
}
