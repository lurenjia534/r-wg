//! WireGuard 后端引擎：
//! - 在独立线程中创建 tokio runtime，避免占用 UI 线程或依赖外部运行时。
//! - 通过 MPSC 命令通道驱动 gotatun 设备的生命周期，确保串行执行。
//! - 对外提供同步 API（start/stop/status），内部异步执行并做清理回滚。
use std::any::Any;
use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::Duration;

use futures_util::FutureExt;
use gotatun::device::{self, DefaultDeviceTransports, Device};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};

use crate::core::config::{self, ConfigError};
use crate::core::dns::DnsSelection;
use crate::core::route_plan::{
    normalize_config_for_runtime, RouteApplyReport, RoutePlan, RoutePlanPlatform,
    DEFAULT_FULL_TUNNEL_FWMARK,
};
use crate::log::events::engine as log_engine;
use crate::platform;

use super::ephemeral::{self, DaitaMode, EphemeralFailureKind, QuantumMode};
#[cfg(target_os = "linux")]
use super::linux_kernel::{self, KernelWireGuardDevice, KernelWireGuardError};
use super::relay_inventory;

#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn trim_allocator() {
    // 尝试让 glibc 将空闲堆页归还给 OS，尽量降低 RSS（不保证一定生效）。
    unsafe {
        libc::malloc_trim(0);
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn trim_allocator() {
    // 非 glibc 平台不执行 trim，避免引入不兼容行为。
}
/// 启动请求：包含 TUN 设备名称与配置文本。
///
/// 之所以传入完整配置文本，是为了让引擎在后台线程内完成解析与应用，
/// 避免 UI 线程阻塞或出现跨线程的生命周期管理问题。
/// 在 Windows 按需提权模式下，这个结构还会通过 IPC 从普通权限 UI 发送给管理员 helper，
/// 因此字段语义需要稳定且可序列化。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartRequest {
    /// TUN 设备名称，例如 "wg0"。
    pub tun_name: String,
    /// WireGuard 配置文本（含 wg-quick 兼容字段）。
    pub config_text: String,
    /// DNS 选择（全局 UI 状态传入）。
    pub dns: DnsSelection,
    /// 量子抗性隧道升级模式。
    pub quantum_mode: QuantumMode,
    /// DAITA 模式。
    pub daita_mode: DaitaMode,
    /// 是否启用 Kill switch。
    pub kill_switch_enabled: bool,
    /// WireGuard 数据面实现偏好。
    #[serde(default)]
    pub wireguard_backend_preference: WireGuardBackendPreference,
}

impl StartRequest {
    /// 便捷构造器。
    pub fn new(
        tun_name: impl Into<String>,
        config_text: impl Into<String>,
        dns: DnsSelection,
        quantum_mode: QuantumMode,
        daita_mode: DaitaMode,
        kill_switch_enabled: bool,
        wireguard_backend_preference: WireGuardBackendPreference,
    ) -> Self {
        Self {
            tun_name: tun_name.into(),
            config_text: config_text.into(),
            dns,
            quantum_mode,
            daita_mode,
            kill_switch_enabled,
            wireguard_backend_preference,
        }
    }
}

/// WireGuard 数据面实现偏好。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireGuardBackendPreference {
    /// 自动选择；Linux 后续优先 kernel，不可用时回退 GotaTun。
    Auto,
    /// 明确要求 Linux kernel WireGuard。
    Kernel,
    /// 明确要求用户态 GotaTun。
    Userspace,
}

impl Default for WireGuardBackendPreference {
    fn default() -> Self {
        #[cfg(target_os = "linux")]
        {
            Self::Auto
        }
        #[cfg(not(target_os = "linux"))]
        {
            Self::Userspace
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BackendDecision {
    UserspaceGotaTun,
    LinuxKernel,
}

#[cfg(target_os = "linux")]
enum KernelStartError {
    Kernel(KernelWireGuardError),
    Engine(EngineError),
}

#[cfg(target_os = "linux")]
impl KernelStartError {
    fn into_engine_error(self) -> EngineError {
        match self {
            Self::Kernel(error) => EngineError::KernelWireGuard(error.to_string()),
            Self::Engine(error) => error,
        }
    }
}

/// 引擎状态：是否处于“已启动并生效”状态。
///
/// Running 代表网络配置已应用且设备可用；Stopped 则表示未启动或已停止（设备可能被缓存复用）。
/// Windows helper 模式下，这个状态会回传给普通权限 UI，用来恢复开关状态与按钮文案。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EngineStatus {
    /// 未启动。
    Stopped,
    /// 已启动。
    Running,
}

/// 当前运行中的 WireGuard 数据面实现。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActiveBackendStatus {
    /// Linux kernel WireGuard interface.
    LinuxKernel,
    /// GotaTun userspace WireGuard device.
    UserspaceGotaTun,
}

impl ActiveBackendStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::LinuxKernel => "Linux Kernel",
            Self::UserspaceGotaTun => "GotaTun",
        }
    }
}

/// 引擎运行时快照。
///
/// 用于 UI/IPC 恢复展示，包含基础运行态、最近一次路由应用结果，以及 ephemeral 协商状态。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineRuntimeSnapshot {
    pub status: EngineStatus,
    #[serde(default)]
    pub active_backend: Option<ActiveBackendStatus>,
    pub apply_report: Option<RouteApplyReport>,
    pub quantum_protected: bool,
    pub last_quantum_failure: Option<EphemeralFailureKind>,
    pub daita_active: bool,
    pub last_daita_failure: Option<EphemeralFailureKind>,
}

/// DAITA 资源缓存状态快照。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayInventoryStatusSnapshot {
    pub cache_path: String,
    pub present: bool,
    pub relay_count: usize,
    pub daita_relay_count: usize,
    pub fetched_at_unix_secs: Option<u64>,
}

/// gotatun 设备的运行时统计信息。
/// 这些数据会在 Windows helper 模式下通过 IPC 返回给 UI，驱动统计面板刷新。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineStats {
    pub peers: Vec<PeerStats>,
}

