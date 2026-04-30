use std::fmt;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use futures_util::stream::TryStreamExt;
use gotatun::device::Peer as DevicePeer;
use nl_wireguard::{WireguardIpAddress, WireguardParsed, WireguardPeerParsed};
use rtnetlink::{new_connection, LinkWireguard};
use tokio::task::JoinHandle;

use super::engine::{DaitaStats, EngineStats, PeerStats};
use super::ephemeral::EphemeralPeerUpdate;
use crate::core::config::DeviceSettings;

#[derive(Debug)]
pub(crate) struct KernelWireGuardDevice {
    tun_name: String,
}

impl KernelWireGuardDevice {
    pub(crate) fn tun_name(&self) -> &str {
        &self.tun_name
    }
}

#[derive(Debug)]
pub(crate) enum KernelWireGuardError {
    Unavailable(String),
    Operation(String),
}

impl KernelWireGuardError {
    pub(crate) fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable(_))
    }
}

impl fmt::Display for KernelWireGuardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(message) | Self::Operation(message) => {
                write!(f, "{message}")
            }
        }
    }
}

impl std::error::Error for KernelWireGuardError {}

pub(crate) async fn start_kernel_device(
    tun_name: &str,
    settings: &DeviceSettings,
) -> Result<KernelWireGuardDevice, KernelWireGuardError> {
    create_wireguard_link(tun_name).await?;
    if let Err(error) = configure_wireguard_device(tun_name, settings).await {
        let _ = delete_kernel_device(tun_name).await;
        return Err(error);
    }
    Ok(KernelWireGuardDevice {
        tun_name: tun_name.to_string(),
    })
}

pub(crate) async fn delete_kernel_device(tun_name: &str) -> Result<(), KernelWireGuardError> {
    let connection = route_connection()?;
    let handle = connection.handle.clone();
    let link = find_link_by_name(&handle, tun_name).await?;
    handle
        .link()
        .del(link.header.index)
        .execute()
        .await
        .map_err(map_route_error)?;
    connection.shutdown().await;
    Ok(())
}

pub(crate) async fn read_kernel_stats(tun_name: &str) -> Result<EngineStats, KernelWireGuardError> {
    let (conn, mut handle, _) = nl_wireguard::new_connection()
        .map_err(|error| unavailable(format!("failed to open wireguard netlink: {error}")))?;
    let task = tokio::spawn(conn);
    let parsed = handle
        .get_by_name(tun_name)
        .await
        .map_err(|error| operation(format!("failed to read kernel WireGuard stats: {error}")))?;
    task.abort();
    let _ = task.await;

    let peers = parsed
        .peers
        .unwrap_or_default()
        .into_iter()
        .filter_map(peer_stats_from_kernel_peer)
        .collect();

    Ok(EngineStats { peers })
}

pub(crate) async fn apply_ephemeral_update(
    tun_name: &str,
    update: &EphemeralPeerUpdate,
) -> Result<(), KernelWireGuardError> {
    let mut config = WireguardParsed::default();
    config.iface_name = Some(tun_name.to_string());
    config.private_key = Some(STANDARD.encode(update.private_key.to_bytes()));
    config.peers = Some(vec![kernel_peer_from_device_peer(&update.peer)]);
    set_wireguard_config(config, "failed to apply kernel WireGuard ephemeral update").await?;
    tracing::debug!(
        tun = tun_name,
        "Mullvad ephemeral peer configuration applied to kernel WireGuard"
    );
    Ok(())
}

async fn create_wireguard_link(tun_name: &str) -> Result<(), KernelWireGuardError> {
    let connection = route_connection()?;
    let handle = connection.handle.clone();
    let result = handle
        .link()
        .add(LinkWireguard::new(tun_name).build())
        .execute()
        .await
        .map_err(map_route_error);
    connection.shutdown().await;
    result
}

async fn configure_wireguard_device(
    tun_name: &str,
    settings: &DeviceSettings,
) -> Result<(), KernelWireGuardError> {
    let mut config = WireguardParsed::default();
    config.iface_name = Some(tun_name.to_string());
    config.private_key = Some(STANDARD.encode(settings.private_key.to_bytes()));
    config.listen_port = settings.listen_port;
    config.fwmark = settings.fwmark;
    config.peers = Some(
        settings
            .peers
            .iter()
            .map(kernel_peer_from_device_peer)
            .collect(),
    );

    set_wireguard_config(config, "failed to configure kernel WireGuard").await
}

async fn set_wireguard_config(
    mut config: WireguardParsed,
    operation_context: &'static str,
) -> Result<(), KernelWireGuardError> {
    let (conn, mut handle, _) = nl_wireguard::new_connection()
        .map_err(|error| unavailable(format!("failed to open wireguard netlink: {error}")))?;
    let task = tokio::spawn(conn);
    let result = handle
        .set(std::mem::take(&mut config))
        .await
        .map_err(|error| {
            let message = error.to_string();
            if is_unavailable_message(&message) {
                unavailable(format!(
                    "kernel WireGuard generic netlink unavailable: {message}"
                ))
            } else {
                operation(format!("{operation_context}: {message}"))
            }
        });
    task.abort();
    let _ = task.await;
    result
}

