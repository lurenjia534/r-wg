//! WireGuard 后端引擎：
//! - 在独立线程中创建 tokio runtime，避免占用 UI 线程或依赖外部运行时。
//! - 通过 MPSC 命令通道驱动 gotatun 设备的生命周期。
//! - 对外提供同步 API（start/stop/status），内部异步执行。
use std::fmt;
use std::sync::Arc;

use gotatun::device::{
    self,
    api::{command::Response, ApiServer},
    DeviceConfig, DeviceHandle, DefaultDeviceTransports,
};
use gotatun::udp::socket::UdpSocketFactory;
use tokio::sync::{mpsc, oneshot};

use super::config::{self, ConfigError};
/// 启动请求：包含 TUN 设备名称与配置文本。
#[derive(Debug, Clone)]
pub struct StartRequest {
    pub tun_name: String,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineStatus {
    Stopped,
    Running,
}

/// 引擎错误类型。
/// 仅涵盖启动/停止/状态查询相关的错误面。
#[derive(Debug)]
pub enum EngineError {
    /// 后台命令通道已关闭（通常是后台线程退出）。
    ChannelClosed,
    /// 重复启动。
    AlreadyRunning,
    /// 未启动却请求停止或其它操作。
    NotRunning,
    /// gotatun 设备层错误。
    Device(device::Error),
    /// WireGuard 配置错误。
    Config(ConfigError),
    /// gotatun API 请求失败。
    Api(String),
    /// gotatun API 返回 errno。
    ApiErrno(i32),
}

/// 将错误转换为可读文本。
impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::ChannelClosed => write!(f, "backend channel closed"),
            EngineError::AlreadyRunning => write!(f, "backend already running"),
            EngineError::NotRunning => write!(f, "backend not running"),
            EngineError::Device(err) => write!(f, "device error: {err}"),
            EngineError::Config(err) => write!(f, "config error: {err}"),
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

/// 前端/调用方持有的引擎句柄。
/// 内部通过 Arc 共享，避免多处 clone 造成状态分裂。
#[derive(Clone)]
pub struct Engine {
    inner: Arc<EngineInner>,
}

/// 引擎内部共享数据，目前只有命令发送端。
struct EngineInner {
    tx: mpsc::Sender<Command>,
}

/// 后台线程接收的命令类型。
enum Command {
    /// 启动引擎并回传结果。
    Start(StartRequest, oneshot::Sender<Result<(), EngineError>>),
    /// 停止引擎并回传结果。
    Stop(oneshot::Sender<Result<(), EngineError>>),
    /// 查询状态。
    Status(oneshot::Sender<EngineStatus>),
}

impl Engine {
    /// 创建引擎：初始化通道并启动后台线程 + tokio runtime。
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
}

/// 后台线程维护的运行态状态。
#[derive(Default)]
struct EngineState {
    /// gotatun 设备句柄；存在则代表已启动。
    device: Option<DeviceHandle<DefaultDeviceTransports>>,
    /// gotatun 内部 API 客户端（用于后续配置/查询）。
    api: Option<gotatun::device::api::ApiClient>,
}

impl EngineState {
    /// 启动设备：
    /// - 创建 gotatun API 通道（内部用，不暴露 UAPI socket）。
    /// - 创建 TUN 并绑定 UDP socket。
    async fn start(&mut self, request: StartRequest) -> Result<(), EngineError> {
        if self.device.is_some() {
            return Err(EngineError::AlreadyRunning);
        }

        let parsed = config::parse_config(&request.config_text)?;
        let set_request = parsed.to_set_request().await?;

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

        // 保存运行态状态。
        self.device = Some(handle);
        self.api = Some(api_client);

        Ok(())
    }

    /// 停止设备：释放句柄并清空 API 客户端。
    async fn stop(&mut self) -> Result<(), EngineError> {
        let Some(handle) = self.device.take() else {
            return Err(EngineError::NotRunning);
        };
        handle.stop().await;
        self.api = None;
        Ok(())
    }

    /// 查询状态。
    fn status(&self) -> EngineStatus {
        if self.device.is_some() {
            EngineStatus::Running
        } else {
            EngineStatus::Stopped
        }
    }
}

/// 后台线程的主事件循环：
/// 接收命令并顺序执行，通道关闭后安全收尾。
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
        }
    }

    // 通道关闭，尝试优雅停止设备。
    let _ = state.stop().await;
}
