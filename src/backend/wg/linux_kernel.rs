use std::env;
use std::fmt;
use std::fs;
use std::io::Write;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use futures_util::{stream::TryStreamExt, StreamExt};
use gotatun::device::Peer as DevicePeer;
use netlink_packet_route::link::{InfoKind, LinkAttribute, LinkInfo, LinkMessage};
use nl_wireguard::{
    WireguardAddressFamily, WireguardAllowedIp, WireguardAllowedIpAttr, WireguardAttribute,
    WireguardCmd, WireguardMessage, WireguardPeer, WireguardPeerAttribute, WireguardPeerParsed,
};
use rtnetlink::{new_connection, LinkWireguard};
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;

use super::engine::{DaitaStats, EngineStats, PeerStats};
use super::ephemeral::EphemeralPeerUpdate;
use crate::core::config::DeviceSettings;

const NLM_F_REQUEST: u16 = 1;
const NLM_F_ACK: u16 = 4;
const WGDEVICE_F_REPLACE_PEERS: u32 = 1 << 0;
const WGPEER_F_REPLACE_ALLOWEDIPS: u32 = 1 << 1;

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
    NameConflict(String),
}

impl KernelWireGuardError {
    pub(crate) fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable(_))
    }
}

impl fmt::Display for KernelWireGuardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(message) | Self::Operation(message) | Self::NameConflict(message) => {
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
    cleanup_journaled_link_before_create(tun_name).await?;
    write_kernel_backend_journal(tun_name, KernelBackendPhase::CreatingLink)?;
    if let Err(error) = create_wireguard_link(tun_name).await {
        if let Err(clear_error) = clear_kernel_backend_journal() {
            tracing::warn!(
                "kernel backend journal may be stale after create failure: {clear_error}"
            );
        }
        return Err(error);
    }
    write_kernel_backend_journal(tun_name, KernelBackendPhase::LinkCreated)?;
    if let Err(error) = configure_wireguard_device(tun_name, settings).await {
        if delete_kernel_device(tun_name).await.is_ok() {
            let _ = clear_kernel_backend_journal();
        }
        return Err(error);
    }
    Ok(KernelWireGuardDevice {
        tun_name: tun_name.to_string(),
    })
}

pub(crate) fn mark_kernel_device_running(tun_name: &str) -> Result<(), KernelWireGuardError> {
    write_kernel_backend_journal(tun_name, KernelBackendPhase::Running)
}

pub(crate) async fn delete_kernel_device(tun_name: &str) -> Result<(), KernelWireGuardError> {
    let connection = route_connection()?;
    let handle = connection.handle.clone();
    if let Some(link) = find_link_by_name(&handle, tun_name).await? {
        if !is_wireguard_link(&link) {
            connection.shutdown().await;
            return Err(operation(format!(
                "refusing to delete non-WireGuard link '{tun_name}' during kernel backend cleanup"
            )));
        }
        handle
            .link()
            .del(link.header.index)
            .execute()
            .await
            .map_err(map_route_error)?;
    }
    connection.shutdown().await;
    Ok(())
}

pub(crate) async fn repair_stale_kernel_device_from_journal() -> Result<(), KernelWireGuardError> {
    let Some(journal) = load_kernel_backend_journal()? else {
        return Ok(());
    };
    delete_kernel_device(&journal.tun_name).await?;
    clear_kernel_backend_journal()
}

pub(crate) fn repair_stale_kernel_device_from_journal_sync() -> Result<(), KernelWireGuardError> {
    let runtime = tokio::runtime::Runtime::new()
        .map_err(|error| operation(format!("failed to create startup repair runtime: {error}")))?;
    runtime.block_on(async { repair_stale_kernel_device_from_journal().await })
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
    let attributes = vec![
        WireguardAttribute::IfName(tun_name.to_string()),
        WireguardAttribute::PrivateKey(update.private_key.to_bytes()),
        WireguardAttribute::Flags(WGDEVICE_F_REPLACE_PEERS),
        WireguardAttribute::Peers(vec![kernel_peer_from_device_peer(&update.peer)]),
    ];
    set_wireguard_config(
        attributes,
        "failed to apply kernel WireGuard ephemeral update",
    )
    .await?;
    tracing::debug!(
        tun = tun_name,
        "Mullvad ephemeral peer configuration applied to kernel WireGuard"
    );
    Ok(())
}