/// 单个 Peer 的 DAITA 统计快照。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaitaStats {
    pub tx_padding_bytes: u64,
    pub rx_padding_bytes: u64,
    pub tx_decoy_packet_bytes: u64,
    pub rx_decoy_packet_bytes: u64,
}

/// 单个 Peer 的状态快照。
/// 字段保持简单扁平，便于直接序列化后跨进程回传。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerStats {
    pub public_key: [u8; 32],
    pub endpoint: Option<SocketAddr>,
    /// 上次握手距现在的时间。backend 必须在填充前把原生时间戳转换成 age。
    pub last_handshake: Option<Duration>,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub daita: Option<DaitaStats>,
}

/// 引擎错误类型。
/// 仅涵盖启动/停止/状态查询相关的错误面。
#[derive(Debug)]
pub enum EngineError {
    /// 后台命令通道已关闭（通常是后台线程退出或崩溃）。
    ChannelClosed,
    /// 重复启动。
    AlreadyRunning,
    /// 未启动却请求停止或其它操作。
    NotRunning,
    /// 调用方无权访问特权后端。
    AccessDenied,
    /// gotatun 设备层错误（如 TUN 创建失败）。
    Device(device::Error),
    /// WireGuard 配置错误（解析或字段非法）。
    Config(ConfigError),
    /// 系统网络配置错误（地址/路由/DNS 应用失败）。
    Network(platform::NetworkError),
    /// Linux kernel WireGuard 控制面错误。
    KernelWireGuard(String),
    /// 请求的 WireGuard backend 当前不可用或与所选功能冲突。
    UnsupportedBackend(String),
    /// Ephemeral peer 协商或重配置失败。
    Ephemeral(String),
    /// UI 与特权后端协议版本不一致。
    VersionMismatch { expected: u32, actual: u32 },
    /// Windows 提权 helper / IPC 层返回的文本错误。
    Remote(String),
}

/// 将错误转换为可读文本，便于上层日志与提示。
impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::ChannelClosed => write!(f, "backend channel closed"),
            EngineError::AlreadyRunning => write!(f, "backend already running"),
            EngineError::NotRunning => write!(f, "backend not running"),
            EngineError::AccessDenied => write!(f, "access denied to privileged backend"),
            EngineError::Device(err) => write!(f, "device error: {err}"),
            EngineError::Config(err) => write!(f, "config error: {err}"),
            EngineError::Network(err) => write!(f, "network error: {err}"),
            EngineError::KernelWireGuard(message) => {
                write!(f, "kernel WireGuard error: {message}")
            }
            EngineError::UnsupportedBackend(message) => {
                write!(f, "unsupported WireGuard backend: {message}")
            }
            EngineError::Ephemeral(message) => write!(f, "ephemeral negotiation error: {message}"),
            EngineError::VersionMismatch { expected, actual } => write!(
                f,
                "privileged backend protocol mismatch (expected v{expected}, got v{actual})"
            ),
            EngineError::Remote(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for EngineError {}

/// 设备层错误 -> 引擎错误。
impl From<device::Error> for EngineError {
    fn from(err: device::Error) -> Self {
        EngineError::Device(err)
    }
}

impl From<ConfigError> for EngineError {
    fn from(err: ConfigError) -> Self {
        EngineError::Config(err)
    }
}

impl From<platform::NetworkError> for EngineError {
    fn from(err: platform::NetworkError) -> Self {
        EngineError::Network(err)
    }
}

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

/// 后台线程接收的命令类型。
enum Command {
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

    /// 同步启动接口：
    /// - 通过 blocking_send 把命令送入后台。
    /// - 用 oneshot 等待后台完成并回传结果。
    ///
    /// 上层无需持有 runtime，也不需要 async/await。
    pub fn start(&self, request: StartRequest) -> Result<(), EngineError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.inner
            .tx
            .blocking_send(Command::Start(request, reply_tx))
            .map_err(|_| EngineError::ChannelClosed)?;
        reply_rx
            .blocking_recv()
            .map_err(|_| EngineError::ChannelClosed)?
    }

    /// 同步停止接口。
    ///
    /// 停止会触发网络配置回滚，并释放 gotatun 设备句柄。
    pub fn stop(&self) -> Result<(), EngineError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.inner
            .tx
            .blocking_send(Command::Stop(reply_tx))
            .map_err(|_| EngineError::ChannelClosed)?;
        reply_rx
            .blocking_recv()
            .map_err(|_| EngineError::ChannelClosed)?
    }

    /// 同步状态查询接口。
    ///
    /// 返回的是“已启动/未启动”，不包含更细粒度的统计信息。
    pub fn status(&self) -> Result<EngineStatus, EngineError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.inner
            .tx
            .blocking_send(Command::Status(reply_tx))
            .map_err(|_| EngineError::ChannelClosed)?;
        reply_rx
            .blocking_recv()
            .map_err(|_| EngineError::ChannelClosed)
    }

    /// 同步获取 gotatun 运行时统计信息。
    pub fn stats(&self) -> Result<EngineStats, EngineError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.inner
            .tx
            .blocking_send(Command::Stats(reply_tx))
            .map_err(|_| EngineError::ChannelClosed)?;
        reply_rx
            .blocking_recv()
            .map_err(|_| EngineError::ChannelClosed)?
    }

    pub fn apply_report(&self) -> Result<Option<RouteApplyReport>, EngineError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.inner
            .tx
            .blocking_send(Command::ApplyReport(reply_tx))
            .map_err(|_| EngineError::ChannelClosed)?;
        reply_rx
            .blocking_recv()
            .map_err(|_| EngineError::ChannelClosed)
    }

    pub fn runtime_snapshot(&self) -> Result<EngineRuntimeSnapshot, EngineError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.inner
            .tx
            .blocking_send(Command::RuntimeSnapshot(reply_tx))
            .map_err(|_| EngineError::ChannelClosed)?;
        reply_rx
            .blocking_recv()
            .map_err(|_| EngineError::ChannelClosed)
    }

    pub fn relay_inventory_status(&self) -> Result<RelayInventoryStatusSnapshot, EngineError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.inner
            .tx
            .blocking_send(Command::RelayInventoryStatus(reply_tx))
            .map_err(|_| EngineError::ChannelClosed)?;
        reply_rx
            .blocking_recv()
            .map_err(|_| EngineError::ChannelClosed)?
    }

    pub fn refresh_relay_inventory(&self) -> Result<RelayInventoryStatusSnapshot, EngineError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.inner
            .tx
            .blocking_send(Command::RefreshRelayInventory(reply_tx))
            .map_err(|_| EngineError::ChannelClosed)?;
        reply_rx
            .blocking_recv()
            .map_err(|_| EngineError::ChannelClosed)?
    }
}

