//! Linux 特权后端：
//! - 普通 UI/CLI 通过 Unix socket 与常驻 systemd service 通信；
//! - 真正持有 TUN / 路由 / DNS 生命周期的是 root service；
//! - 第一版保持常驻 service，不做 socket activation；
//! - 开发期可通过 `service install/repair/remove` + `pkexec` 管理安装。
use std::env;
use std::ffi::{CString, OsString};
use std::fs;
use std::io::{self, BufReader};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use super::engine::Engine as LocalEngine;
use super::ipc::{
    map_backend_error, protocol_mismatch, read_json_line, unexpected_reply, write_json_line,
    BackendCommand, BackendReply, IPC_PROTOCOL_VERSION,
};
use super::{EngineError, EngineStats, EngineStatus, StartRequest};

const SERVICE_ARG: &str = "--linux-service";
const SERVICE_SUBCOMMAND: &str = "service";
const DEFAULT_SOCKET_PATH: &str = "/run/r-wg/control.sock";
const DEFAULT_SOCKET_GROUP: &str = "r-wg";
const DEFAULT_INSTALLED_BINARY: &str = "/usr/local/libexec/r-wg/r-wg";
const DEFAULT_UNIT_PATH: &str = "/etc/systemd/system/r-wg.service";
const DEFAULT_SOCKET_UNIT_PATH: &str = "/etc/systemd/system/r-wg.socket";
const SERVICE_UNIT_NAME: &str = "r-wg.service";
const SOCKET_UNIT_NAME: &str = "r-wg.socket";
const SERVICE_POLL_INTERVAL: Duration = Duration::from_millis(200);
const SERVICE_IO_TIMEOUT: Duration = Duration::from_secs(30);
const SERVICE_IDLE_TIMEOUT: Duration = Duration::from_secs(15);

/// Linux 特权 backend 的当前探测状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrivilegedServiceStatus {
    Running,
    Installed,
    NotInstalled,
    AccessDenied,
    VersionMismatch { expected: u32, actual: u32 },
    Unreachable(String),
}

/// 设置页可触发的特权 backend 管理动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivilegedServiceAction {
    Install,
    Repair,
    Remove,
}

impl PrivilegedServiceAction {
    fn as_cli(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Repair => "repair",
            Self::Remove => "remove",
        }
    }
}

#[derive(Clone)]
pub struct Engine {
    inner: Arc<RemoteEngine>,
}

#[derive(Clone)]
struct RemoteEngine {
    socket_path: Arc<PathBuf>,
}

enum LinuxEntryCommand {
    ServiceMode(ServiceOptions),
    Manage(ManageCommand),
}

struct ServiceOptions {
    socket_path: PathBuf,
    socket_group: Option<String>,
    allowed_uid: Option<u32>,
}

enum ManageCommand {
    Install(InstallOptions),
    Repair(InstallOptions),
    Remove(RemoveOptions),
}

struct InstallOptions {
    source_path: PathBuf,
    binary_path: PathBuf,
    unit_path: PathBuf,
    socket_unit_path: PathBuf,
    socket_group: Option<String>,
    socket_user: Option<String>,
    allowed_uid: Option<u32>,
}

struct RemoveOptions {
    binary_path: PathBuf,
    unit_path: PathBuf,
    socket_unit_path: PathBuf,
}

#[derive(Debug, Clone, Copy)]
struct PeerCredentials {
    pid: u32,
    uid: u32,
}

/// UI 侧探测 Linux 特权 service 是否可用。
pub fn probe_privileged_service() -> PrivilegedServiceStatus {
    // 注意：这里不能“无脑尝试连接 socket”来判断 Installed。
    // 在 socket activation 模式下，探测动作本身会把 service 拉起来，
    // 这会把一个原本只是“已安装待命”的后端误判成“正在运行”。
    if !installation_exists() {
        return PrivilegedServiceStatus::NotInstalled;
    }

    if systemd_unit_is_active(SERVICE_UNIT_NAME) {
        let engine = RemoteEngine::new();
        return match engine.send_command_raw(BackendCommand::Info) {
            Ok(BackendReply::Info { protocol_version }) => {
                if protocol_version == IPC_PROTOCOL_VERSION {
                    PrivilegedServiceStatus::Running
                } else {
                    PrivilegedServiceStatus::VersionMismatch {
                        expected: IPC_PROTOCOL_VERSION,
                        actual: protocol_version,
                    }
                }
            }
            Ok(other) => {
                PrivilegedServiceStatus::Unreachable(format!("unexpected backend reply: {other:?}"))
            }
            Err(err) if is_access_denied_error(&err) => PrivilegedServiceStatus::AccessDenied,
            Err(err) => PrivilegedServiceStatus::Unreachable(format!(
                "failed to reach Linux privileged backend: {err}"
            )),
        };
    }

    PrivilegedServiceStatus::Installed
}

