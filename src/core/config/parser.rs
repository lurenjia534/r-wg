use std::net::IpAddr;

use super::types::{
    AllowedIp, ConfigError, Endpoint, InterfaceAddress, InterfaceConfig, Key, PeerConfig,
    RouteTable, WireGuardConfig,
};

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
        let private_key = self
            .private_key
            .ok_or_else(|| ConfigError::new(line, "missing PrivateKey in [Interface] section"))?;
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
            ConfigError::new(Some(self.start_line), "missing PublicKey in [Peer] section")
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
    let input = input.trim_start_matches('\u{feff}');
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
            Section::Interface => parse_interface_kv(&mut interface_builder, &key, value, line_no)?,
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
        return Err(ConfigError::new(Some(line_no), "expected key = value"));
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
            builder.persistent_keepalive = Some(parse_u16(value, line_no, "PersistentKeepalive")?);
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
    value
        .parse::<Key>()
        .map_err(|err| ConfigError::new(Some(line_no), format!("invalid key: {err}")))
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
    let table = value
        .parse::<u32>()
        .map_err(|_| ConfigError::new(Some(line_no), "invalid Table value"))?;
    Ok(RouteTable::Id(table))
}

/// 解析 u16 类型字段。
fn parse_u16(value: &str, line_no: usize, name: &str) -> Result<u16, ConfigError> {
    value
        .parse::<u16>()
        .map_err(|_| ConfigError::new(Some(line_no), format!("invalid {name} value")))
}

/// 解析 FwMark，支持 `off`、`0`、十六进制与十进制。
fn parse_fwmark(value: &str, line_no: usize) -> Result<Option<u32>, ConfigError> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("off") || value == "0" {
        return Ok(None);
    }

    let parsed = if let Some(hex) = value.strip_prefix("0x") {
        u32::from_str_radix(hex, 16)
            .map_err(|_| ConfigError::new(Some(line_no), "invalid FwMark hex value"))?
    } else {
        value
            .parse::<u32>()
            .map_err(|_| ConfigError::new(Some(line_no), "invalid FwMark value"))?
    };

    Ok(Some(parsed))
}