async fn create_wireguard_link(tun_name: &str) -> Result<(), KernelWireGuardError> {
    let connection = route_connection()?;
    let handle = connection.handle.clone();
    if find_link_by_name(&handle, tun_name).await?.is_some() {
        connection.shutdown().await;
        return Err(KernelWireGuardError::NameConflict(format!(
            "kernel WireGuard interface name conflict: link '{tun_name}' already exists"
        )));
    }
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
    let mut attributes = vec![
        WireguardAttribute::IfName(tun_name.to_string()),
        WireguardAttribute::PrivateKey(settings.private_key.to_bytes()),
        WireguardAttribute::Flags(WGDEVICE_F_REPLACE_PEERS),
    ];
    if let Some(port) = settings.listen_port {
        attributes.push(WireguardAttribute::ListenPort(port));
    }
    if let Some(fwmark) = settings.fwmark {
        attributes.push(WireguardAttribute::Fwmark(fwmark));
    }
    // Linux WireGuard netlink supports fragmented SetDevice writes for very
    // large configs. r-wg currently emits one small Mullvad-style config
    // message and relies on the existing single-hop config validation for
    // quantum/DAITA upgrade paths.
    attributes.push(WireguardAttribute::Peers(
        settings
            .peers
            .iter()
            .map(kernel_peer_from_device_peer)
            .collect(),
    ));

    set_wireguard_config(attributes, "failed to configure kernel WireGuard").await
}