/// 从普通 UI 进程触发 install/repair/remove。
///
/// - 默认优先走 `pkexec`；
/// - 已经 root 运行时直接执行当前二进制的管理命令；
/// - install/repair 会把当前 exe 复制到 root-owned 路径，再写入 unit 文件。
pub fn manage_privileged_service(action: PrivilegedServiceAction) -> Result<(), EngineError> {
    let current_exe = env::current_exe()
        .map_err(|err| remote_error(format!("failed to locate current exe: {err}")))?;

    let mut args = vec![
        OsString::from(SERVICE_SUBCOMMAND),
        OsString::from(action.as_cli()),
    ];
    if !matches!(action, PrivilegedServiceAction::Remove) {
        let current_uid = unsafe { libc::getuid() };
        // 这里显式把“发起安装的原始用户”编码进 root 管理命令：
        // - pkexec 会清洗大部分环境变量，不能依赖外层 shell 上下文；
        // - 后端安装完成后需要把 socket 权限/allowed uid 定向给当前桌面用户，
        //   否则最容易出现“安装成功，但当前用户依然打不开控制 socket”。
        args.push(OsString::from("--source"));
        args.push(current_exe.as_os_str().to_os_string());
        args.push(OsString::from("--socket-group"));
        args.push(OsString::from("none"));
        args.push(OsString::from("--allowed-uid"));
        args.push(OsString::from(current_uid.to_string()));
        if let Some(user) = current_username(current_uid) {
            args.push(OsString::from("--socket-user"));
            args.push(OsString::from(user));
        }
    }

    let output = if is_running_as_root() {
        Command::new(&current_exe).args(&args).output()
    } else {
        Command::new("pkexec")
            .arg(&current_exe)
            .args(&args)
            .output()
    }
    .map_err(|err| {
        if err.kind() == io::ErrorKind::NotFound && !is_running_as_root() {
            remote_error(
                "pkexec not found. Install polkit or run the service command as root.".to_string(),
            )
        } else {
            remote_error(format!(
                "failed to launch privileged backend manager: {err}"
            ))
        }
    })?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let message = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        format!("privileged backend manager exited with {}", output.status)
    };
    Err(remote_error(message))
}

/// 入口早分流：当前进程是否应接管为 Linux system service 或其管理命令。
pub fn maybe_run_service_mode() -> bool {
    // 和 Windows helper 一样，Linux 的 service/install/remove 都必须在 UI 初始化前分流：
    // 一旦已经进入 GPUI 应用生命周期，再切换成 system service / root 管理命令就太晚了。
    let entry = match parse_linux_entry_command(env::args_os()) {
        Ok(entry) => entry,
        Err(err) => {
            crate::log::init();
            tracing::error!("linux privileged backend command parse failed: {err}");
            return true;
        }
    };
    let Some(entry) = entry else {
        return false;
    };

    crate::log::init();

    match entry {
        LinuxEntryCommand::ServiceMode(options) => {
            let _mtu = gotatun::tun::MtuWatcher::new(1500);
            if let Err(err) = run_service(options) {
                tracing::error!("linux service exited: {err}");
            }
        }
        LinuxEntryCommand::Manage(command) => {
            if let Err(err) = run_manage_command(command) {
                tracing::error!("{err}");
            }
        }
    }

    true
}

impl Engine {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RemoteEngine::new()),
        }
    }

    pub fn start(&self, request: StartRequest) -> Result<(), EngineError> {
        self.inner.start(request)
    }

    pub fn stop(&self) -> Result<(), EngineError> {
        self.inner.stop()
    }

    pub fn status(&self) -> Result<EngineStatus, EngineError> {
        self.inner.status()
    }

    pub fn stats(&self) -> Result<EngineStats, EngineError> {
        self.inner.stats()
    }
}

impl RemoteEngine {
    fn new() -> Self {
        Self {
            socket_path: Arc::new(control_socket_path()),
        }
    }

    fn start(&self, request: StartRequest) -> Result<(), EngineError> {
        let reply = self.send_command(BackendCommand::Start { request })?;
        self.expect_unit(reply)
    }

    fn stop(&self) -> Result<(), EngineError> {
        match self.send_command_raw(BackendCommand::Stop) {
            Ok(reply) => self.expect_unit(reply),
            Err(err) if is_missing_backend_error(&err) => Err(EngineError::NotRunning),
            Err(err) if is_access_denied_error(&err) => Err(EngineError::AccessDenied),
            Err(err) => Err(connect_error(self.socket_path.as_path(), err)),
        }
    }

    fn status(&self) -> Result<EngineStatus, EngineError> {
        match self.send_command_raw(BackendCommand::Status) {
            Ok(BackendReply::Status { status }) => Ok(status),
            Ok(BackendReply::Error { kind, message }) => Err(map_backend_error(kind, message)),
            Ok(other) => Err(unexpected_reply(other)),
            Err(err) if is_missing_backend_error(&err) => Ok(EngineStatus::Stopped),
            Err(err) if is_access_denied_error(&err) => Err(EngineError::AccessDenied),
            Err(err) => Err(connect_error(self.socket_path.as_path(), err)),
        }
    }

    fn stats(&self) -> Result<EngineStats, EngineError> {
        match self.send_command_raw(BackendCommand::Stats) {
            Ok(BackendReply::Stats { stats }) => Ok(stats),
            Ok(BackendReply::Error { kind, message }) => Err(map_backend_error(kind, message)),
            Ok(other) => Err(unexpected_reply(other)),
            Err(err) if is_missing_backend_error(&err) => Err(EngineError::NotRunning),
            Err(err) if is_access_denied_error(&err) => Err(EngineError::AccessDenied),
            Err(err) => Err(connect_error(self.socket_path.as_path(), err)),
        }
    }