/// 后台线程维护的运行态状态。
///
/// 该状态只在后台线程内访问，保证串行一致性。
#[derive(Default)]
struct EngineState {
    /// 暂停复用的 GotaTun 设备；只在未运行时缓存。
    cached_userspace_device: Option<DeviceSlot>,
    /// 当前运行中的 WireGuard 数据面实现。
    active_backend: Option<ActiveWireGuardBackend>,
    /// 系统网络配置状态，用于停止时回滚。
    net_state: Option<platform::NetworkState>,
    /// 最近一次成功应用的结构化报告。
    route_apply_report: Option<RouteApplyReport>,
    /// 是否处于“已启动并生效”的状态。
    running: bool,
    /// 当前运行态是否使用了量子升级后的临时本地密钥。
    quantum_active: bool,
    /// 当前运行态是否启用了 DAITA。
    daita_active: bool,
    /// 当前运行态是否使用了 ephemeral 临时私钥。
    ephemeral_key_active: bool,
    /// 最近一次量子升级失败分类。
    last_quantum_failure: Option<EphemeralFailureKind>,
    /// 最近一次 DAITA 协商失败分类。
    last_daita_failure: Option<EphemeralFailureKind>,
}

/// 缓存的 gotatun 设备与其 TUN 名称。
struct DeviceSlot {
    device: Device<DefaultDeviceTransports>,
    tun_name: String,
}

/// 当前运行中的后端，确保运行态只由一个具体数据面持有。
enum ActiveWireGuardBackend {
    Userspace(DeviceSlot),
    #[cfg(target_os = "linux")]
    LinuxKernel(KernelWireGuardDevice),
}

impl ActiveWireGuardBackend {
    fn status(&self) -> ActiveBackendStatus {
        match self {
            Self::Userspace(_) => ActiveBackendStatus::UserspaceGotaTun,
            #[cfg(target_os = "linux")]
            Self::LinuxKernel(_) => ActiveBackendStatus::LinuxKernel,
        }
    }
}

impl EngineState {
    /// 启动设备：
    /// - 解析配置并转换为 gotatun Set 请求。
    /// - 创建 TUN 并绑定 UDP socket。
    /// - 写入私钥/端口/peer 配置，若失败则停止设备。
    /// - 应用系统网络配置，失败则停止设备并返回错误。
    async fn start(&mut self, request: StartRequest) -> Result<(), EngineError> {
        if self.running {
            return Err(EngineError::AlreadyRunning);
        }
        self.route_apply_report = None;
        if self.active_backend.is_some() {
            self.shutdown_active_backend().await;
        }
        self.quantum_active = false;
        self.daita_active = false;
        self.ephemeral_key_active = false;
        self.last_quantum_failure = None;
        self.last_daita_failure = None;

        log_engine::start(&request.tun_name, request.config_text.len());
        log_engine::wireguard_backend_preference(request.wireguard_backend_preference.as_str());
        let backend_decision = resolve_backend(
            request.wireguard_backend_preference,
            request.daita_mode,
            request.quantum_mode,
        )?;
        log_engine::wireguard_backend_resolved(backend_decision.as_str());

        // 解析配置并映射为 gotatun 的 DeviceSettings。
        let parsed = config::parse_config(&request.config_text)?;
        let inserted_fwmark = wants_full_tunnel(&parsed.peers) && parsed.interface.fwmark.is_none();
        let parsed = normalize_config_for_runtime(parsed, request.dns);
        if inserted_fwmark {
            log_engine::auto_fwmark(DEFAULT_FULL_TUNNEL_FWMARK);
        }
        if request.daita_mode.is_enabled() {
            match ephemeral::validate_daita_config(&parsed) {
                Ok(()) => {}
                Err(error) => {
                    self.last_daita_failure = Some(error.kind());
                    return Err(EngineError::Ephemeral(error.to_string()));
                }
            }
        }
        let route_plan = RoutePlan::build(RoutePlanPlatform::current(), &parsed);
        let settings = parsed.to_device_settings().await?;
        log_engine::config_parsed();

        if matches!(backend_decision, BackendDecision::LinuxKernel) {
            #[cfg(target_os = "linux")]
            match self
                .start_linux_kernel(&request.tun_name, &parsed, &route_plan, &settings, &request)
                .await
            {
                Ok(()) => return Ok(()),
                Err(KernelStartError::Kernel(error))
                    if request.wireguard_backend_preference == WireGuardBackendPreference::Auto
                        && error.is_unavailable() =>
                {
                    log_engine::wireguard_backend_fallback(&error.to_string());
                }
                Err(error) => return Err(error.into_engine_error()),
            }

            #[cfg(not(target_os = "linux"))]
            return Err(EngineError::UnsupportedBackend(
                "kernel WireGuard backend is only supported on Linux".to_string(),
            ));
        }

        self.start_userspace_gotatun(&request.tun_name, &parsed, &route_plan, &settings, &request)
            .await
    }

