use std::net::{IpAddr, SocketAddr};

use gotatun::device::Peer as DevicePeer;
use gotatun::x25519::{PublicKey, StaticSecret};
use ipnetwork::IpNetwork;
use tokio::net::lookup_host;

use super::types::{AllowedIp, ConfigError, Endpoint, PeerConfig, WireGuardConfig};

/// gotatun 设备配置（对应解析后的 WireGuard 配置）。
pub struct DeviceSettings {
    pub private_key: StaticSecret,
    pub listen_port: Option<u16>,
    pub fwmark: Option<u32>,
    pub peers: Vec<DevicePeer>,
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
    /// 将解析后的配置映射到 gotatun 的 DeviceSettings。
    ///
    /// - 域名 Endpoint 会进行 DNS 解析，优先选择 IPv4。
    pub async fn to_device_settings(&self) -> Result<DeviceSettings, ConfigError> {
        let mut peers = Vec::with_capacity(self.peers.len());
        for peer in &self.peers {
            peers.push(peer.to_device_peer().await?);
        }

        let private_key = StaticSecret::from(*self.interface.private_key.as_bytes());

        Ok(DeviceSettings {
            private_key,
            listen_port: self.interface.listen_port,
            fwmark: self.interface.fwmark,
            peers,
        })
    }
}

impl PeerConfig {
    /// 将单个 Peer 配置映射为 gotatun 的 Peer。
    async fn to_device_peer(&self) -> Result<DevicePeer, ConfigError> {
        let endpoint = match &self.endpoint {
            Some(endpoint) => Some(endpoint.resolve_socket_addr().await?),
            None => None,
        };

        let allowed_ips = self
            .allowed_ips
            .iter()
            .map(allowed_ip_to_network)
            .collect::<Result<Vec<_>, _>>()?;

        let public_key = PublicKey::from(*self.public_key.as_bytes());
        let mut peer = DevicePeer::new(public_key);
        peer.endpoint = endpoint;
        peer.allowed_ips = allowed_ips;
        peer.preshared_key = self.preshared_key.as_ref().map(|key| *key.as_bytes());
        peer.keepalive = self.persistent_keepalive;

        Ok(peer)
    }
}

fn allowed_ip_to_network(allowed: &AllowedIp) -> Result<IpNetwork, ConfigError> {
    IpNetwork::new(allowed.addr, allowed.cidr)
        .map_err(|_| ConfigError::new(None, "invalid allowed ip"))
}