    fn send_command(&self, command: BackendCommand) -> Result<BackendReply, EngineError> {
        self.send_command_raw(command)
            .map_err(|err| connect_error(self.socket_path.as_path(), err))
    }

    fn send_command_raw(&self, command: BackendCommand) -> Result<BackendReply, io::Error> {
        let mut stream = UnixStream::connect(self.socket_path.as_path())?;
        let _ = stream.set_read_timeout(Some(SERVICE_IO_TIMEOUT));
        let _ = stream.set_write_timeout(Some(SERVICE_IO_TIMEOUT));
        write_json_line(&mut stream, &command)?;
        let mut reader = BufReader::new(stream);
        read_json_line(&mut reader)
    }

    fn expect_unit(&self, reply: BackendReply) -> Result<(), EngineError> {
        match reply {
            BackendReply::Ok => Ok(()),
            BackendReply::Error { kind, message } => Err(map_backend_error(kind, message)),
            other => Err(unexpected_reply(other)),
        }
    }
}

fn run_service(options: ServiceOptions) -> Result<(), EngineError> {
    let socket_gid = match options.socket_group.as_deref() {
        Some(group) => Some(lookup_group_gid(group)?),
        None => None,
    };
    let listener = if let Some(listener) = inherited_listener()? {
        // systemd socket activation 路径：
        // socket 已经由 PID 1 创建并设置好权限，这里只接管 fd=3 开始 accept。
        listener
    } else {
        // 直接运行 `r-wg --linux-service` 的兜底路径：
        // 允许开发时绕过 systemd 手动拉起 service，但正式安装路径仍然优先使用 socket unit。
        if let Some(parent) = options.socket_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| remote_error(format!("failed to create runtime dir: {err}")))?;
        }
        remove_stale_socket(options.socket_path.as_path())?;
        let listener = UnixListener::bind(options.socket_path.as_path()).map_err(|err| {
            remote_error(format!(
                "failed to bind Linux privileged backend socket {}: {err}",
                options.socket_path.display()
            ))
        })?;
        configure_socket_permissions(options.socket_path.as_path(), socket_gid)?;
        listener
    };
    listener.set_nonblocking(true).map_err(|err| {
        remote_error(format!(
            "failed to configure Linux privileged backend socket: {err}"
        ))
    })?;

    let engine = LocalEngine::new();
    let mut last_activity = std::time::Instant::now();

    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                last_activity = std::time::Instant::now();
                let _ = handle_service_client(stream, &engine, options.allowed_uid, socket_gid);
            }
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                let running = matches!(engine.status(), Ok(EngineStatus::Running));
                if !running && last_activity.elapsed() >= SERVICE_IDLE_TIMEOUT {
                    // socket activation 下，service 不需要永久驻留；
                    // 空闲退出后，下一次 UI 访问 socket 会由 systemd 再次自动拉起。
                    break Ok(());
                }
                thread::sleep(SERVICE_POLL_INTERVAL);
            }
            Err(err) => {
                tracing::warn!("linux service accept failed: {err}");
                thread::sleep(SERVICE_POLL_INTERVAL);
            }
        }
    }
}

fn run_manage_command(command: ManageCommand) -> Result<(), EngineError> {
    ensure_root()?;
    match command {
        ManageCommand::Install(options) => install_or_repair(options, false),
        ManageCommand::Repair(options) => install_or_repair(options, true),
        ManageCommand::Remove(options) => remove_installation(options),
    }
}

fn install_or_repair(options: InstallOptions, repairing: bool) -> Result<(), EngineError> {
    if !options.source_path.is_file() {
        return Err(remote_error(format!(
            "source binary not found: {}",
            options.source_path.display()
        )));
    }

    if let Some(group) = options.socket_group.as_deref() {
        ensure_group_exists(group)?;
    }

    install_binary(&options.source_path, &options.binary_path)?;
    write_service_unit(
        &options.unit_path,
        render_service_unit(
            &options.binary_path,
            options.socket_group.as_deref(),
            options.allowed_uid,
        ),
    )?;
    write_service_unit(
        &options.socket_unit_path,
        render_socket_unit(
            options.socket_user.as_deref(),
            options.socket_group.as_deref(),
        ),
    )?;

    run_command("systemctl", ["daemon-reload"])?;
    run_command("systemctl", ["enable", SOCKET_UNIT_NAME])?;
    if repairing {
        // repair 需要重启 socket unit，确保旧 socket 权限和 unit 内容被完整替换。
        run_command("systemctl", ["restart", SOCKET_UNIT_NAME])?;
    } else {
        // install 只启动 socket，不主动启动 service；
        // 后端会在首次连接时由 systemd 按需拉起。
        run_command("systemctl", ["start", SOCKET_UNIT_NAME])?;
    }

    Ok(())
}