    async fn start_userspace_gotatun(
        &mut self,
        tun_name: &str,
        parsed: &config::WireGuardConfig,
        route_plan: &RoutePlan,
        settings: &config::DeviceSettings,
        request: &StartRequest,
    ) -> Result<(), EngineError> {
        if self.active_backend.is_some() {
            self.shutdown_active_backend().await;
        }
        if let Some(slot) = &self.cached_userspace_device {
            if slot.tun_name != tun_name {
                self.shutdown_cached_userspace_device().await;
            }
        }

        let mut created_new = false;
        let slot = match self.cached_userspace_device.take() {
            Some(slot) => slot,
            None => {
                let handle = device::build()
                    .with_default_udp()
                    .create_tun(tun_name)?
                    .build()
                    .await?;
                created_new = true;
                log_engine::device_created();
                DeviceSlot {
                    device: handle,
                    tun_name: tun_name.to_string(),
                }
            }
        };

        let slot = match self.configure_userspace_device(slot, settings).await {
            Ok(slot) => slot,
            Err((slot, err)) => {
                self.recover_userspace_slot_after_start_failure(slot, created_new)
                    .await;
                return Err(EngineError::Device(err));
            }
        };
        log_engine::device_configured();

        let net_result = match platform::apply_network_config(
            tun_name,
            parsed,
            route_plan,
            request.kill_switch_enabled,
        )
        .await
        {
            Ok(result) => result,
            Err(err) => {
                self.route_apply_report = Some(err.report);
                self.recover_userspace_slot_after_start_failure(slot, created_new)
                    .await;
                return Err(EngineError::Network(err.error));
            }
        };
        self.net_state = Some(net_result.state);
        log_engine::network_configured();
        self.route_apply_report = Some(net_result.report.clone());

        if request.quantum_mode.is_enabled() || request.daita_mode.is_enabled() {
            if let Err(error) = self
                .upgrade_userspace_ephemeral(tun_name, parsed, settings, &slot, request)
                .await
            {
                let cleanup_result = self.cleanup_active_network_state().await;
                self.recover_userspace_slot_after_start_failure(slot, created_new)
                    .await;
                return match cleanup_result {
                    Ok(()) => Err(error),
                    Err(err) => Err(err),
                };
            }
        }

        self.active_backend = Some(ActiveWireGuardBackend::Userspace(slot));
        self.running = true;
        Ok(())
    }

    async fn configure_userspace_device(
        &self,
        slot: DeviceSlot,
        settings: &config::DeviceSettings,
    ) -> Result<DeviceSlot, (DeviceSlot, device::Error)> {
        let result = slot
            .device
            .write(async |device| {
                device.set_private_key(settings.private_key.clone()).await;
                if let Some(port) = settings.listen_port {
                    device.set_listen_port(port);
                }
                #[cfg(target_os = "linux")]
                if let Some(fwmark) = settings.fwmark {
                    device.set_fwmark(fwmark)?;
                }
                device.clear_peers();
                device.add_peers(settings.peers.clone());
                Ok::<_, device::Error>(())
            })
            .await
            .and_then(|result| result);

        match result {
            Ok(()) => Ok(slot),
            Err(error) => Err((slot, error)),
        }
    }

    async fn upgrade_userspace_ephemeral(
        &mut self,
        tun_name: &str,
        parsed: &config::WireGuardConfig,
        settings: &config::DeviceSettings,
        slot: &DeviceSlot,
        request: &StartRequest,
    ) -> Result<(), EngineError> {
        log_engine::ephemeral_negotiation_requested(
            request.quantum_mode.is_enabled(),
            request.daita_mode.is_enabled(),
        );
        let Some(base_peer) = settings.peers.first() else {
            let error = ephemeral::Error::UnsupportedConfig(
                "ephemeral peer negotiation requires exactly one configured peer",
            );
            if request.quantum_mode.is_enabled() {
                self.last_quantum_failure = Some(error.kind());
            }
            if request.daita_mode.is_enabled() {
                self.last_daita_failure = Some(error.kind());
            }
            let message = error.to_string();
            log_engine::ephemeral_negotiation_failed(&message);
            return Err(EngineError::Ephemeral(message));
        };

        match ephemeral::upgrade_tunnel(
            request.quantum_mode,
            request.daita_mode,
            &slot.device,
            tun_name,
            parsed,
            base_peer,
        )
        .await
        {
            Ok(outcome) => {
                log_engine::ephemeral_negotiation_completed(
                    outcome.quantum_applied,
                    outcome.daita_applied,
                );
                self.quantum_active = outcome.quantum_applied;
                self.daita_active = outcome.daita_applied;
                self.ephemeral_key_active = outcome.quantum_applied || outcome.daita_applied;
                self.last_quantum_failure = None;
                self.last_daita_failure = None;
                Ok(())
            }
            Err(error) => {
                if request.quantum_mode.is_enabled() {
                    self.last_quantum_failure = Some(error.kind());
                }
                if request.daita_mode.is_enabled() {
                    self.last_daita_failure = Some(error.kind());
                }
                let message = error.to_string();
                log_engine::ephemeral_negotiation_failed(&message);
                Err(EngineError::Ephemeral(message))
            }
        }
    }

    async fn recover_userspace_slot_after_start_failure(
        &mut self,
        slot: DeviceSlot,
        created_new: bool,
    ) {
        if created_new {
            stop_userspace_slot(slot).await;
        } else {
            cache_cleared_userspace_slot(&mut self.cached_userspace_device, slot).await;
        }
    }

