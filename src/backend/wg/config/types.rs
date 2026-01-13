use std::fmt;
use std::net::IpAddr;
use std::str::FromStr;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;

/// 解析后的完整配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireGuardConfig {
    pub interface: InterfaceConfig,
    pub peers: Vec<PeerConfig>,
}

/// `[Interface]` 段配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceConfig {
    pub private_key: Key,
    pub listen_port: Option<u16>,
    pub fwmark: Option<u32>,
    /// wg-quick: Address
    pub addresses: Vec<InterfaceAddress>,
    /// wg-quick: DNS 服务器
    pub dns_servers: Vec<IpAddr>,
    /// wg-quick: DNS 搜索域
    pub dns_search: Vec<String>,
    /// wg-quick: MTU
    pub mtu: Option<u16>,
    /// wg-quick: Table
    pub table: Option<RouteTable>,
}

/// 接口地址，支持 `IP/CIDR`，若未提供 CIDR 则使用 /32 或 /128。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceAddress {
    pub addr: IpAddr,
    pub cidr: u8,
}

impl FromStr for InterfaceAddress {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value.trim();
        if value.is_empty() {
            return Err("address is empty");
        }

        let (addr, cidr) = if let Some((addr_str, cidr_str)) = value.split_once('/') {
            let addr = addr_str.parse::<IpAddr>().map_err(|_| "invalid ip")?;
            let cidr = cidr_str.parse::<u8>().map_err(|_| "invalid cidr")?;
            (addr, cidr)
        } else {
            let addr = value.parse::<IpAddr>().map_err(|_| "invalid ip")?;
            let cidr = match addr {
                IpAddr::V4(_) => 32,
                IpAddr::V6(_) => 128,
            };
            (addr, cidr)
        };

        match addr {
            IpAddr::V4(_) if cidr <= 32 => Ok(Self { addr, cidr }),
            IpAddr::V6(_) if cidr <= 128 => Ok(Self { addr, cidr }),
            _ => Err("invalid cidr for ip version"),
        }
    }
}

/// 路由表配置（wg-quick: Table）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteTable {
    Auto,
    Off,
    Id(u32),
}

/// `[Peer]` 段配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerConfig {
    pub public_key: Key,
    pub preshared_key: Option<Key>,
    pub allowed_ips: Vec<AllowedIp>,
    pub endpoint: Option<Endpoint>,
    pub persistent_keepalive: Option<u16>,
}

/// 允许的 IP/网段，使用 CIDR 表示。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllowedIp {
    pub addr: IpAddr,
    pub cidr: u8,
}

impl FromStr for AllowedIp {
    type Err = &'static str;

    /// 支持 `IP/CIDR`，并按 IPv4/IPv6 校验 CIDR 范围。
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut parts = value.split('/');
        let addr = parts
            .next()
            .ok_or("missing ip")?
            .parse::<IpAddr>()
            .map_err(|_| "invalid ip")?;
        let cidr = parts
            .next()
            .ok_or("missing cidr")?
            .parse::<u8>()
            .map_err(|_| "invalid cidr")?;
        if parts.next().is_some() {
            return Err("invalid allowed ip format");
        }

        match addr {
            IpAddr::V4(_) if cidr <= 32 => Ok(Self { addr, cidr }),
            IpAddr::V6(_) if cidr <= 128 => Ok(Self { addr, cidr }),
            _ => Err("invalid cidr for ip version"),
        }
    }
}

/// 远端端点信息，保留主机名与端口（主机名不强制解析）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
}

impl FromStr for Endpoint {
    type Err = &'static str;

    /// 支持 `host:port` 与 `[ipv6]:port`。
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value.trim();
        if value.is_empty() {
            return Err("endpoint is empty");
        }

        let (host, port_str) = if let Some(rest) = value.strip_prefix('[') {
            let end = rest.find(']').ok_or("missing ']' in endpoint")?;
            let host = &rest[..end];
            let port_str = rest[end + 1..]
                .strip_prefix(':')
                .ok_or("missing port in endpoint")?;
            (host, port_str)
        } else {
            let (host, port_str) = value
                .rsplit_once(':')
                .ok_or("missing port in endpoint")?;
            (host, port_str)
        };

        if host.is_empty() {
            return Err("endpoint host is empty");
        }

        let port = port_str.parse::<u16>().map_err(|_| "invalid endpoint port")?;
        if port == 0 {
            return Err("endpoint port must be non-zero");
        }

        Ok(Self {
            host: host.to_string(),
            port,
        })
    }
}

/// WireGuard 32 字节密钥。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key([u8; 32]);

impl Key {
    /// 获取内部字节数组。
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl FromStr for Key {
    type Err = &'static str;

    /// 支持 64 字节十六进制或 Base64（43/44 字符）。
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value.trim();
        if value.is_empty() {
            return Err("key is empty");
        }

        let mut bytes = [0u8; 32];
        match value.len() {
            64 => {
                for i in 0..32 {
                    bytes[i] = u8::from_str_radix(&value[i * 2..=i * 2 + 1], 16)
                        .map_err(|_| "invalid hex key")?;
                }
            }
            43 | 44 => {
                let decoded = STANDARD.decode(value).map_err(|_| "invalid base64 key")?;
                if decoded.len() != bytes.len() {
                    return Err("invalid key length");
                }
                bytes.copy_from_slice(&decoded);
            }
            _ => return Err("invalid key length"),
        }

        Ok(Self(bytes))
    }
}

/// 解析错误，包含可选行号与错误消息。
#[derive(Debug, Clone)]
pub struct ConfigError {
    pub line: Option<usize>,
    pub message: String,
}

impl ConfigError {
    /// 构造带行号的错误。
    pub(crate) fn new(line: Option<usize>, message: impl Into<String>) -> Self {
        Self {
            line,
            message: message.into(),
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(line) = self.line {
            write!(f, "line {line}: {}", self.message)
        } else {
            write!(f, "{}", self.message)
        }
    }
}

impl std::error::Error for ConfigError {}