fn remove_installation(options: RemoveOptions) -> Result<(), EngineError> {
    let _ = run_command("systemctl", ["disable", "--now", SOCKET_UNIT_NAME]);
    let _ = run_command("systemctl", ["stop", SERVICE_UNIT_NAME]);

    if options.unit_path.exists() {
        fs::remove_file(&options.unit_path).map_err(|err| {
            remote_error(format!(
                "failed to remove service unit {}: {err}",
                options.unit_path.display()
            ))
        })?;
    }
    if options.socket_unit_path.exists() {
        fs::remove_file(&options.socket_unit_path).map_err(|err| {
            remote_error(format!(
                "failed to remove socket unit {}: {err}",
                options.socket_unit_path.display()
            ))
        })?;
    }

    let _ = run_command("systemctl", ["daemon-reload"]);

    if options.binary_path.exists() {
        fs::remove_file(&options.binary_path).map_err(|err| {
            remote_error(format!(
                "failed to remove installed binary {}: {err}",
                options.binary_path.display()
            ))
        })?;
    }

    if let Some(parent) = options.binary_path.parent() {
        match fs::remove_dir(parent) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) if err.kind() == io::ErrorKind::DirectoryNotEmpty => {}
            Err(err) => {
                return Err(remote_error(format!(
                    "failed to clean install dir {}: {err}",
                    parent.display()
                )));
            }
        }
    }

    Ok(())
}

fn handle_service_client(
    mut stream: UnixStream,
    engine: &LocalEngine,
    allowed_uid: Option<u32>,
    allowed_gid: Option<u32>,
) -> io::Result<()> {
    // 服务端即使已经依赖 socket 文件权限，也仍然额外校验 peer credentials。
    // 这样即使 socket 权限被误改宽，也能在应用层再挡一次不可信调用方。
    let reply = match peer_credentials(&stream) {
        Ok(creds) if is_peer_allowed(creds, allowed_uid, allowed_gid) => {
            handle_command(&mut stream, engine)?
        }
        Ok(_) => BackendReply::Error {
            kind: super::ipc::BackendErrorKind::AccessDenied,
            message: "peer is not allowed to access Linux privileged backend".to_string(),
        },
        Err(err) => BackendReply::Error {
            kind: super::ipc::BackendErrorKind::AccessDenied,
            message: format!("failed to inspect peer credentials: {err}"),
        },
    };

    write_json_line(&mut stream, &reply)
}

fn handle_command(stream: &mut UnixStream, engine: &LocalEngine) -> io::Result<BackendReply> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let command: BackendCommand = read_json_line(&mut reader)?;

    Ok(match command {
        BackendCommand::Ping => BackendReply::Ok,
        BackendCommand::Info => BackendReply::Info {
            protocol_version: IPC_PROTOCOL_VERSION,
        },
        BackendCommand::Start { request } => super::ipc::unit_reply(engine.start(request)),
        BackendCommand::Stop => super::ipc::unit_reply(engine.stop()),
        BackendCommand::Status => match engine.status() {
            Ok(status) => BackendReply::Status { status },
            Err(err) => super::ipc::error_reply(err),
        },
        BackendCommand::Stats => match engine.stats() {
            Ok(stats) => BackendReply::Stats { stats },
            Err(err) => super::ipc::error_reply(err),
        },
    })
}

fn peer_credentials(stream: &UnixStream) -> io::Result<PeerCredentials> {
    let mut creds = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut creds as *mut libc::ucred).cast(),
            &mut len,
        )
    };
    if rc == 0 {
        Ok(PeerCredentials {
            pid: creds.pid as u32,
            uid: creds.uid,
        })
    } else {
        Err(io::Error::last_os_error())
    }
}

fn is_peer_allowed(
    creds: PeerCredentials,
    allowed_uid: Option<u32>,
    allowed_gid: Option<u32>,
) -> bool {
    if creds.uid == 0 {
        return true;
    }
    if allowed_uid.is_some_and(|uid| uid == creds.uid) {
        return true;
    }
    if let Some(gid) = allowed_gid {
        return peer_in_group(creds.pid, gid).unwrap_or(false);
    }
    true
}