    #[cfg(target_os = "linux")]
    async fn start_linux_kernel(
        &mut self,
        tun_name: &str,
        parsed: &config::WireGuardConfig,
        route_plan: &RoutePlan,
        settings: &config::DeviceSettings,
        request: &StartRequest,
    ) -> Result<(), KernelStartError> {
        if self.active_backend.is_some() {
            self.shutdown_active_backend().await;
        }
        self.shutdown_cached_userspace_device().await;

        let kernel_device = linux_kernel::start_kernel_device(tun_name, settings)
            .await
            .map_err(KernelStartError::Kernel)?;
        log_engine::kernel_device_created(tun_name);

        let mut quantum_guard = if request.quantum_mode.is_enabled() {
            match platform::linux::apply_quantum_negotiation_traffic_guard(
                tun_name,
                route_plan_has_ipv6_tunnel_routes(route_plan),
            )
            .await
            {
                Ok(guard) => Some(guard),
                Err(error) => {
                    if linux_kernel::delete_kernel_device(kernel_device.tun_name())
                        .await
                        .is_ok()
                    {
                        let _ = linux_kernel::clear_kernel_backend_journal();
                    }
                    return Err(KernelStartError::Engine(EngineError::Network(error)));
                }
            }
        } else {
            None
        };

        let net_result = match platform::apply_network_config(
            tun_name,
            parsed,
            route_plan,
            request.kill_switch_enabled,
        )
        .await
        {
            Ok(result) => result,
            Err(err) => {
                if let Some(guard) = quantum_guard.take() {
                    let _ = platform::linux::cleanup_quantum_negotiation_traffic_guard(guard).await;
                }
                self.route_apply_report = Some(err.report);
                if linux_kernel::delete_kernel_device(kernel_device.tun_name())
                    .await
                    .is_ok()
                {
                    let _ = linux_kernel::clear_kernel_backend_journal();
                }
                return Err(KernelStartError::Engine(EngineError::Network(err.error)));
            }
        };

        self.net_state = Some(net_result.state);
        log_engine::network_configured();
        self.route_apply_report = Some(net_result.report.clone());
        if let Err(error) = linux_kernel::mark_kernel_device_running(tun_name) {
            if let Some(guard) = quantum_guard.take() {
                let _ = platform::linux::cleanup_quantum_negotiation_traffic_guard(guard).await;
            }
            let cleanup_result = self.cleanup_active_network_state().await;
            if linux_kernel::delete_kernel_device(kernel_device.tun_name())
                .await
                .is_ok()
            {
                let _ = linux_kernel::clear_kernel_backend_journal();
            }
            return match cleanup_result {
                Ok(()) => Err(KernelStartError::Kernel(error)),
                Err(err) => Err(KernelStartError::Engine(err)),
            };
        }

        if request.quantum_mode.is_enabled() {
            if let Err(error) = self
                .upgrade_kernel_ephemeral(tun_name, parsed, settings, request)
                .await
            {
                let guard_cleanup_result = if let Some(guard) = quantum_guard.take() {
                    platform::linux::cleanup_quantum_negotiation_traffic_guard(guard)
                        .await
                        .map_err(EngineError::from)
                } else {
                    Ok(())
                };
                let cleanup_result = self.cleanup_active_network_state().await;
                if linux_kernel::delete_kernel_device(kernel_device.tun_name())
                    .await
                    .is_ok()
                {
                    let _ = linux_kernel::clear_kernel_backend_journal();
                }
                return match (guard_cleanup_result, cleanup_result) {
                    (Err(err), _) => Err(KernelStartError::Engine(err)),
                    (Ok(()), Ok(())) => Err(error),
                    (Ok(()), Err(err)) => Err(KernelStartError::Engine(err)),
                };
            }

            if let Some(guard) = quantum_guard.take() {
                if let Err(error) =
                    platform::linux::cleanup_quantum_negotiation_traffic_guard(guard).await
                {
                    let cleanup_result = self.cleanup_active_network_state().await;
                    if linux_kernel::delete_kernel_device(kernel_device.tun_name())
                        .await
                        .is_ok()
                    {
                        let _ = linux_kernel::clear_kernel_backend_journal();
                    }
                    return match cleanup_result {
                        Ok(()) => Err(KernelStartError::Engine(EngineError::Network(error))),
                        Err(err) => Err(KernelStartError::Engine(err)),
                    };
                }
            }
        }

        self.active_backend = Some(ActiveWireGuardBackend::LinuxKernel(kernel_device));
        self.running = true;
        Ok(())
    }

    #[cfg(target_os = "linux")]
    async fn upgrade_kernel_ephemeral(
        &mut self,
        tun_name: &str,
        parsed: &config::WireGuardConfig,
        settings: &config::DeviceSettings,
        request: &StartRequest,
    ) -> Result<(), KernelStartError> {
        if request.daita_mode.is_enabled() {
            return Err(KernelStartError::Engine(EngineError::UnsupportedBackend(
                "DAITA requires userspace GotaTun".to_string(),
            )));
        }
        log_engine::ephemeral_negotiation_requested(
            request.quantum_mode.is_enabled(),
            request.daita_mode.is_enabled(),
        );
        let Some(base_peer) = settings.peers.first() else {
            let error = ephemeral::Error::UnsupportedConfig(
                "ephemeral peer negotiation requires exactly one configured peer",
            );
            self.last_quantum_failure = Some(error.kind());
            let message = error.to_string();
            log_engine::ephemeral_negotiation_failed(&message);
            return Err(KernelStartError::Engine(EngineError::Ephemeral(message)));
        };

        let update = match ephemeral::negotiate_tunnel_upgrade(
            request.quantum_mode,
            request.daita_mode,
            tun_name,
            parsed,
            base_peer,
        )
        .await
        {
            Ok(update) => update,
            Err(error) => {
                self.last_quantum_failure = Some(error.kind());
                let message = error.to_string();
                log_engine::ephemeral_negotiation_failed(&message);
                return Err(KernelStartError::Engine(EngineError::Ephemeral(message)));
            }
        };

        if let Err(error) = linux_kernel::apply_ephemeral_update(tun_name, &update).await {
            self.last_quantum_failure = Some(EphemeralFailureKind::Reconfigure);
            let message = error.to_string();
            log_engine::ephemeral_negotiation_failed(&message);
            return Err(KernelStartError::Engine(EngineError::Ephemeral(format!(
                "failed to apply kernel ephemeral update: {message}"
            ))));
        }

        let outcome = update.outcome();
        log_engine::ephemeral_negotiation_completed(outcome.quantum_applied, outcome.daita_applied);
        self.quantum_active = outcome.quantum_applied;
        self.daita_active = outcome.daita_applied;
        self.ephemeral_key_active = outcome.quantum_applied || outcome.daita_applied;
        self.last_quantum_failure = None;
        self.last_daita_failure = None;
        Ok(())
    }

