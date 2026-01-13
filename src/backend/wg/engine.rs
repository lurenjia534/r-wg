//! WireGuard 后端引擎：
//! - 在独立线程中创建 tokio runtime，避免占用 UI 线程或依赖外部运行时。
//! - 通过 MPSC 命令通道驱动 gotatun 设备的生命周期，确保串行执行。
//! - 对外提供同步 API（start/stop/status），内部异步执行并做清理回滚。
use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use gotatun::device::{
    self,
    api::{command::{Get, Response}, ApiServer},
    DeviceConfig, DeviceHandle, DefaultDeviceTransports,
};
use gotatun::udp::socket::UdpSocketFactory;
use tokio::sync::{mpsc, oneshot};

use super::config::{self, ConfigError};
use crate::log;
use crate::platform;
/// 启动请求：包含 TUN 设备名称与配置文本。
///
/// 之所以传入完整配置文本，是为了让引擎在后台线程内完成解析与应用，
/// 避免 UI 线程阻塞或出现跨线程的生命周期管理问题。
#[derive(Debug, Clone)]
pub struct StartRequest {
    /// TUN 设备名称，例如 "wg0"。
    pub tun_name: String,
    /// WireGuard 配置文本（含 wg-quick 兼容字段）。
    pub config_text: String,
}

impl StartRequest {
    /// 便捷构造器。
    pub fn new(tun_name: impl Into<String>, config_text: impl Into<String>) -> Self {
        Self {
            tun_name: tun_name.into(),
            config_text: config_text.into(),
        }
    }
}

/// 引擎状态：是否已经持有 gotatun DeviceHandle。
///
/// Running 代表已创建并持有设备句柄；Stopped 则表示未启动或已停止。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineStatus {
    /// 未启动。
    Stopped,
    /// 已启动。
    Running,
}

/// gotatun 设备的运行时统计信息。
#[derive(Debug, Clone)]
pub struct EngineStats {
    pub peers: Vec<PeerStats>,
}

/// 单个 Peer 的状态快照。
#[derive(Debug, Clone)]
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
    /// gotatun 设备层错误（如 TUN 创建失败）。
    Device(device::Error),
    /// WireGuard 配置错误（解析或字段非法）。
    Config(ConfigError),
    /// 系统网络配置错误（地址/路由/DNS 应用失败）。
    Network(platform::NetworkError),
    /// gotatun API 请求失败（请求未成功发送或返回失败）。
    Api(String),
    /// gotatun API 返回 errno（gotatun 内部错误码）。
    ApiErrno(i32),
}

/// 将错误转换为可读文本，便于上层日志与提示。
impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::ChannelClosed => write!(f, "backend channel closed"),
            EngineError::AlreadyRunning => write!(f, "backend already running"),
            EngineError::NotRunning => write!(f, "backend not running"),
            EngineError::Device(err) => write!(f, "device error: {err}"),
            EngineError::Config(err) => write!(f, "config error: {err}"),
            EngineError::Network(err) => write!(f, "network error: {err}"),
            EngineError::Api(err) => write!(f, "api error: {err}"),
            EngineError::ApiErrno(errno) => write!(f, "api errno: {errno}"),
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
                let runtime = tokio::runtime::Runtime::new()
                    .expect("failed to create backend runtime");
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
}

/// 后台线程维护的运行态状态。
///
/// 该状态只在后台线程内访问，保证串行一致性。
#[derive(Default)]
struct EngineState {
    /// gotatun 设备句柄；存在则代表已启动。
    device: Option<DeviceHandle<DefaultDeviceTransports>>,
    /// gotatun 内部 API 客户端（用于后续配置/查询）。
    api: Option<gotatun::device::api::ApiClient>,
    /// 系统网络配置状态，用于停止时回滚。
    net_state: Option<platform::NetworkState>,
}