fn peer_in_group(pid: u32, wanted_gid: u32) -> io::Result<bool> {
    // Linux UDS 的 SO_PEERCRED 只直接给 pid/uid/gid；
    // 若要判断“调用方是否属于某个附加组”，最稳妥的本地办法就是读 /proc/<pid>/status。
    let status = fs::read_to_string(format!("/proc/{pid}/status"))?;
    let groups = status
        .lines()
        .find(|line| line.starts_with("Groups:"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Groups line missing"))?;

    Ok(groups
        .split_whitespace()
        .skip(1)
        .filter_map(|value| value.parse::<u32>().ok())
        .any(|gid| gid == wanted_gid))
}

fn control_socket_path() -> PathBuf {
    env::var_os("RWG_CONTROL_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_SOCKET_PATH))
}

fn installation_exists() -> bool {
    Path::new(DEFAULT_UNIT_PATH).exists()
        || Path::new(DEFAULT_SOCKET_UNIT_PATH).exists()
        || Path::new(DEFAULT_INSTALLED_BINARY).exists()
}

fn parse_linux_entry_command(
    args: impl IntoIterator<Item = OsString>,
) -> Result<Option<LinuxEntryCommand>, EngineError> {
    let mut args = args.into_iter();
    let _ = args.next();
    let Some(first) = args.next() else {
        return Ok(None);
    };

    if first == OsString::from(SERVICE_ARG) {
        let options = parse_service_mode_args(args)?;
        return Ok(Some(LinuxEntryCommand::ServiceMode(options)));
    }
    if first == OsString::from(SERVICE_SUBCOMMAND) {
        let command = parse_manage_command(args)?;
        return Ok(Some(LinuxEntryCommand::Manage(command)));
    }
    Ok(None)
}

fn parse_service_mode_args(
    args: impl IntoIterator<Item = OsString>,
) -> Result<ServiceOptions, EngineError> {
    let mut socket_path = control_socket_path();
    let mut socket_group = Some(DEFAULT_SOCKET_GROUP.to_string());
    let mut allowed_uid = None;

    let mut pending = None::<String>;
    for arg in args {
        let arg = arg.to_string_lossy().to_string();
        match pending.take().as_deref() {
            Some("socket") => {
                socket_path = PathBuf::from(arg);
                continue;
            }
            Some("socket_group") => {
                socket_group = if arg.eq_ignore_ascii_case("none") {
                    None
                } else {
                    Some(arg)
                };
                continue;
            }
            Some("allowed_uid") => {
                allowed_uid =
                    Some(arg.parse().map_err(|_| {
                        remote_error(format!("invalid --allowed-uid value: {arg}"))
                    })?);
                continue;
            }
            Some(other) => {
                return Err(remote_error(format!(
                    "unknown pending service arg: {other}"
                )));
            }
            None => {}
        }

        match arg.as_str() {
            "--socket" => pending = Some("socket".to_string()),
            "--socket-group" => pending = Some("socket_group".to_string()),
            "--allowed-uid" => pending = Some("allowed_uid".to_string()),
            other => return Err(remote_error(format!("unknown Linux service arg: {other}"))),
        }
    }

    if let Some(flag) = pending {
        return Err(remote_error(format!(
            "missing value for --{}",
            flag.replace('_', "-")
        )));
    }

    Ok(ServiceOptions {
        socket_path,
        socket_group,
        allowed_uid,
    })
}

fn parse_manage_command(
    args: impl IntoIterator<Item = OsString>,
) -> Result<ManageCommand, EngineError> {
    let mut args = args.into_iter();
    let action = args.next().ok_or_else(|| {
        remote_error("missing service action (install/repair/remove)".to_string())
    })?;

    let mut source_path = None;
    let mut binary_path = PathBuf::from(DEFAULT_INSTALLED_BINARY);
    let mut unit_path = PathBuf::from(DEFAULT_UNIT_PATH);
    let mut socket_unit_path = PathBuf::from(DEFAULT_SOCKET_UNIT_PATH);
    let mut socket_group = Some(DEFAULT_SOCKET_GROUP.to_string());
    let mut socket_user = None;
    let mut allowed_uid = None;
    let mut pending = None::<String>;

    for arg in args {
        let arg = arg.to_string_lossy().to_string();
        match pending.take().as_deref() {
            Some("source") => {
                source_path = Some(PathBuf::from(arg));
                continue;
            }
            Some("binary_path") => {
                binary_path = PathBuf::from(arg);
                continue;
            }
            Some("unit_path") => {
                unit_path = PathBuf::from(arg);
                continue;
            }
            Some("socket_unit_path") => {
                socket_unit_path = PathBuf::from(arg);
                continue;
            }
            Some("socket_group") => {
                socket_group = if arg.eq_ignore_ascii_case("none") {
                    None
                } else {
                    Some(arg)
                };
                continue;
            }
            Some("socket_user") => {
                socket_user = if arg.eq_ignore_ascii_case("none") {
                    None
                } else {
                    Some(arg)
                };
                continue;
            }
            Some("allowed_uid") => {
                allowed_uid =
                    Some(arg.parse().map_err(|_| {
                        remote_error(format!("invalid --allowed-uid value: {arg}"))
                    })?);
                continue;
            }
            Some(other) => {
                return Err(remote_error(format!(
                    "unknown pending service arg: {other}"
                )));
            }
            None => {}
        }

        match arg.as_str() {
            "--source" => pending = Some("source".to_string()),
            "--binary-path" => pending = Some("binary_path".to_string()),
            "--unit-path" => pending = Some("unit_path".to_string()),
            "--socket-unit-path" => pending = Some("socket_unit_path".to_string()),
            "--socket-group" => pending = Some("socket_group".to_string()),
            "--socket-user" => pending = Some("socket_user".to_string()),
            "--allowed-uid" => pending = Some("allowed_uid".to_string()),
            other => {
                return Err(remote_error(format!(
                    "unknown service management arg: {other}"
                )))
            }
        }
    }

    if let Some(flag) = pending {
        return Err(remote_error(format!(
            "missing value for --{}",
            flag.replace('_', "-")
        )));
    }

    match action.to_string_lossy().as_ref() {
        "install" => Ok(ManageCommand::Install(InstallOptions {
            source_path: source_path
                .ok_or_else(|| remote_error("service install requires --source".to_string()))?,
            binary_path,
            unit_path,
            socket_unit_path,
            socket_group,
            socket_user,
            allowed_uid,
        })),
        "repair" => Ok(ManageCommand::Repair(InstallOptions {
            source_path: source_path
                .ok_or_else(|| remote_error("service repair requires --source".to_string()))?,
            binary_path,
            unit_path,
            socket_unit_path,
            socket_group,
            socket_user,
            allowed_uid,
        })),
        // remove 不需要 source/socket owner 这些安装期元信息，只需要知道清理哪些目标路径。
        "remove" => Ok(ManageCommand::Remove(RemoveOptions {
            binary_path,
            unit_path,
            socket_unit_path,
        })),
        other => Err(remote_error(format!("unknown service action: {other}"))),
    }
}

fn remove_stale_socket(path: &Path) -> Result<(), EngineError> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Ok(());
    };
    if metadata.file_type().is_socket() {
        fs::remove_file(path)
            .map_err(|err| remote_error(format!("failed to remove stale socket: {err}")))?;
        return Ok(());
    }
    Err(remote_error(format!(
        "refusing to replace non-socket path at {}",
        path.display()
    )))
}

fn configure_socket_permissions(path: &Path, socket_gid: Option<u32>) -> Result<(), EngineError> {
    if let Some(gid) = socket_gid {
        let c_path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| remote_error("socket path contains interior NUL".to_string()))?;
        let rc = unsafe { libc::chown(c_path.as_ptr(), u32::MAX, gid) };
        if rc != 0 {
            return Err(remote_error(format!(
                "failed to change socket group: {}",
                io::Error::last_os_error()
            )));
        }
    }

    fs::set_permissions(path, fs::Permissions::from_mode(0o660))
        .map_err(|err| remote_error(format!("failed to chmod control socket: {err}")))
}