async fn set_wireguard_config(
    attributes: Vec<WireguardAttribute>,
    operation_context: &'static str,
) -> Result<(), KernelWireGuardError> {
    let (conn, mut handle, _) = nl_wireguard::new_connection()
        .map_err(|error| unavailable(format!("failed to open wireguard netlink: {error}")))?;
    let task = tokio::spawn(conn);
    let message = WireguardMessage {
        cmd: WireguardCmd::SetDevice,
        attributes,
    };
    let result = match handle.request(NLM_F_REQUEST | NLM_F_ACK, message).await {
        Ok(mut replies) => match replies.next().await {
            None | Some(Ok(_)) => Ok(()),
            Some(Err(error)) => Err(error),
        },
        Err(error) => Err(error),
    }
    .map_err(|error| {
        let message = error.to_string();
        if is_wireguard_family_unavailable_message(&message) {
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

fn kernel_peer_from_device_peer(peer: &DevicePeer) -> WireguardPeer {
    let mut attributes = vec![
        WireguardPeerAttribute::PublicKey(peer.public_key.to_bytes()),
        WireguardPeerAttribute::Flags(WGPEER_F_REPLACE_ALLOWEDIPS),
        WireguardPeerAttribute::AllowedIps(
            peer.allowed_ips
                .iter()
                .map(|network| {
                    WireguardAllowedIp(vec![
                        WireguardAllowedIpAttr::Family(match network.ip() {
                            IpAddr::V4(_) => WireguardAddressFamily::Ipv4,
                            IpAddr::V6(_) => WireguardAddressFamily::Ipv6,
                        }),
                        WireguardAllowedIpAttr::IpAddr(network.ip()),
                        WireguardAllowedIpAttr::Cidr(network.prefix()),
                    ])
                })
                .collect(),
        ),
    ];
    if let Some(key) = peer.preshared_key {
        attributes.push(WireguardPeerAttribute::PresharedKey(key));
    }
    if let Some(endpoint) = peer.endpoint {
        attributes.push(WireguardPeerAttribute::Endpoint(endpoint));
    }
    if let Some(keepalive) = peer.keepalive {
        attributes.push(WireguardPeerAttribute::PersistentKeepalive(keepalive));
    }
    WireguardPeer(attributes)
}

fn peer_stats_from_kernel_peer(peer: WireguardPeerParsed) -> Option<PeerStats> {
    let public_key = decode_key(peer.public_key.as_deref()?)?;
    Some(PeerStats {
        public_key,
        endpoint: peer.endpoint,
        last_handshake: peer.last_handshake.and_then(kernel_handshake_age),
        rx_bytes: peer.rx_bytes.unwrap_or(0),
        tx_bytes: peer.tx_bytes.unwrap_or(0),
        daita: None::<DaitaStats>,
    })
}

fn kernel_handshake_age(since_epoch: Duration) -> Option<Duration> {
    let when = SystemTime::UNIX_EPOCH.checked_add(since_epoch)?;
    SystemTime::now().duration_since(when).ok()
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
) -> Result<Option<LinkMessage>, KernelWireGuardError> {
    let mut links = handle
        .link()
        .get()
        .match_name(tun_name.to_string())
        .execute();
    match links.try_next().await {
        Ok(link) => Ok(link),
        Err(error) if is_route_link_not_found_error(&error) => Ok(None),
        Err(error) => Err(map_route_error(error)),
    }
}

fn is_wireguard_link(link: &LinkMessage) -> bool {
    link_info_kind(link) == Some(InfoKind::Wireguard)
}

fn link_info_kind(link: &LinkMessage) -> Option<InfoKind> {
    link.attributes.iter().find_map(|attr| match attr {
        LinkAttribute::LinkInfo(infos) => infos.iter().find_map(|info| match info {
            LinkInfo::Kind(kind) => Some(kind.clone()),
            _ => None,
        }),
        _ => None,
    })
}

async fn cleanup_journaled_link_before_create(tun_name: &str) -> Result<(), KernelWireGuardError> {
    let Some(journal) = load_kernel_backend_journal()? else {
        return Ok(());
    };
    if journal.tun_name != tun_name {
        return Ok(());
    }
    delete_kernel_device(tun_name).await?;
    clear_kernel_backend_journal()
}

fn map_route_error(error: rtnetlink::Error) -> KernelWireGuardError {
    let message = error.to_string();
    if is_unavailable_route_error(&error) || is_kernel_link_unavailable_message(&message) {
        unavailable(format!(
            "Linux kernel WireGuard unavailable while creating interface: {message}"
        ))
    } else {
        operation(format!("kernel WireGuard netlink error: {message}"))
    }
}

fn is_unavailable_route_error(error: &rtnetlink::Error) -> bool {
    let Some(errno) = route_error_errno(error) else {
        return false;
    };
    errno == libc::EOPNOTSUPP || errno == libc::ENODEV || errno == libc::EAFNOSUPPORT
}

fn is_route_link_not_found_error(error: &rtnetlink::Error) -> bool {
    route_error_errno(error) == Some(libc::ENODEV)
}

fn route_error_errno(error: &rtnetlink::Error) -> Option<i32> {
    let rtnetlink::Error::NetlinkError(message) = error else {
        return None;
    };
    Some(message.code?.get().abs())
}

fn is_kernel_link_unavailable_message(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("operation not supported")
        || message.contains("no such device")
        || message.contains("address family not supported")
        || message.contains("family not found")
        || message.contains("wireguard generic netlink")
}

fn is_wireguard_family_unavailable_message(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("family not found") || message.contains("wireguard generic netlink")
}

fn unavailable(message: String) -> KernelWireGuardError {
    KernelWireGuardError::Unavailable(message)
}

fn operation(message: String) -> KernelWireGuardError {
    KernelWireGuardError::Operation(message)
}

const KERNEL_BACKEND_JOURNAL_FILE: &str = "kernel-backend.json";

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum KernelBackendPhase {
    CreatingLink,
    LinkCreated,
    Running,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KernelBackendJournal {
    tun_name: String,
    phase: KernelBackendPhase,
}

fn write_kernel_backend_journal(
    tun_name: &str,
    phase: KernelBackendPhase,
) -> Result<(), KernelWireGuardError> {
    let path = kernel_backend_journal_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            operation(format!(
                "failed to create kernel backend journal dir: {error}"
            ))
        })?;
    }
    let journal = KernelBackendJournal {
        tun_name: tun_name.to_string(),
        phase,
    };
    let json = serde_json::to_string(&journal)
        .map_err(|error| operation(format!("failed to encode kernel backend journal: {error}")))?;
    write_atomic(&path, json.as_bytes())
        .map_err(|error| operation(format!("failed to write kernel backend journal: {error}")))
}

fn load_kernel_backend_journal() -> Result<Option<KernelBackendJournal>, KernelWireGuardError> {
    let path = kernel_backend_journal_path();
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(operation(format!(
                "failed to read kernel backend journal: {error}"
            )))
        }
    };
    match serde_json::from_str(&text) {
        Ok(journal) => Ok(Some(journal)),
        Err(error) => {
            quarantine_kernel_backend_journal(&path)?;
            tracing::warn!("quarantined corrupt kernel backend journal: {error}");
            Ok(None)
        }
    }
}

