//! WireGuard 后端引擎：
//! - 在独立线程中创建 tokio runtime，避免占用 UI 线程或依赖外部运行时。
//! - 通过 MPSC 命令通道驱动 gotatun 设备的生命周期，确保串行执行。
//! - 对外提供同步 API（start/stop/status），内部异步执行并做清理回滚。
use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

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

use super::quantum::{self, QuantumFailureKind, QuantumMode};

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
}

impl StartRequest {
    /// 便捷构造器。
    pub fn new(
        tun_name: impl Into<String>,
        config_text: impl Into<String>,
        dns: DnsSelection,
        quantum_mode: QuantumMode,
    ) -> Self {
        Self {
            tun_name: tun_name.into(),
            config_text: config_text.into(),
            dns,
            quantum_mode,
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

/// 引擎运行时快照。
///
/// 用于 UI/IPC 恢复展示，包含基础运行态、最近一次路由应用结果，以及量子升级状态。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineRuntimeSnapshot {
    pub status: EngineStatus,
    pub apply_report: Option<RouteApplyReport>,
    pub quantum_protected: bool,
    pub last_quantum_failure: Option<QuantumFailureKind>,
}

/// gotatun 设备的运行时统计信息。
/// 这些数据会在 Windows helper 模式下通过 IPC 返回给 UI，驱动统计面板刷新。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineStats {
    pub peers: Vec<PeerStats>,
}

/// 单个 Peer 的状态快照。
/// 字段保持简单扁平，便于直接序列化后跨进程回传。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerStats {
    pub public_key: [u8; 32],
    pub endpoint: Option<SocketAddr>,
    pub last_handshake: Option<Duration>,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
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
    /// 量子抗性升级流程失败。
    Quantum(String),
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
            EngineError::Quantum(message) => write!(f, "quantum upgrade error: {message}"),
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
}

/// 后台线程维护的运行态状态。
///
/// 该状态只在后台线程内访问，保证串行一致性。
#[derive(Default)]
struct EngineState {
    /// gotatun 设备句柄；可能处于“运行中”或“暂停复用”状态。
    device: Option<DeviceSlot>,
    /// 系统网络配置状态，用于停止时回滚。
    net_state: Option<platform::NetworkState>,
    /// 最近一次成功应用的结构化报告。
    route_apply_report: Option<RouteApplyReport>,
    /// 是否处于“已启动并生效”的状态。
    running: bool,
    /// 当前运行态是否使用了量子升级后的临时本地密钥。
    quantum_active: bool,
    /// 最近一次量子升级失败分类。
    last_quantum_failure: Option<QuantumFailureKind>,
}

/// 缓存的 gotatun 设备与其 TUN 名称。
struct DeviceSlot {
    device: Device<DefaultDeviceTransports>,
    tun_name: String,
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
        self.quantum_active = false;
        self.last_quantum_failure = None;

        log_engine::start(&request.tun_name, request.config_text.len());

        // 解析配置并映射为 gotatun 的 DeviceSettings。
        let parsed = config::parse_config(&request.config_text)?;
        let inserted_fwmark = wants_full_tunnel(&parsed.peers) && parsed.interface.fwmark.is_none();
        let parsed = normalize_config_for_runtime(parsed, request.dns);
        if inserted_fwmark {
            log_engine::auto_fwmark(DEFAULT_FULL_TUNNEL_FWMARK);
        }
        let route_plan = RoutePlan::build(RoutePlanPlatform::current(), &parsed);
        let settings = parsed.to_device_settings().await?;
        log_engine::config_parsed();

        // 如已有设备但 TUN 名称不同，则先彻底停止旧设备。
        if let Some(slot) = &self.device {
            if slot.tun_name != request.tun_name {
                self.shutdown_device().await;
            }
        }

        let mut created_new = false;
        if self.device.is_none() {
            // 使用 DeviceBuilder 创建 gotatun 设备。
            let handle = device::build()
                .with_default_udp()
                .create_tun(&request.tun_name)?
                .build()
                .await?;
            self.device = Some(DeviceSlot {
                device: handle,
                tun_name: request.tun_name.clone(),
            });
            created_new = true;
            log_engine::device_created();
        }

        let device = &self
            .device
            .as_ref()
            .expect("device must exist after creation/reuse")
            .device;