fn lookup_group_gid(group: &str) -> Result<u32, EngineError> {
    let group_c = CString::new(group)
        .map_err(|_| remote_error("socket group contains interior NUL".to_string()))?;
    let mut grp = std::mem::MaybeUninit::<libc::group>::uninit();
    let mut result = std::ptr::null_mut();
    let mut buf = vec![0u8; 1024];

    loop {
        let rc = unsafe {
            libc::getgrnam_r(
                group_c.as_ptr(),
                grp.as_mut_ptr(),
                buf.as_mut_ptr().cast(),
                buf.len(),
                &mut result,
            )
        };
        if rc == 0 {
            if result.is_null() {
                return Err(remote_error(format!("socket group not found: {group}")));
            }
            let group = unsafe { grp.assume_init() };
            return Ok(group.gr_gid);
        }
        if rc == libc::ERANGE {
            buf.resize(buf.len() * 2, 0);
            continue;
        }
        return Err(remote_error(format!(
            "failed to resolve socket group {group}: {}",
            io::Error::from_raw_os_error(rc)
        )));
    }
}

fn ensure_group_exists(group: &str) -> Result<(), EngineError> {
    if lookup_group_gid(group).is_ok() {
        return Ok(());
    }
    run_command("groupadd", ["--system", group])
}

fn install_binary(source_path: &Path, binary_path: &Path) -> Result<(), EngineError> {
    let install_dir = binary_path
        .parent()
        .ok_or_else(|| remote_error("binary install path has no parent".to_string()))?;
    fs::create_dir_all(install_dir).map_err(|err| {
        remote_error(format!(
            "failed to create install dir {}: {err}",
            install_dir.display()
        ))
    })?;
    fs::set_permissions(install_dir, fs::Permissions::from_mode(0o755)).map_err(|err| {
        remote_error(format!(
            "failed to chmod install dir {}: {err}",
            install_dir.display()
        ))
    })?;

    let temp_path = install_dir.join(".r-wg.install.tmp");
    fs::copy(source_path, &temp_path).map_err(|err| {
        remote_error(format!(
            "failed to copy {} -> {}: {err}",
            source_path.display(),
            temp_path.display()
        ))
    })?;
    fs::set_permissions(&temp_path, fs::Permissions::from_mode(0o755))
        .map_err(|err| remote_error(format!("failed to chmod installed binary: {err}")))?;
    fs::rename(&temp_path, binary_path).map_err(|err| {
        remote_error(format!(
            "failed to place installed binary {}: {err}",
            binary_path.display()
        ))
    })
}

fn write_service_unit(unit_path: &Path, contents: String) -> Result<(), EngineError> {
    let parent = unit_path
        .parent()
        .ok_or_else(|| remote_error("service unit path has no parent".to_string()))?;
    fs::create_dir_all(parent).map_err(|err| {
        remote_error(format!(
            "failed to create service unit dir {}: {err}",
            parent.display()
        ))
    })?;

    let temp_path = parent.join(".r-wg.service.tmp");
    fs::write(&temp_path, contents)
        .map_err(|err| remote_error(format!("failed to write temp service unit: {err}")))?;
    fs::set_permissions(&temp_path, fs::Permissions::from_mode(0o644))
        .map_err(|err| remote_error(format!("failed to chmod service unit: {err}")))?;
    fs::rename(&temp_path, unit_path).map_err(|err| {
        remote_error(format!(
            "failed to place service unit {}: {err}",
            unit_path.display()
        ))
    })
}