pub(crate) fn clear_kernel_backend_journal() -> Result<(), KernelWireGuardError> {
    let path = kernel_backend_journal_path();
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(operation(format!(
            "failed to clear kernel backend journal: {error}"
        ))),
    }
}

fn kernel_backend_journal_path() -> PathBuf {
    kernel_backend_journal_path_in(env::var_os("STATE_DIRECTORY").as_deref())
}

fn kernel_backend_journal_path_in(state_directory: Option<&std::ffi::OsStr>) -> PathBuf {
    state_directory
        .map(Path::new)
        .unwrap_or_else(|| Path::new("/var/lib/r-wg"))
        .join(KERNEL_BACKEND_JOURNAL_FILE)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp_path = path.with_file_name(format!(
        ".{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(KERNEL_BACKEND_JOURNAL_FILE)
    ));
    {
        let mut file = fs::File::create(&tmp_path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&tmp_path, path)?;
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

fn quarantine_kernel_backend_journal(path: &Path) -> Result<(), KernelWireGuardError> {
    let suffix = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let quarantine_path =
        path.with_file_name(format!("{KERNEL_BACKEND_JOURNAL_FILE}.corrupt.{suffix}"));
    fs::rename(path, quarantine_path).map_err(|error| {
        operation(format!(
            "failed to quarantine kernel backend journal: {error}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtnetlink::packet_core::ErrorMessage;
    use std::num::NonZeroI32;

    #[test]
    fn kernel_link_unavailable_message_matches_capability_failures() {
        for message in [
            "Operation not supported",
            "No such device",
            "Address family not supported by protocol",
            "family not found",
            "wireguard generic netlink unavailable",
        ] {
            assert!(is_kernel_link_unavailable_message(message), "{message}");
        }
    }

    #[test]
    fn kernel_link_unavailable_message_rejects_configuration_failures() {
        for message in [
            "invalid public key",
            "permission denied",
            "failed to configure peer endpoint",
            "invalid allowed ip",
            "feature not supported for this configuration",
        ] {
            assert!(!is_kernel_link_unavailable_message(message), "{message}");
        }
    }

    #[test]
    fn set_device_unavailable_message_only_matches_generic_family_failures() {
        for message in ["family not found", "wireguard generic netlink unavailable"] {
            assert!(
                is_wireguard_family_unavailable_message(message),
                "{message}"
            );
        }
        for message in [
            "Operation not supported",
            "No such device",
            "Address family not supported by protocol",
            "invalid public key",
        ] {
            assert!(
                !is_wireguard_family_unavailable_message(message),
                "{message}"
            );
        }
    }

    #[test]
    fn route_link_not_found_only_matches_enodev() {
        let mut not_found_message = ErrorMessage::default();
        not_found_message.code = NonZeroI32::new(-libc::ENODEV);
        let not_found = rtnetlink::Error::NetlinkError(not_found_message);
        let mut unsupported_message = ErrorMessage::default();
        unsupported_message.code = NonZeroI32::new(-libc::EOPNOTSUPP);
        let unsupported = rtnetlink::Error::NetlinkError(unsupported_message);

        assert!(is_route_link_not_found_error(&not_found));
        assert!(!is_route_link_not_found_error(&unsupported));
    }

    #[test]
    fn kernel_handshake_age_rejects_future_timestamps() {
        let future = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("system clock before UNIX epoch")
            + Duration::from_secs(60);
        assert_eq!(kernel_handshake_age(future), None);
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
