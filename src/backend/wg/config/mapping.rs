use std::net::{IpAddr, SocketAddr};

use gotatun::device::api::command::{Peer as ApiPeer, Set, SetPeer, SetUnset};
use gotatun::device::peer::AllowedIP as ApiAllowedIp;
use tokio::net::lookup_host;

use super::types::{AllowedIp, ConfigError, Endpoint, PeerConfig, WireGuardConfig};

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

        let allowed_ip = self.allowed_ips.iter().map(ApiAllowedIp::from).collect();

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