fn render_service_unit(
    binary_path: &Path,
    socket_group: Option<&str>,
    allowed_uid: Option<u32>,
) -> String {
    let mut exec_start = format!("{}", binary_path.display());
    exec_start.push_str(" --linux-service");
    // socket unit 负责“谁能连上 socket”，service unit 负责“service 端额外信任谁”。
    // 因此 allowed uid 也要进 ExecStart，保证服务端 peer 校验与安装时的授权对象一致。
    if let Some(group) = socket_group {
        exec_start.push_str(" --socket-group ");
        exec_start.push_str(group);
    }
    if let Some(uid) = allowed_uid {
        exec_start.push_str(" --allowed-uid ");
        exec_start.push_str(&uid.to_string());
    }
    format!(
        "[Unit]\nDescription=r-wg privileged backend\nAfter=network-online.target\nWants=network-online.target\nRequires=r-wg.socket\n\n[Service]\nType=simple\nExecStart={exec_start}\nRestart=on-failure\nRestartSec=1\nRuntimeDirectory=r-wg\nRuntimeDirectoryMode=0755\nNoNewPrivileges=yes\n\n[Install]\nWantedBy=multi-user.target\n"
    )
}

fn render_socket_unit(socket_user: Option<&str>, socket_group: Option<&str>) -> String {
    let mut unit = format!(
        "[Unit]\nDescription=r-wg privileged backend socket\n\n[Socket]\nListenStream={DEFAULT_SOCKET_PATH}\n"
    );
    match (socket_user, socket_group) {
        (Some(user), _) => {
            // 对开发机的默认安装流，优先把 socket 直接给发起安装的桌面用户，
            // 避免还要额外处理 group membership 和重新登录的问题。
            unit.push_str("SocketMode=0600\n");
            unit.push_str("SocketUser=");
            unit.push_str(user);
            unit.push('\n');
        }
        (None, Some(group)) => {
            // 保留 group 模式，便于后续 package/installer 或多用户机器按组授权。
            unit.push_str("SocketMode=0660\nSocketUser=root\nSocketGroup=");
            unit.push_str(group);
            unit.push('\n');
        }
        (None, None) => {
            unit.push_str("SocketMode=0600\nSocketUser=root\n");
        }
    }
    unit.push_str("RemoveOnStop=true\n\n[Install]\nWantedBy=sockets.target\n");
    unit
}

fn inherited_listener() -> Result<Option<UnixListener>, EngineError> {
    // systemd 约定：
    // - LISTEN_PID 指向当前 service 进程；
    // - LISTEN_FDS 表示从 fd=3 开始传了多少个监听 fd。
    let listen_pid = env::var("LISTEN_PID")
        .ok()
        .and_then(|value| value.parse::<u32>().ok());
    let listen_fds = env::var("LISTEN_FDS")
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(0);

    if listen_pid != Some(std::process::id()) || listen_fds <= 0 {
        return Ok(None);
    }

    let fd = 3;
    let rc = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if rc < 0 {
        return Err(remote_error(format!(
            "systemd passed invalid listener fd: {}",
            io::Error::last_os_error()
        )));
    }

    let listener = unsafe { UnixListener::from_raw_fd(fd) };
    Ok(Some(listener))
}

fn systemd_unit_is_active(unit: &str) -> bool {
    systemctl_success(["is-active", "--quiet", unit])
}

fn systemctl_success<const N: usize>(args: [&str; N]) -> bool {
    Command::new("systemctl")
        .args(args)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn run_command<const N: usize>(program: &str, args: [&str; N]) -> Result<(), EngineError> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|err| remote_error(format!("failed to run {program}: {err}")))?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        format!("exit status {}", output.status)
    };
    Err(remote_error(format!("{program} failed: {detail}")))
}

fn ensure_root() -> Result<(), EngineError> {
    if is_running_as_root() {
        Ok(())
    } else {
        Err(remote_error(
            "service management commands must run as root".to_string(),
        ))
    }
}

fn is_running_as_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

fn current_username(uid: u32) -> Option<String> {
    // install/repair 从普通用户经 pkexec 进入 root 后，仍然需要恢复“原始用户是谁”，
    // 以便生成归属到该用户的 socket unit。
    let mut pwd = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut result = std::ptr::null_mut();
    let mut buf = vec![0u8; 1024];

    loop {
        let rc = unsafe {
            libc::getpwuid_r(
                uid,
                pwd.as_mut_ptr(),
                buf.as_mut_ptr().cast(),
                buf.len(),
                &mut result,
            )
        };
        if rc == 0 {
            if result.is_null() {
                return None;
            }
            let pwd = unsafe { pwd.assume_init() };
            let name = unsafe { std::ffi::CStr::from_ptr(pwd.pw_name) };
            return Some(name.to_string_lossy().into_owned());
        }
        if rc == libc::ERANGE {
            buf.resize(buf.len() * 2, 0);
            continue;
        }
        return None;
    }
}

fn is_missing_backend_error(err: &io::Error) -> bool {
    matches!(
        err.raw_os_error(),
        Some(libc::ENOENT | libc::ECONNREFUSED | libc::ECONNRESET)
    )
}

fn is_access_denied_error(err: &io::Error) -> bool {
    matches!(err.raw_os_error(), Some(libc::EACCES | libc::EPERM))
}