impl EngineState {
    /// 启动设备：
    /// - 解析配置并转换为 gotatun Set 请求。
    /// - 创建 gotatun API 通道（内部用，不暴露 UAPI socket）。
    /// - 创建 TUN 并绑定 UDP socket。
    /// - 发送 Set 请求，若失败则停止设备。
    /// - 应用系统网络配置，失败则停止设备并返回错误。
    async fn start(&mut self, request: StartRequest) -> Result<(), EngineError> {
        if self.device.is_some() {
            return Err(EngineError::AlreadyRunning);
        }

        log_engine(format!(
            "start: tun={} config_len={}",
            request.tun_name,
            request.config_text.len()
        ));

        // 解析配置并映射为 gotatun 的 Set 请求。
        let parsed = config::parse_config(&request.config_text)?;
        let set_request = parsed.to_set_request().await?;
        log_engine("config parsed".to_string());

        // 建立 gotatun 内部 API 通道。
        let (api_client, api_server) = ApiServer::new();
        let config = DeviceConfig {
            api: Some(api_server),
        };
        // 使用默认 UDP 工厂 + TUN 设备实现。
        let udp_factory = UdpSocketFactory;
        let handle = DeviceHandle::<DefaultDeviceTransports>::from_tun_name(
            udp_factory,
            &request.tun_name,
            config,
        )
        .await?;
        log_engine("device created".to_string());

        // 配置 gotatun 设备；若返回 errno 需立即停止设备。
        let response = api_client
            .send(set_request)
            .await
            .map_err(|err| EngineError::Api(err.to_string()))?;
        if let Response::Set(response) = response {
            if response.errno != 0 {
                handle.stop().await;
                return Err(EngineError::ApiErrno(response.errno));
            }
        }
        log_engine("device configured".to_string());

        // 应用系统网络配置；失败时回滚 gotatun 设备。
        let net_state = match platform::apply_network_config(
            &request.tun_name,
            &parsed.interface,
            &parsed.peers,
        )
        .await
        {
            Ok(state) => state,
            Err(err) => {
                handle.stop().await;
                return Err(EngineError::Network(err));
            }
        };
        log_engine("network configured".to_string());

        // 保存运行态状态，便于后续 stop/cleanup。
        self.device = Some(handle);
        self.api = Some(api_client);
        self.net_state = Some(net_state);

        Ok(())
    }

    /// 停止设备：
    /// - 先回滚系统网络配置（若存在）。
    /// - 再停止 gotatun 设备并清空 API 客户端。
    async fn stop(&mut self) -> Result<(), EngineError> {
        let Some(handle) = self.device.take() else {
            return Err(EngineError::NotRunning);
        };

        log_engine("stop requested".to_string());

        // 优先回滚系统网络配置，避免留下路由/DNS 污染。
        let cleanup_result = if let Some(state) = self.net_state.take() {
            platform::cleanup_network_config(state)
                .await
                .map_err(EngineError::from)
        } else {
            Ok(())
        };

        // 停止 gotatun 设备。
        handle.stop().await;
        self.api = None;
        log_engine("device stopped".to_string());

        // 若回滚失败，仍然返回错误以便上层提示。
        cleanup_result
    }

    /// 查询状态。
    fn status(&self) -> EngineStatus {
        if self.device.is_some() {
            EngineStatus::Running
        } else {
            EngineStatus::Stopped
        }
    }

    /// 获取 gotatun 运行时统计信息。
    async fn stats(&self) -> Result<EngineStats, EngineError> {
        let Some(api) = self.api.as_ref() else {
            return Err(EngineError::NotRunning);
        };

        let response = api
            .send(Get::default())
            .await
            .map_err(|err| EngineError::Api(err.to_string()))?;

        let Response::Get(get) = response else {
            return Err(EngineError::Api("unexpected api response".to_string()));
        };

        if get.errno != 0 {
            return Err(EngineError::ApiErrno(get.errno));
        }

        let peers = get
            .peers
            .into_iter()
            .map(|peer| PeerStats {
                public_key: peer.peer.public_key.0,
                endpoint: peer.peer.endpoint,
                last_handshake: duration_from_parts(
                    peer.last_handshake_time_sec,
                    peer.last_handshake_time_nsec,
                ),
                rx_bytes: peer.rx_bytes.unwrap_or(0),
                tx_bytes: peer.tx_bytes.unwrap_or(0),
            })
            .collect();

        Ok(EngineStats { peers })
    }
}

fn log_engine(message: String) {
    log::log("engine", message);
}

/// 后台线程的主事件循环：
/// 接收命令并顺序执行，通道关闭后安全收尾。
///
/// 该循环是引擎的“串行化核心”，避免并发修改内部状态。
async fn run(mut rx: mpsc::Receiver<Command>) {
    let mut state = EngineState::default();

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
        }
    }

    // 通道关闭，尝试优雅停止设备。
    let _ = state.stop().await;
}

fn duration_from_parts(sec: Option<u64>, nsec: Option<u32>) -> Option<Duration> {
    match (sec, nsec) {
        (None, None) => None,
        (Some(sec), Some(nsec)) => Some(Duration::new(sec, nsec)),
        (Some(sec), None) => Some(Duration::new(sec, 0)),
        (None, Some(nsec)) => Some(Duration::new(0, nsec)),
    }
}