fn kernel_peer_from_device_peer(peer: &DevicePeer) -> WireguardPeerParsed {
    let mut parsed = WireguardPeerParsed::default();
    parsed.endpoint = peer.endpoint;
    parsed.public_key = Some(STANDARD.encode(peer.public_key.to_bytes()));
    parsed.preshared_key = peer.preshared_key.map(|key| STANDARD.encode(key));
    parsed.persistent_keepalive = peer.keepalive;
    parsed.allowed_ips = Some(
        peer.allowed_ips
            .iter()
            .map(|network| WireguardIpAddress {
                ip_addr: network.ip(),
                prefix_length: network.prefix(),
            })
            .collect(),
    );
    parsed
}

fn peer_stats_from_kernel_peer(peer: WireguardPeerParsed) -> Option<PeerStats> {
    let public_key = decode_key(peer.public_key.as_deref()?)?;
    Some(PeerStats {
        public_key,
        endpoint: peer.endpoint,
        last_handshake: peer.last_handshake,
        rx_bytes: peer.rx_bytes.unwrap_or(0),
        tx_bytes: peer.tx_bytes.unwrap_or(0),
        daita: None::<DaitaStats>,
    })
}

fn decode_key(encoded: &str) -> Option<[u8; 32]> {
    let decoded = STANDARD.decode(encoded).ok()?;
    decoded.try_into().ok()
}

struct RouteConnection {
    handle: rtnetlink::Handle,
    task: JoinHandle<()>,
}

impl RouteConnection {
    async fn shutdown(self) {
        let RouteConnection { handle, task } = self;
        drop(handle);
        task.abort();
        let _ = task.await;
    }
}

fn route_connection() -> Result<RouteConnection, KernelWireGuardError> {
    let (connection, handle, _) = new_connection()
        .map_err(|error| operation(format!("failed to open route netlink: {error}")))?;
    let task = tokio::spawn(connection);
    Ok(RouteConnection { handle, task })
}

async fn find_link_by_name(
    handle: &rtnetlink::Handle,
    tun_name: &str,
) -> Result<netlink_packet_route::link::LinkMessage, KernelWireGuardError> {
    let mut links = handle
        .link()
        .get()
        .match_name(tun_name.to_string())
        .execute();
    links
        .try_next()
        .await
        .map_err(map_route_error)?
        .ok_or_else(|| operation(format!("kernel WireGuard link not found: {tun_name}")))
}

fn map_route_error(error: rtnetlink::Error) -> KernelWireGuardError {
    let message = error.to_string();
    if is_unavailable_message(&message) {
        unavailable(format!(
            "Linux kernel WireGuard unavailable while creating interface: {message}"
        ))
    } else {
        operation(format!("kernel WireGuard netlink error: {message}"))
    }
}

fn is_unavailable_message(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("operation not supported")
        || message.contains("no such device")
        || message.contains("address family not supported")
        || message.contains("family not found")
        || message.contains("wireguard generic netlink")
        || message.contains("not supported")
}

fn unavailable(message: String) -> KernelWireGuardError {
    KernelWireGuardError::Unavailable(message)
}

fn operation(message: String) -> KernelWireGuardError {
    KernelWireGuardError::Operation(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_message_matches_kernel_capability_failures() {
        for message in [
            "Operation not supported",
            "No such device",
            "Address family not supported by protocol",
            "family not found",
            "wireguard generic netlink unavailable",
        ] {
            assert!(is_unavailable_message(message), "{message}");
        }
    }

    #[test]
    fn unavailable_message_rejects_configuration_failures() {
        for message in [
            "invalid public key",
            "permission denied",
            "failed to configure peer endpoint",
            "invalid allowed ip",
        ] {
            assert!(!is_unavailable_message(message), "{message}");
        }
    }

    #[tokio::test]
    async fn privileged_kernel_device_smoke_test() {
        if std::env::var("R_WG_PRIVILEGED_LINUX_WG_TESTS")
            .ok()
            .as_deref()
            != Some("1")
        {
            return;
        }

        let tun_name = format!("rwgt{}", std::process::id() % 100_000);
        let _ = delete_kernel_device(&tun_name).await;
        let settings = DeviceSettings {
            private_key: gotatun::x25519::StaticSecret::from([7u8; 32]),
            listen_port: None,
            fwmark: Some(0x52_57_47),
            peers: Vec::new(),
        };

        let device = start_kernel_device(&tun_name, &settings)
            .await
            .expect("kernel WireGuard device should start");
        assert_eq!(device.tun_name(), tun_name);
        let stats = read_kernel_stats(&tun_name)
            .await
            .expect("kernel WireGuard stats should be readable");
        assert!(stats.peers.is_empty());
        delete_kernel_device(&tun_name)
            .await
            .expect("kernel WireGuard device should be deleted");
    }
}