    async fn cleanup_active_network_state(&mut self) -> Result<(), EngineError> {
        match self.net_state.take() {
            Some(state) => platform::cleanup_network_config(state)
                .await
                .map_err(EngineError::from),
            None => Ok(()),
        }
    }

    /// 停止设备：
    /// - 先回滚系统网络配置（若存在）。
    /// - 再按当前 active backend 清理数据面。
    async fn stop(&mut self) -> Result<(), EngineError> {
        if !self.running {
            return Err(EngineError::NotRunning);
        };

        log_engine::stop_requested();

        // 优先回滚系统网络配置，避免留下路由/DNS 污染。
        let cleanup_result = if let Some(state) = self.net_state.take() {
            platform::cleanup_network_config(state)
                .await
                .map_err(EngineError::from)
        } else {
            Ok(())
        };

        if let Err(error) = self.stop_active_backend().await {
            if cleanup_result.is_ok() {
                self.route_apply_report = None;
                self.running = false;
                self.quantum_active = false;
                self.daita_active = false;
                self.ephemeral_key_active = false;
                self.last_quantum_failure = None;
                self.last_daita_failure = None;
                trim_allocator();
            }
            return Err(error);
        }
        self.route_apply_report = None;
        self.running = false;
        self.quantum_active = false;
        self.daita_active = false;
        self.ephemeral_key_active = false;
        self.last_quantum_failure = None;
        self.last_daita_failure = None;

        trim_allocator();

        // 若回滚失败，仍然返回错误以便上层提示。
        cleanup_result
    }

    async fn stop_active_backend(&mut self) -> Result<(), EngineError> {
        let Some(active_backend) = self.active_backend.take() else {
            return Ok(());
        };

        match active_backend {
            ActiveWireGuardBackend::Userspace(slot) => {
                if self.ephemeral_key_active {
                    stop_userspace_slot(slot).await;
                } else {
                    clear_userspace_slot_peers(&slot).await?;
                    self.cached_userspace_device = Some(slot);
                }
            }
            #[cfg(target_os = "linux")]
            ActiveWireGuardBackend::LinuxKernel(kernel_device) => {
                if let Err(error) =
                    linux_kernel::delete_kernel_device(kernel_device.tun_name()).await
                {
                    self.active_backend = Some(ActiveWireGuardBackend::LinuxKernel(kernel_device));
                    return Err(EngineError::KernelWireGuard(format!(
                        "failed to delete kernel WireGuard interface: {error}"
                    )));
                }
                linux_kernel::clear_kernel_backend_journal()
                    .map_err(|error| EngineError::KernelWireGuard(error.to_string()))?;
            }
        }
        Ok(())
    }

    /// 彻底停止并释放当前 active backend 与缓存的 userspace device。
    async fn shutdown_active_backend(&mut self) {
        if let Some(active_backend) = self.active_backend.take() {
            match active_backend {
                ActiveWireGuardBackend::Userspace(slot) => stop_userspace_slot(slot).await,
                #[cfg(target_os = "linux")]
                ActiveWireGuardBackend::LinuxKernel(kernel_device) => {
                    if let Err(error) =
                        linux_kernel::delete_kernel_device(kernel_device.tun_name()).await
                    {
                        tracing::warn!("failed to delete kernel WireGuard interface: {error}");
                    } else if let Err(error) = linux_kernel::clear_kernel_backend_journal() {
                        tracing::warn!("failed to clear kernel WireGuard journal: {error}");
                    }
                }
            }
        }
        self.running = false;
        self.quantum_active = false;
        self.daita_active = false;
        self.ephemeral_key_active = false;
        self.last_quantum_failure = None;
        self.last_daita_failure = None;
        self.net_state = None;
        self.route_apply_report = None;
        trim_allocator();
    }

    async fn shutdown_cached_userspace_device(&mut self) {
        if let Some(slot) = self.cached_userspace_device.take() {
            stop_userspace_slot(slot).await;
        }
    }

    /// 查询状态。
    fn status(&self) -> EngineStatus {
        if self.running {
            EngineStatus::Running
        } else {
            EngineStatus::Stopped
        }
    }

    /// 获取 gotatun 运行时统计信息。
    async fn stats(&self) -> Result<EngineStats, EngineError> {
        if !self.running {
            return Err(EngineError::NotRunning);
        }
        let Some(active_backend) = self.active_backend.as_ref() else {
            return Err(EngineError::NotRunning);
        };
        match active_backend {
            ActiveWireGuardBackend::Userspace(slot) => read_userspace_stats(slot).await,
            #[cfg(target_os = "linux")]
            ActiveWireGuardBackend::LinuxKernel(kernel_device) => {
                linux_kernel::read_kernel_stats(kernel_device.tun_name())
                    .await
                    .map_err(|error| EngineError::KernelWireGuard(error.to_string()))
            }
        }
    }

    fn apply_report(&self) -> Option<RouteApplyReport> {
        self.route_apply_report.clone()
    }

    fn runtime_snapshot(&self) -> EngineRuntimeSnapshot {
        EngineRuntimeSnapshot {
            status: self.status(),
            active_backend: self
                .active_backend
                .as_ref()
                .map(ActiveWireGuardBackend::status),
            apply_report: self.apply_report(),
            quantum_protected: self.running && self.quantum_active,
            last_quantum_failure: self.last_quantum_failure,
            daita_active: self.running && self.daita_active,
            last_daita_failure: self.last_daita_failure,
        }
    }

    fn relay_inventory_status(&self) -> Result<RelayInventoryStatusSnapshot, EngineError> {
        relay_inventory::status_snapshot()
            .map(map_relay_inventory_status_snapshot)
            .map_err(|error| EngineError::Remote(error.to_string()))
    }
}

fn wants_full_tunnel(peers: &[config::PeerConfig]) -> bool {
    peers.iter().any(|peer| {
        peer.allowed_ips
            .iter()
            .any(|allowed| allowed.addr.is_unspecified() && allowed.cidr == 0)
    })
}

