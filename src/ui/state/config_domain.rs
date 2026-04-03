use std::path::PathBuf;

use gpui::SharedString;

/// 配置来源：文件或粘贴文本。
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum ConfigSource {
    File { origin_path: Option<PathBuf> },
    Paste,
}

/// Endpoint 地址族标识（基于配置文本解析）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum EndpointFamily {
    V4,
    V6,
    Dual,
    Unknown,
}

/// 隧道配置条目：用于配置列表与编辑器。
#[derive(Clone)]
pub(crate) struct TunnelConfig {
    /// 持久化 ID（用于内部文件名）。
    pub(crate) id: u64,
    /// 配置名称（用于列表与启动）。
    pub(crate) name: String,
    /// 小写版本的名称，用于搜索过滤，避免每次渲染都重复分配/转换。
    pub(crate) name_lower: String,
    /// 配置文本：文件导入时懒加载，因此可能为空。
    pub(crate) text: Option<SharedString>,
    /// 配置来源：文件路径或粘贴内容。
    pub(crate) source: ConfigSource,
    /// 内部存储路径：用于持久化读写。
    pub(crate) storage_path: PathBuf,
    /// Endpoint 地址族 metadata，供 Proxies 页直接读取，避免渲染时派生。
    pub(crate) endpoint_family: EndpointFamily,
}

/// 延迟启动请求（用于 stop -> start 过渡期间）。
#[derive(Clone, Copy)]
pub(crate) struct PendingStart {
    pub(crate) config_id: u64,
    pub(crate) password_authorized: bool,
}