fn connect_error(socket_path: &Path, err: io::Error) -> EngineError {
    if is_access_denied_error(&err) {
        return EngineError::AccessDenied;
    }
    if is_missing_backend_error(&err) {
        return remote_error(format!(
            "Linux privileged backend is not installed or not running ({})",
            socket_path.display()
        ));
    }
    remote_error(format!(
        "failed to reach Linux privileged backend {}: {err}",
        socket_path.display()
    ))
}

fn remote_error(message: String) -> EngineError {
    EngineError::Remote(message)
}

#[allow(dead_code)]
fn _version_check(reply: BackendReply) -> Result<(), EngineError> {
    match reply {
        BackendReply::Info { protocol_version } if protocol_version == IPC_PROTOCOL_VERSION => {
            Ok(())
        }
        BackendReply::Info { protocol_version } => {
            Err(protocol_mismatch(IPC_PROTOCOL_VERSION, protocol_version))
        }
        other => Err(unexpected_reply(other)),
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{
        parse_linux_entry_command, render_service_unit, render_socket_unit, InstallOptions,
        LinuxEntryCommand, ManageCommand, RemoveOptions, ServiceOptions, DEFAULT_INSTALLED_BINARY,
        DEFAULT_SOCKET_GROUP, DEFAULT_SOCKET_UNIT_PATH, DEFAULT_UNIT_PATH,
    };

    fn parse(args: &[&str]) -> LinuxEntryCommand {
        parse_linux_entry_command(args.iter().map(std::ffi::OsString::from))
            .expect("args should parse")
            .expect("linux entry command should be detected")
    }

    #[test]
    fn parse_service_mode_accepts_defaults() {
        let LinuxEntryCommand::ServiceMode(ServiceOptions {
            socket_path,
            socket_group,
            allowed_uid,
        }) = parse(&["r-wg", "--linux-service"])
        else {
            panic!("expected service mode");
        };
        assert_eq!(socket_path, PathBuf::from("/run/r-wg/control.sock"));
        assert_eq!(socket_group.as_deref(), Some(DEFAULT_SOCKET_GROUP));
        assert_eq!(allowed_uid, None);
    }

    #[test]
    fn parse_service_mode_accepts_overrides() {
        let LinuxEntryCommand::ServiceMode(ServiceOptions {
            socket_path,
            socket_group,
            allowed_uid,
        }) = parse(&[
            "r-wg",
            "--linux-service",
            "--socket",
            "/tmp/r-wg.sock",
            "--socket-group",
            "vpnusers",
            "--allowed-uid",
            "1000",
        ])
        else {
            panic!("expected service mode");
        };
        assert_eq!(socket_path, PathBuf::from("/tmp/r-wg.sock"));
        assert_eq!(socket_group.as_deref(), Some("vpnusers"));
        assert_eq!(allowed_uid, Some(1000));
    }

    #[test]
    fn parse_install_command_uses_defaults() {
        let LinuxEntryCommand::Manage(ManageCommand::Install(InstallOptions {
            source_path,
            binary_path,
            unit_path,
            socket_unit_path,
            socket_group,
            socket_user,
            allowed_uid,
        })) = parse(&["r-wg", "service", "install", "--source", "/tmp/r-wg"])
        else {
            panic!("expected install command");
        };
        assert_eq!(source_path, PathBuf::from("/tmp/r-wg"));
        assert_eq!(binary_path, PathBuf::from(DEFAULT_INSTALLED_BINARY));
        assert_eq!(unit_path, PathBuf::from(DEFAULT_UNIT_PATH));
        assert_eq!(socket_unit_path, PathBuf::from(DEFAULT_SOCKET_UNIT_PATH));
        assert_eq!(socket_group.as_deref(), Some(DEFAULT_SOCKET_GROUP));
        assert_eq!(socket_user, None);
        assert_eq!(allowed_uid, None);
    }

    #[test]
    fn parse_remove_command_uses_defaults() {
        let LinuxEntryCommand::Manage(ManageCommand::Remove(RemoveOptions {
            binary_path,
            unit_path,
            socket_unit_path,
        })) = parse(&["r-wg", "service", "remove"])
        else {
            panic!("expected remove command");
        };
        assert_eq!(binary_path, PathBuf::from(DEFAULT_INSTALLED_BINARY));
        assert_eq!(unit_path, PathBuf::from(DEFAULT_UNIT_PATH));
        assert_eq!(socket_unit_path, PathBuf::from(DEFAULT_SOCKET_UNIT_PATH));
    }

    #[test]
    fn render_service_unit_uses_binary_and_group() {
        let unit = render_service_unit(Path::new("/opt/r-wg/r-wg"), Some("vpnusers"), Some(1000));
        assert!(unit.contains(
            "ExecStart=/opt/r-wg/r-wg --linux-service --socket-group vpnusers --allowed-uid 1000"
        ));
        assert!(unit.contains("WantedBy=multi-user.target"));
    }

    #[test]
    fn render_socket_unit_uses_group_and_socket_target() {
        let unit = render_socket_unit(None, Some("vpnusers"));
        assert!(unit.contains("SocketGroup=vpnusers"));
        assert!(unit.contains("WantedBy=sockets.target"));
    }

    #[test]
    fn render_socket_unit_uses_socket_user_when_present() {
        let unit = render_socket_unit(Some("luren"), None);
        assert!(unit.contains("SocketUser=luren"));
        assert!(unit.contains("SocketMode=0600"));
    }
}