#[cfg(target_os = "linux")]
fn route_plan_has_ipv6_tunnel_routes(route_plan: &RoutePlan) -> bool {
    route_plan
        .allowed_routes
        .iter()
        .any(|route| matches!(route.addr, IpAddr::V6(_)))
}

async fn read_userspace_stats(slot: &DeviceSlot) -> Result<EngineStats, EngineError> {
    let peers = slot
        .device
        .read(async |device| device.peers().await)
        .await
        .into_iter()
        .map(|peer| PeerStats {
            public_key: peer.peer.public_key.to_bytes(),
            endpoint: peer.peer.endpoint,
            last_handshake: peer.stats.last_handshake,
            rx_bytes: peer.stats.rx_bytes as u64,
            tx_bytes: peer.stats.tx_bytes as u64,
            daita: peer.stats.daita.map(|stats| DaitaStats {
                tx_padding_bytes: stats.tx_padding_bytes as u64,
                rx_padding_bytes: stats.rx_padding_bytes as u64,
                tx_decoy_packet_bytes: stats.tx_decoy_packet_bytes as u64,
                rx_decoy_packet_bytes: stats.rx_decoy_packet_bytes as u64,
            }),
        })
        .collect();

    Ok(EngineStats { peers })
}

async fn clear_userspace_slot_peers(slot: &DeviceSlot) -> Result<(), device::Error> {
    let _ = slot.device.clear_peers().await?;
    Ok(())
}

async fn cache_cleared_userspace_slot(cache: &mut Option<DeviceSlot>, slot: DeviceSlot) {
    if let Err(error) = clear_userspace_slot_peers(&slot).await {
        tracing::warn!("failed to clear cached GotaTun peers: {error}");
        stop_userspace_slot(slot).await;
        return;
    }
    *cache = Some(slot);
}

async fn stop_userspace_slot(slot: DeviceSlot) {
    slot.device.stop().await;
    log_engine::device_stopped();
}

impl WireGuardBackendPreference {
    fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Kernel => "Kernel",
            Self::Userspace => "UserspaceGotaTun",
        }
    }
}

impl BackendDecision {
    fn as_str(self) -> &'static str {
        match self {
            Self::UserspaceGotaTun => "UserspaceGotaTun",
            Self::LinuxKernel => "LinuxKernel",
        }
    }
}

fn resolve_backend(
    preference: WireGuardBackendPreference,
    daita_mode: DaitaMode,
    _quantum_mode: QuantumMode,
) -> Result<BackendDecision, EngineError> {
    if daita_mode.is_enabled() {
        return match preference {
            WireGuardBackendPreference::Kernel => Err(EngineError::UnsupportedBackend(
                "DAITA currently requires GotaTun; switch WireGuard implementation to Userspace"
                    .to_string(),
            )),
            WireGuardBackendPreference::Auto | WireGuardBackendPreference::Userspace => {
                Ok(BackendDecision::UserspaceGotaTun)
            }
        };
    }

    match preference {
        WireGuardBackendPreference::Userspace => Ok(BackendDecision::UserspaceGotaTun),
        WireGuardBackendPreference::Kernel => linux_kernel_backend_decision(),
        WireGuardBackendPreference::Auto => Ok(default_auto_backend_decision()),
    }
}

fn linux_kernel_backend_decision() -> Result<BackendDecision, EngineError> {
    #[cfg(target_os = "linux")]
    {
        Ok(BackendDecision::LinuxKernel)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(EngineError::UnsupportedBackend(
            "kernel WireGuard backend is only supported on Linux".to_string(),
        ))
    }
}

fn default_auto_backend_decision() -> BackendDecision {
    #[cfg(target_os = "linux")]
    {
        BackendDecision::LinuxKernel
    }
    #[cfg(not(target_os = "linux"))]
    {
        BackendDecision::UserspaceGotaTun
    }
}

/// 后台线程的主事件循环：
/// 接收命令并顺序执行，通道关闭后安全收尾。
///
/// 该循环是引擎的“串行化核心”，避免并发修改内部状态。
async fn run(mut rx: mpsc::Receiver<Command>) {
    let mut state = EngineState {
        route_apply_report: platform::load_persisted_apply_report(),
        ..Default::default()
    };

    while let Some(command) = rx.recv().await {
        match command {
            Command::Start(request, reply) => {
                let result = catch_start_panic(&mut state, request).await;
                let _ = reply.send(result);
            }
            Command::Stop(reply) => {
                // 停止请求：返回 Ok/Err。
                let result = state.stop().await;
                let _ = reply.send(result);
            }
            Command::Status(reply) => {
                // 状态查询：返回当前状态。
                let _ = reply.send(state.status());
            }
            Command::Stats(reply) => {
                // 统计信息：返回运行态统计。
                let result = state.stats().await;
                let _ = reply.send(result);
            }
            Command::ApplyReport(reply) => {
                let _ = reply.send(state.apply_report());
            }
            Command::RuntimeSnapshot(reply) => {
                let _ = reply.send(state.runtime_snapshot());
            }
            Command::RelayInventoryStatus(reply) => {
                let _ = reply.send(state.relay_inventory_status());
            }
            Command::RefreshRelayInventory(reply) => {
                tokio::spawn(async move {
                    let result = relay_inventory::refresh_cache()
                        .await
                        .map(map_relay_inventory_status_snapshot)
                        .map_err(|error| EngineError::Remote(error.to_string()));
                    let _ = reply.send(result);
                });
            }
        }
    }

    // 通道关闭，尝试优雅停止设备。
    let _ = state.stop().await;
    state.shutdown_active_backend().await;
    state.shutdown_cached_userspace_device().await;
}