        // 配置 gotatun 设备；失败则立即停止设备。
        let config_result = device
            .write(async |device| {
                device.set_private_key(settings.private_key).await;
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
        if let Err(err) = config_result {
            if created_new {
                self.shutdown_device().await;
            } else {
                let _ = device.clear_peers().await;
            }
            return Err(EngineError::Device(err));
        }
        log_engine::device_configured();

        // 应用系统网络配置；失败时回滚 gotatun 设备。
        let net_result =
            match platform::apply_network_config(&request.tun_name, &parsed, &route_plan).await {
                Ok(result) => result,
                Err(err) => {
                    self.route_apply_report = Some(err.report);
                    if created_new {
                        self.shutdown_device().await;
                    } else {
                        let _ = device.clear_peers().await;
                    }
                    return Err(EngineError::Network(err.error));
                }
            };
        log_engine::network_configured();
        self.route_apply_report = Some(net_result.report.clone());

        if request.quantum_mode.is_enabled() {
            log_engine::quantum_upgrade_requested();
            let Some(base_peer) = settings.peers.first() else {
                let error = quantum::Error::UnsupportedConfig(
                    "quantum upgrade requires exactly one configured peer",
                );
                let message = error.to_string();
                self.last_quantum_failure = Some(error.kind());
                log_engine::quantum_upgrade_failed(&message);
                let cleanup_result = platform::cleanup_network_config(net_result.state)
                    .await
                    .map_err(EngineError::from);
                if created_new {
                    self.shutdown_device().await;
                } else {
                    let _ = device.clear_peers().await;
                }
                return match cleanup_result {
                    Ok(()) => Err(EngineError::Quantum(message)),
                    Err(err) => Err(err),
                };
            };
            if let Err(error) = quantum::upgrade_tunnel(
                request.quantum_mode,
                device,
                &request.tun_name,
                &parsed,
                base_peer,
            )
            .await
            {
                self.last_quantum_failure = Some(error.kind());
                let message = error.to_string();
                log_engine::quantum_upgrade_failed(&message);
                let cleanup_result = platform::cleanup_network_config(net_result.state)
                    .await
                    .map_err(EngineError::from);
                if created_new {
                    self.shutdown_device().await;
                } else {
                    let _ = device.clear_peers().await;
                }
                return match cleanup_result {
                    Ok(()) => Err(EngineError::Quantum(message)),
                    Err(err) => Err(err),
                };
            }
            log_engine::quantum_upgrade_completed();
            self.quantum_active = true;
            self.last_quantum_failure = None;
        }

        // 保存运行态状态，便于后续 stop/cleanup。
        self.net_state = Some(net_result.state);
        self.running = true;

        Ok(())
    }

    /// 停止设备：
    /// - 先回滚系统网络配置（若存在）。
    /// - 再清空 peers（保留 gotatun 设备以便复用）。
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

        // 量子升级后的本地临时私钥不复用，直接销毁整个 gotatun device。
        if self.quantum_active {
            if let Some(slot) = self.device.take() {
                slot.device.stop().await;
                log_engine::device_stopped();
            }
        } else if let Some(slot) = &self.device {
            // 普通隧道保留 gotatun 设备，只清空 peers。
            slot.device.clear_peers().await?;
        }
        self.route_apply_report = None;
        self.running = false;
        self.quantum_active = false;
        self.last_quantum_failure = None;

        trim_allocator();

        // 若回滚失败，仍然返回错误以便上层提示。
        cleanup_result
    }

    /// 彻底停止并释放 gotatun 设备（用于后台线程退出或强制清理）。
    async fn shutdown_device(&mut self) {
        if let Some(slot) = self.device.take() {
            slot.device.stop().await;
            log_engine::device_stopped();
        }
        self.running = false;
        self.quantum_active = false;
        self.last_quantum_failure = None;
        self.net_state = None;
        self.route_apply_report = None;
        trim_allocator();
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
        let Some(device) = self.device.as_ref() else {
            return Err(EngineError::NotRunning);
        };

        let peers = device
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
            })
            .collect();

        Ok(EngineStats { peers })
    }

    fn apply_report(&self) -> Option<RouteApplyReport> {
        self.route_apply_report.clone()
    }

    fn runtime_snapshot(&self) -> EngineRuntimeSnapshot {
        EngineRuntimeSnapshot {
            status: self.status(),
            apply_report: self.apply_report(),
            quantum_protected: self.running && self.quantum_active,
            last_quantum_failure: self.last_quantum_failure,
        }
    }
}

fn wants_full_tunnel(peers: &[config::PeerConfig]) -> bool {
    peers.iter().any(|peer| {
        peer.allowed_ips
            .iter()
            .any(|allowed| allowed.addr.is_unspecified() && allowed.cidr == 0)
    })
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
                // 启动请求：返回 Ok/Err。
                let result = state.start(request).await;
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
        }
    }

    // 通道关闭，尝试优雅停止设备。
    let _ = state.stop().await;
    state.shutdown_device().await;
}
