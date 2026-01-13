//! WireGuard 标准配置解析器：
//! - 支持 `[Interface]` 与 `[Peer]` 两个标准段。
//! - 严格校验并报告行号，便于定位配置问题。
//! - 仅解析标准字段，遇到未知字段直接报错，避免误用。
use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use gotatun::device::api::command::{Peer as ApiPeer, Set, SetPeer, SetUnset};
use gotatun::device::peer::AllowedIP as ApiAllowedIp;
use tokio::net::lookup_host;

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
        let addr = parts.next().ok_or("missing ip")?.parse::<IpAddr>().map_err(|_| "invalid ip")?;
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
    fn new(line: Option<usize>, message: impl Into<String>) -> Self {
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

/// 把解析后的 AllowedIp 转成 gotatun 使用的 AllowedIP。
impl From<&AllowedIp> for ApiAllowedIp {
    fn from(value: &AllowedIp) -> Self {
        ApiAllowedIp {
            addr: value.addr,
            cidr: value.cidr,
        }
    }
}

impl Endpoint {
    /// 将 Endpoint 解析为 SocketAddr。
    ///
    /// - 若 host 是 IP，则直接返回。
    /// - 若 host 是域名，使用异步 DNS 解析，优先选择 IPv4。
    pub async fn resolve_socket_addr(&self) -> Result<SocketAddr, ConfigError> {
        let host = self.host.trim();
        if let Ok(ip) = host.parse::<IpAddr>() {
            return Ok(SocketAddr::new(ip, self.port));
        }

        let mut addrs = lookup_host((host, self.port))
            .await
            .map_err(|_| ConfigError::new(None, "failed to resolve endpoint host"))?;

        let mut first = None;
        for addr in addrs.by_ref() {
            if first.is_none() {
                first = Some(addr);
            }
            if addr.is_ipv4() {
                return Ok(addr);
            }
        }

        first.ok_or_else(|| ConfigError::new(None, "no socket address resolved"))
    }
}

impl WireGuardConfig {
    /// 将解析后的配置映射到 gotatun 的 Set/SetPeer 请求。
    ///
    /// - 默认使用 `replace_peers = true`，保证配置是权威的。
    /// - 域名 Endpoint 会进行 DNS 解析，优先选择 IPv4。
    pub async fn to_set_request(&self) -> Result<Set, ConfigError> {
        let mut peers = Vec::with_capacity(self.peers.len());
        for peer in &self.peers {
            peers.push(peer.to_set_peer().await?);
        }

        let mut set = Set::default();
        set.private_key = Some((*self.interface.private_key.as_bytes()).into());
        set.listen_port = self.interface.listen_port;
        set.fwmark = self.interface.fwmark;
        set.replace_peers = true;
        set.protocol_version = None;
        set.peers = peers;

        Ok(set)
    }
}

impl PeerConfig {
    /// 将单个 Peer 配置映射为 gotatun 的 SetPeer。
    async fn to_set_peer(&self) -> Result<SetPeer, ConfigError> {
        let endpoint = match &self.endpoint {
            Some(endpoint) => Some(endpoint.resolve_socket_addr().await?),
            None => None,
        };

        let allowed_ip = self
            .allowed_ips
            .iter()
            .map(ApiAllowedIp::from)
            .collect();

        let mut peer = ApiPeer::builder()
            .public_key(*self.public_key.as_bytes())
            .allowed_ip(allowed_ip)
            .build();

        peer.preshared_key = self
            .preshared_key
            .as_ref()
            .map(|key| SetUnset::Set((*key.as_bytes()).into()));
        peer.endpoint = endpoint;
        peer.persistent_keepalive_interval = self.persistent_keepalive;

        let mut set_peer = SetPeer::builder().peer(peer).build();
        set_peer.replace_allowed_ips = true;

        Ok(set_peer)
    }
}

/// 当前正在解析的段。
#[derive(Clone, Copy)]
enum Section {
    Interface,
    Peer,
}

/// `[Interface]` 解析过程的临时结构。
#[derive(Default)]
struct InterfaceBuilder {
    private_key: Option<Key>,
    listen_port: Option<u16>,
    fwmark: Option<u32>,
    addresses: Vec<InterfaceAddress>,
    dns_servers: Vec<IpAddr>,
    dns_search: Vec<String>,
    mtu: Option<u16>,
    table: Option<RouteTable>,
}

impl InterfaceBuilder {
    /// 完成构建并校验必填项。
    fn finish(self, line: Option<usize>) -> Result<InterfaceConfig, ConfigError> {
        let private_key = self.private_key.ok_or_else(|| {
            ConfigError::new(line, "missing PrivateKey in [Interface] section")
        })?;
        Ok(InterfaceConfig {
            private_key,
            listen_port: self.listen_port,
            fwmark: self.fwmark,
            addresses: self.addresses,
            dns_servers: self.dns_servers,
            dns_search: self.dns_search,
            mtu: self.mtu,
            table: self.table,
        })
    }
}

/// `[Peer]` 解析过程的临时结构，记录起始行用于报错。
struct PeerBuilder {
    start_line: usize,
    public_key: Option<Key>,
    preshared_key: Option<Key>,
    allowed_ips: Vec<AllowedIp>,
    endpoint: Option<Endpoint>,
    persistent_keepalive: Option<u16>,
}

impl PeerBuilder {
    /// 新建 builder，并记录该 `[Peer]` 起始行号。
    fn new(start_line: usize) -> Self {
        Self {
            start_line,
            public_key: None,
            preshared_key: None,
            allowed_ips: Vec::new(),
            endpoint: None,
            persistent_keepalive: None,
        }
    }

    /// 完成构建并校验必填项。
    fn finish(self) -> Result<PeerConfig, ConfigError> {
        let public_key = self.public_key.ok_or_else(|| {
            ConfigError::new(
                Some(self.start_line),
                "missing PublicKey in [Peer] section",
            )
        })?;
        Ok(PeerConfig {
            public_key,
            preshared_key: self.preshared_key,
            allowed_ips: self.allowed_ips,
            endpoint: self.endpoint,
            persistent_keepalive: self.persistent_keepalive,
        })
    }
}

/// 解析标准 WireGuard `.conf` 文本。
/// - 支持 `#` 与 `;` 行内注释。
/// - 键名大小写不敏感。
/// - 多个 `[Peer]` 段会被追加到 peers 列表。
pub fn parse_config(input: &str) -> Result<WireGuardConfig, ConfigError> {
    let mut section: Option<Section> = None;
    let mut interface_builder = InterfaceBuilder::default();
    let mut interface_line: Option<usize> = None;
    let mut peers = Vec::new();
    let mut current_peer: Option<PeerBuilder> = None;
    let mut seen_interface = false;

    // 遍历逐行解析，保留行号用于错误信息。
    for (idx, raw_line) in input.lines().enumerate() {
        let line_no = idx + 1;
        let line = strip_comments(raw_line);
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(name) = parse_section_header(line) {
            match name.to_ascii_lowercase().as_str() {
                "interface" => {
                    if seen_interface {
                        return Err(ConfigError::new(
                            Some(line_no),
                            "multiple [Interface] sections are not supported",
                        ));
                    }
                    if let Some(peer) = current_peer.take() {
                        peers.push(peer.finish()?);
                    }
                    seen_interface = true;
                    interface_line = Some(line_no);
                    section = Some(Section::Interface);
                }
                "peer" => {
                    if let Some(peer) = current_peer.take() {
                        peers.push(peer.finish()?);
                    }
                    current_peer = Some(PeerBuilder::new(line_no));
                    section = Some(Section::Peer);
                }
                _ => {
                    return Err(ConfigError::new(
                        Some(line_no),
                        format!("unsupported section [{name}]"),
                    ));
                }
            }
            continue;
        }

        let Some(active) = section else {
            return Err(ConfigError::new(
                Some(line_no),
                "key/value pair found before any section header",
            ));
        };

        let (key, value) = split_key_value(line, line_no)?;

        match active {
            Section::Interface => {
                parse_interface_kv(&mut interface_builder, &key, value, line_no)?
            }
            Section::Peer => {
                let Some(peer) = current_peer.as_mut() else {
                    return Err(ConfigError::new(
                        Some(line_no),
                        "peer key/value without [Peer] section",
                    ));
                };
                parse_peer_kv(peer, &key, value, line_no)?;
            }
        }
    }

    // 收尾：把最后一个 Peer 也提交进列表。
    if let Some(peer) = current_peer.take() {
        peers.push(peer.finish()?);
    }

    if !seen_interface {
        return Err(ConfigError::new(None, "missing [Interface] section"));
    }

    let interface = interface_builder.finish(interface_line)?;

    Ok(WireGuardConfig { interface, peers })
}

/// 去除 `#` 或 `;` 之后的注释。
fn strip_comments(line: &str) -> &str {
    let mut idx = line.len();
    for (i, ch) in line.char_indices() {
        if ch == '#' || ch == ';' {
            idx = i;
            break;
        }
    }
    &line[..idx]
}

/// 解析段标题，如 `[Interface]`。
fn parse_section_header(line: &str) -> Option<&str> {
    let line = line.trim();
    if !line.starts_with('[') || !line.ends_with(']') {
        return None;
    }
    let inner = &line[1..line.len() - 1];
    Some(inner.trim())
}

/// 拆分 `key = value`。
fn split_key_value(line: &str, line_no: usize) -> Result<(String, &str), ConfigError> {
    let Some((key, value)) = line.split_once('=') else {
        return Err(ConfigError::new(
            Some(line_no),
            "expected key = value",
        ));
    };
    let key = key.trim();
    let value = value.trim();
    if key.is_empty() {
        return Err(ConfigError::new(Some(line_no), "empty key"));
    }
    if value.is_empty() {
        return Err(ConfigError::new(Some(line_no), "empty value"));
    }
    Ok((key.to_ascii_lowercase(), value))
}

/// 解析 `[Interface]` 内的字段。
fn parse_interface_kv(
    builder: &mut InterfaceBuilder,
    key: &str,
    value: &str,
    line_no: usize,
) -> Result<(), ConfigError> {
    match key {
        "privatekey" => {
            if builder.private_key.is_some() {
                return Err(ConfigError::new(
                    Some(line_no),
                    "duplicate PrivateKey in [Interface] section",
                ));
            }
            builder.private_key = Some(parse_key(value, line_no)?);
        }
        "listenport" => {
            if builder.listen_port.is_some() {
                return Err(ConfigError::new(
                    Some(line_no),
                    "duplicate ListenPort in [Interface] section",
                ));
            }
            builder.listen_port = Some(parse_u16(value, line_no, "ListenPort")?);
        }
        "fwmark" => {
            if builder.fwmark.is_some() {
                return Err(ConfigError::new(
                    Some(line_no),
                    "duplicate FwMark in [Interface] section",
                ));
            }
            builder.fwmark = parse_fwmark(value, line_no)?;
        }
        "address" => {
            let addresses = parse_interface_addresses(value, line_no)?;
            builder.addresses.extend(addresses);
        }
        "dns" => {
            let (servers, search) = parse_dns_entries(value, line_no)?;
            builder.dns_servers.extend(servers);
            builder.dns_search.extend(search);
        }
        "mtu" => {
            if builder.mtu.is_some() {
                return Err(ConfigError::new(
                    Some(line_no),
                    "duplicate MTU in [Interface] section",
                ));
            }
            builder.mtu = Some(parse_u16(value, line_no, "MTU")?);
        }
        "table" => {
            if builder.table.is_some() {
                return Err(ConfigError::new(
                    Some(line_no),
                    "duplicate Table in [Interface] section",
                ));
            }
            builder.table = Some(parse_table(value, line_no)?);
        }
        _ => {
            return Err(ConfigError::new(
                Some(line_no),
                format!("unsupported [Interface] key: {key}"),
            ));
        }
    }

    Ok(())
}

/// 解析 `[Peer]` 内的字段。
fn parse_peer_kv(
    builder: &mut PeerBuilder,
    key: &str,
    value: &str,
    line_no: usize,
) -> Result<(), ConfigError> {
    match key {
        "publickey" => {
            if builder.public_key.is_some() {
                return Err(ConfigError::new(
                    Some(line_no),
                    "duplicate PublicKey in [Peer] section",
                ));
            }
            builder.public_key = Some(parse_key(value, line_no)?);
        }
        "presharedkey" => {
            if builder.preshared_key.is_some() {
                return Err(ConfigError::new(
                    Some(line_no),
                    "duplicate PresharedKey in [Peer] section",
                ));
            }
            builder.preshared_key = Some(parse_key(value, line_no)?);
        }
        "allowedips" => {
            let allowed = parse_allowed_ips(value, line_no)?;
            builder.allowed_ips.extend(allowed);
        }
        "endpoint" => {
            if builder.endpoint.is_some() {
                return Err(ConfigError::new(
                    Some(line_no),
                    "duplicate Endpoint in [Peer] section",
                ));
            }
            builder.endpoint = Some(value.parse::<Endpoint>().map_err(|err| {
                ConfigError::new(Some(line_no), format!("invalid Endpoint: {err}"))
            })?);
        }
        "persistentkeepalive" => {
            if builder.persistent_keepalive.is_some() {
                return Err(ConfigError::new(
                    Some(line_no),
                    "duplicate PersistentKeepalive in [Peer] section",
                ));
            }
            builder.persistent_keepalive =
                Some(parse_u16(value, line_no, "PersistentKeepalive")?);
        }
        _ => {
            return Err(ConfigError::new(
                Some(line_no),
                format!("unsupported [Peer] key: {key}"),
            ));
        }
    }

    Ok(())
}

/// 解析密钥并包装成 ConfigError。
fn parse_key(value: &str, line_no: usize) -> Result<Key, ConfigError> {
    value.parse::<Key>().map_err(|err| {
        ConfigError::new(Some(line_no), format!("invalid key: {err}"))
    })
}

/// 解析 AllowedIPs，逗号分隔。
fn parse_allowed_ips(value: &str, line_no: usize) -> Result<Vec<AllowedIp>, ConfigError> {
    let mut out = Vec::new();
    for raw in value.split(',') {
        let item = raw.trim();
        if item.is_empty() {
            continue;
        }
        let allowed = item.parse::<AllowedIp>().map_err(|err| {
            ConfigError::new(Some(line_no), format!("invalid AllowedIPs entry: {err}"))
        })?;
        out.push(allowed);
    }
    if out.is_empty() {
        return Err(ConfigError::new(
            Some(line_no),
            "AllowedIPs must contain at least one entry",
        ));
    }
    Ok(out)
}

/// 解析 Address，逗号分隔。
fn parse_interface_addresses(
    value: &str,
    line_no: usize,
) -> Result<Vec<InterfaceAddress>, ConfigError> {
    let mut out = Vec::new();
    for raw in value.split(',') {
        let item = raw.trim();
        if item.is_empty() {
            continue;
        }
        let addr = item.parse::<InterfaceAddress>().map_err(|err| {
            ConfigError::new(Some(line_no), format!("invalid Address entry: {err}"))
        })?;
        out.push(addr);
    }
    if out.is_empty() {
        return Err(ConfigError::new(
            Some(line_no),
            "Address must contain at least one entry",
        ));
    }
    Ok(out)
}

/// 解析 DNS，逗号分隔。
fn parse_dns_entries(
    value: &str,
    line_no: usize,
) -> Result<(Vec<IpAddr>, Vec<String>), ConfigError> {
    let mut servers = Vec::new();
    let mut search = Vec::new();
    for raw in value.split(',') {
        let item = raw.trim();
        if item.is_empty() {
            continue;
        }
        if let Ok(ip) = item.parse::<IpAddr>() {
            servers.push(ip);
        } else {
            search.push(item.to_string());
        }
    }
    if servers.is_empty() && search.is_empty() {
        return Err(ConfigError::new(Some(line_no), "DNS must not be empty"));
    }
    Ok((servers, search))
}

/// 解析 Table，支持 auto/off/数字。
fn parse_table(value: &str, line_no: usize) -> Result<RouteTable, ConfigError> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("auto") {
        return Ok(RouteTable::Auto);
    }
    if value.eq_ignore_ascii_case("off") {
        return Ok(RouteTable::Off);
    }
    let table = value.parse::<u32>().map_err(|_| {
        ConfigError::new(Some(line_no), "invalid Table value")
    })?;
    Ok(RouteTable::Id(table))
}

/// 解析 u16 类型字段。
fn parse_u16(value: &str, line_no: usize, name: &str) -> Result<u16, ConfigError> {
    value.parse::<u16>().map_err(|_| {
        ConfigError::new(Some(line_no), format!("invalid {name} value"))
    })
}

/// 解析 FwMark，支持 `off`、`0`、十六进制与十进制。
fn parse_fwmark(value: &str, line_no: usize) -> Result<Option<u32>, ConfigError> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("off") || value == "0" {
        return Ok(None);
    }

    let parsed = if let Some(hex) = value.strip_prefix("0x") {
        u32::from_str_radix(hex, 16).map_err(|_| {
            ConfigError::new(Some(line_no), "invalid FwMark hex value")
        })?
    } else {
        value.parse::<u32>().map_err(|_| {
            ConfigError::new(Some(line_no), "invalid FwMark value")
        })?
    };

    Ok(Some(parsed))
}