async fn catch_start_panic(
    state: &mut EngineState,
    request: StartRequest,
) -> Result<(), EngineError> {
    match AssertUnwindSafe(state.start(request)).catch_unwind().await {
        Ok(result) => result,
        Err(payload) => {
            let message = format!(
                "backend worker panicked while starting tunnel: {}",
                panic_payload_message(payload)
            );
            tracing::error!("{message}");
            recover_after_worker_panic(state).await;
            Err(EngineError::Remote(message))
        }
    }
}

async fn recover_after_worker_panic(state: &mut EngineState) {
    let apply_report = state.apply_report();
    if let Err(err) = state.cleanup_active_network_state().await {
        tracing::warn!("failed to clean up network state after backend panic: {err}");
    }
    state.shutdown_active_backend().await;
    state.shutdown_cached_userspace_device().await;
    state.route_apply_report = apply_report;
}

fn map_relay_inventory_status_snapshot(
    snapshot: relay_inventory::RelayInventoryStatusSnapshot,
) -> RelayInventoryStatusSnapshot {
    RelayInventoryStatusSnapshot {
        cache_path: snapshot.cache_path,
        present: snapshot.present,
        relay_count: snapshot.relay_count,
        daita_relay_count: snapshot.daita_relay_count,
        fetched_at_unix_secs: snapshot.fetched_at_unix_secs,
    }
}

fn panic_payload_message(payload: Box<dyn Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

#[cfg(test)]
mod tests {
    use crate::dns::{DnsMode, DnsPreset};

    use super::*;

    #[test]
    fn wireguard_backend_preference_defaults_by_platform() {
        #[cfg(target_os = "linux")]
        assert_eq!(
            WireGuardBackendPreference::default(),
            WireGuardBackendPreference::Auto
        );
        #[cfg(not(target_os = "linux"))]
        assert_eq!(
            WireGuardBackendPreference::default(),
            WireGuardBackendPreference::Userspace
        );
    }

    #[test]
    fn start_request_deserializes_missing_backend_preference() {
        let json = r#"{
            "tun_name": "wg0",
            "config_text": "[Interface]\n",
            "dns": {"mode": "FollowConfig", "preset": "CloudflareStandard"},
            "quantum_mode": "off",
            "daita_mode": "off",
            "kill_switch_enabled": true
        }"#;

        let request: StartRequest =
            serde_json::from_str(json).expect("legacy request should deserialize");

        assert_eq!(
            request.wireguard_backend_preference,
            WireGuardBackendPreference::default()
        );
    }

    #[test]
    fn backend_resolution_handles_daita_conflicts_and_quantum_kernel_support() {
        assert_eq!(
            resolve_backend(
                WireGuardBackendPreference::Auto,
                DaitaMode::On,
                QuantumMode::Off,
            )
            .expect("auto daita should choose userspace"),
            BackendDecision::UserspaceGotaTun
        );
        #[cfg(target_os = "linux")]
        {
            assert_eq!(
                resolve_backend(
                    WireGuardBackendPreference::Auto,
                    DaitaMode::Off,
                    QuantumMode::On,
                )
                .expect("auto quantum should prefer kernel on Linux"),
                BackendDecision::LinuxKernel
            );
            assert_eq!(
                resolve_backend(
                    WireGuardBackendPreference::Kernel,
                    DaitaMode::Off,
                    QuantumMode::On,
                )
                .expect("kernel quantum should be supported on Linux"),
                BackendDecision::LinuxKernel
            );
        }
        #[cfg(not(target_os = "linux"))]
        {
            assert_eq!(
                resolve_backend(
                    WireGuardBackendPreference::Auto,
                    DaitaMode::Off,
                    QuantumMode::On,
                )
                .expect("auto quantum should remain userspace off Linux"),
                BackendDecision::UserspaceGotaTun
            );
            assert!(matches!(
                resolve_backend(
                    WireGuardBackendPreference::Kernel,
                    DaitaMode::Off,
                    QuantumMode::On,
                ),
                Err(EngineError::UnsupportedBackend(message)) if message.contains("Linux")
            ));
        }
        assert_eq!(
            resolve_backend(
                WireGuardBackendPreference::Userspace,
                DaitaMode::Off,
                QuantumMode::On,
            )
            .expect("userspace quantum should remain userspace"),
            BackendDecision::UserspaceGotaTun
        );
        assert!(matches!(
            resolve_backend(
                WireGuardBackendPreference::Kernel,
                DaitaMode::On,
                QuantumMode::Off,
            ),
            Err(EngineError::UnsupportedBackend(message)) if message.contains("DAITA")
        ));
    }

    #[test]
    fn backend_resolution_uses_platform_default_without_capability_probe() {
        #[cfg(target_os = "linux")]
        let expected_auto = BackendDecision::LinuxKernel;
        #[cfg(not(target_os = "linux"))]
        let expected_auto = BackendDecision::UserspaceGotaTun;

        assert_eq!(
            resolve_backend(
                WireGuardBackendPreference::Auto,
                DaitaMode::Off,
                QuantumMode::Off,
            )
            .expect("auto should resolve from platform defaults"),
            expected_auto
        );
        #[cfg(target_os = "linux")]
        assert_eq!(
            resolve_backend(
                WireGuardBackendPreference::Kernel,
                DaitaMode::Off,
                QuantumMode::Off,
            )
            .expect("kernel should be selected on Linux"),
            BackendDecision::LinuxKernel
        );
        #[cfg(not(target_os = "linux"))]
        assert!(matches!(
            resolve_backend(
                WireGuardBackendPreference::Kernel,
                DaitaMode::Off,
                QuantumMode::Off,
            ),
            Err(EngineError::UnsupportedBackend(message)) if message.contains("Linux")
        ));
    }

    #[test]
    fn start_request_constructor_carries_backend_preference() {
        let request = StartRequest::new(
            "wg0",
            "[Interface]\n",
            DnsSelection::new(DnsMode::FollowConfig, DnsPreset::CloudflareStandard),
            QuantumMode::Off,
            DaitaMode::Off,
            true,
            WireGuardBackendPreference::Kernel,
        );

        assert_eq!(
            request.wireguard_backend_preference,
            WireGuardBackendPreference::Kernel
        );
    }
}
