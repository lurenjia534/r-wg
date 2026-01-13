//! WireGuard 标准配置解析器：
//! - 支持 `[Interface]` 与 `[Peer]` 两个标准段。
//! - 严格校验并报告行号，便于定位配置问题。
//! - 仅解析标准字段，遇到未知字段直接报错，避免误用。

mod mapping;
mod parser;
mod types;

pub use parser::parse_config;
pub use types::{
    AllowedIp, ConfigError, Endpoint, InterfaceAddress, InterfaceConfig, Key, PeerConfig,
    RouteTable, WireGuardConfig,
};
