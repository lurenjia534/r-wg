use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::str::FromStr;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use gotatun::device::{
    self, daita, DefaultDeviceTransports, Device, Peer as DevicePeer,
};
use gotatun::x25519::{PublicKey, StaticSecret};
use hyper_util::rt::TokioIo;
use ml_kem::array::typenum::marker_traits::Unsigned;
use ml_kem::kem::Decapsulate;
use ml_kem::{Ciphertext, EncodedSizeUser, KemCore, MlKem1024, MlKem1024Params};
use pqcrypto_hqc::hqc256;
use pqcrypto_traits::kem::{Ciphertext as _, PublicKey as _, SharedSecret as _};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use tokio::net::TcpSocket;
use tokio::time::timeout;
use tonic::transport::{Channel, Endpoint};
use tower::service_fn;
use zeroize::Zeroize;

use crate::core::config::{InterfaceAddress, WireGuardConfig};
#[cfg(target_os = "windows")]
use crate::platform;
use super::relay_inventory::{self, MullvadRelayInventory};

#[expect(clippy::allow_attributes)]
mod proto {
    tonic::include_proto!("ephemeralpeer");
}

const CONFIG_SERVICE_PORT: u16 = 1337;
const CONFIG_SERVICE_GATEWAY: Ipv4Addr = Ipv4Addr::new(10, 64, 0, 1);
const NEGOTIATION_TIMEOUT: Duration = Duration::from_secs(8);
const ML_KEM_ALGORITHM_NAME: &str = "ML-KEM-1024";
const HQC_ALGORITHM_NAME: &str = "HQC-256";
const DAITA_VERSION: u32 = 2;
#[cfg(target_os = "windows")]
const WINDOWS_CONFIG_CLIENT_MTU: u32 = 576;

/// 量子抗性隧道升级模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum QuantumMode {
    #[default]
    Off,
    On,
}

impl QuantumMode {
    pub fn is_enabled(self) -> bool {
        matches!(self, Self::On)
    }
}

/// DAITA 模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DaitaMode {
    #[default]
    Off,
    On,
}

impl DaitaMode {
    pub fn is_enabled(self) -> bool {
        matches!(self, Self::On)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EphemeralFailureKind {
    UnsupportedConfig,
    RelayInventory,
    Connect,
    Rpc,
    Timeout,
    InvalidServerResponse,
    #[cfg(target_os = "windows")]
    WindowsMtuAdjust,
    #[cfg(target_os = "windows")]
    WindowsMtuRestore,
    Reconfigure,
}

pub type DaitaFailureKind = EphemeralFailureKind;

impl fmt::Display for EphemeralFailureKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedConfig => write!(f, "unsupported config"),
            Self::RelayInventory => write!(f, "Mullvad relay inventory lookup failed"),
            Self::Connect => write!(f, "config service unavailable"),
            Self::Rpc => write!(f, "config service rejected request"),
            Self::Timeout => write!(f, "ephemeral peer negotiation timeout"),
            Self::InvalidServerResponse => write!(f, "invalid config service response"),
            #[cfg(target_os = "windows")]
            Self::WindowsMtuAdjust => write!(f, "windows mtu adjustment failed"),
            #[cfg(target_os = "windows")]
            Self::WindowsMtuRestore => write!(f, "windows mtu restore failed"),
            Self::Reconfigure => write!(f, "wireguard reconfigure failed"),
        }
    }
}

#[derive(Debug)]
pub(crate) enum Error {
    UnsupportedConfig(&'static str),
    MissingRelayInventoryCache,
    RelayInventoryCache(relay_inventory::Error),
    Connect(tonic::transport::Error),
    Rpc(Box<tonic::Status>),
    Timeout,
    MissingCiphertexts,
    MissingDaitaResponse,
    MlKemDecapsulationFailed,
    InvalidCiphertextCount {
        actual: usize,
    },
    InvalidCiphertextLength {
        algorithm: &'static str,
        actual: usize,
        expected: usize,
    },
    ParseMaybenotMachines {
        reason: String,
    },
    #[cfg(target_os = "windows")]
    WindowsMtuAdjust(platform::NetworkError),
    #[cfg(target_os = "windows")]
    WindowsMtuRestore(platform::NetworkError),
    Reconfigure(device::Error),
}

impl Error {
    pub(crate) fn kind(&self) -> EphemeralFailureKind {
        match self {
            Self::UnsupportedConfig(_) => EphemeralFailureKind::UnsupportedConfig,
            Self::MissingRelayInventoryCache | Self::RelayInventoryCache(_) => {
                EphemeralFailureKind::RelayInventory
            }
            Self::Connect(_) => EphemeralFailureKind::Connect,
            Self::Rpc(_) => EphemeralFailureKind::Rpc,
            Self::Timeout => EphemeralFailureKind::Timeout,
            Self::MissingCiphertexts
            | Self::MissingDaitaResponse
            | Self::MlKemDecapsulationFailed
            | Self::InvalidCiphertextCount { .. }
            | Self::InvalidCiphertextLength { .. }
            | Self::ParseMaybenotMachines { .. } => EphemeralFailureKind::InvalidServerResponse,
            #[cfg(target_os = "windows")]
            Self::WindowsMtuAdjust(_) => EphemeralFailureKind::WindowsMtuAdjust,
            #[cfg(target_os = "windows")]
            Self::WindowsMtuRestore(_) => EphemeralFailureKind::WindowsMtuRestore,
            Self::Reconfigure(_) => EphemeralFailureKind::Reconfigure,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedConfig(message) => write!(f, "{message}"),
            Self::MissingRelayInventoryCache => {
                write!(
                    f,
                    "no cached Mullvad relay inventory is available for DAITA validation; download DAITA resources in Settings while connected through a regular Mullvad tunnel first"
                )
            }
            Self::RelayInventoryCache(error) => {
                write!(f, "failed to load cached Mullvad relay inventory: {error}")
            }
            Self::Connect(error) => {
                write!(f, "failed to connect to Mullvad config service: {error}")
            }
            Self::Rpc(status) => write!(f, "Mullvad config service RPC failed: {status}"),
            Self::Timeout => write!(
                f,
                "timed out while connecting to Mullvad config service or negotiating ephemeral peer"
            ),
            Self::MissingCiphertexts => write!(f, "Mullvad config service returned no ciphertexts"),
            Self::MissingDaitaResponse => {
                write!(f, "Mullvad config service returned no DAITA configuration")
            }
            Self::MlKemDecapsulationFailed => {
                write!(
                    f,
                    "Mullvad config service returned an ML-KEM ciphertext that failed decapsulation"
                )
            }
            Self::InvalidCiphertextCount { actual } => {
                write!(
                    f,
                    "expected 2 ciphertexts from Mullvad config service, got {actual}"
                )
            }
            Self::InvalidCiphertextLength {
                algorithm,
                actual,
                expected,
            } => write!(
                f,
                "invalid ciphertext length for {algorithm}: expected {expected} bytes, got {actual}"
            ),
            Self::ParseMaybenotMachines { reason } => {
                write!(f, "failed to parse DAITA maybenot machines: {reason}")
            }
            #[cfg(target_os = "windows")]
            Self::WindowsMtuAdjust(error) => {
                write!(
                    f,
                    "failed to lower Windows tunnel MTU before ephemeral negotiation: {error}"
                )
            }
            #[cfg(target_os = "windows")]
            Self::WindowsMtuRestore(error) => {
                write!(
                    f,
                    "failed to restore Windows tunnel MTU after ephemeral negotiation: {error}"
                )
            }
            Self::Reconfigure(error) => write!(f, "failed to hot-reconfigure gotatun: {error}"),
        }
    }
}

impl std::error::Error for Error {}

type RelayConfigService = proto::ephemeral_peer_client::EphemeralPeerClient<Channel>;

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct UpgradeOutcome {
    pub quantum_applied: bool,
    pub daita_applied: bool,
}

struct EphemeralNegotiation {
    psk: Option<[u8; 32]>,
    daita: Option<daita::DaitaSettings>,
}

pub(crate) async fn upgrade_tunnel(
    quantum_mode: QuantumMode,
    daita_mode: DaitaMode,
    device: &Device<DefaultDeviceTransports>,
    tun_name: &str,
    parsed: &WireGuardConfig,
    base_peer: &DevicePeer,
) -> Result<UpgradeOutcome, Error> {
    if !quantum_mode.is_enabled() && !daita_mode.is_enabled() {
        return Ok(UpgradeOutcome::default());
    }

    ensure_supported_ephemeral_config(parsed)?;

    #[cfg(target_os = "windows")]
    let mtu_guard = platform::windows::lower_tunnel_ipv4_mtu(tun_name, WINDOWS_CONFIG_CLIENT_MTU)
        .await
        .map_err(Error::WindowsMtuAdjust)?;

    let upgrade_result = async {
        let parent_private_key = StaticSecret::from(*parsed.interface.private_key.as_bytes());
        let parent_public_key = PublicKey::from(&parent_private_key);
        let ephemeral_private_key = StaticSecret::random_from_rng(OsRng);
        let ephemeral_public_key = PublicKey::from(&ephemeral_private_key);

        tracing::debug!(
            tun = tun_name,
            quantum = quantum_mode.is_enabled(),
            daita = daita_mode.is_enabled(),
            "negotiating Mullvad ephemeral peer"
        );

        let mut negotiation = negotiate_ephemeral_peer(
            parent_public_key,
            ephemeral_public_key,
            quantum_mode.is_enabled(),
            daita_mode.is_enabled(),
        )
        .await?;

        let mut peer = base_peer.clone();
        if let Some(psk) = negotiation.psk.take() {
            peer.preshared_key = Some(psk);
        }
        if let Some(daita) = negotiation.daita.take() {
            peer = peer.with_daita(daita);
        }

        let config_result = device
            .write(async |device| {
                device.set_private_key(ephemeral_private_key.clone()).await;
                device.clear_peers();
                device.add_peers(vec![peer]);
                Ok::<_, device::Error>(())
            })
            .await
            .and_then(|result| result)
            .map_err(Error::Reconfigure);

        config_result?;
        tracing::debug!(tun = tun_name, "Mullvad ephemeral peer configuration applied");
        Ok(UpgradeOutcome {
            quantum_applied: quantum_mode.is_enabled(),
            daita_applied: daita_mode.is_enabled(),
        })
    }
    .await;

    #[cfg(target_os = "windows")]
    let restore_result = mtu_guard.restore().map_err(Error::WindowsMtuRestore);

    #[cfg(target_os = "windows")]
    {
        match (upgrade_result, restore_result) {
            (Err(error), _) => Err(error),
            (Ok(()), Err(error)) => Err(error),
            (Ok(outcome), Ok(())) => Ok(outcome),
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        upgrade_result
    }
}

fn ensure_supported_ephemeral_config(parsed: &WireGuardConfig) -> Result<(), Error> {
    if parsed.peers.len() != 1 {
        return Err(Error::UnsupportedConfig(
            "ephemeral peer negotiation currently supports only Mullvad single-hop configs",
        ));
    }

    if !parsed
        .interface
        .addresses
        .iter()
        .any(is_mullvad_tunnel_address)
    {
        return Err(Error::UnsupportedConfig(
            "ephemeral peer negotiation currently supports only Mullvad tunnel addresses",
        ));
    }

    Ok(())
}

fn is_mullvad_tunnel_address(address: &InterfaceAddress) -> bool {
    match address.addr {
        IpAddr::V4(ipv4) => {
            let octets = ipv4.octets();
            octets[0] == 10 && (64..128).contains(&octets[1])
        }
        IpAddr::V6(_) => false,
    }
}

pub(crate) fn validate_daita_config(parsed: &WireGuardConfig) -> Result<(), Error> {
    let inventory = relay_inventory::load_cached_inventory()
        .map_err(Error::RelayInventoryCache)?
        .ok_or(Error::MissingRelayInventoryCache)?;
    validate_daita_config_against_inventory(parsed, &inventory)
}

fn validate_daita_config_against_inventory(
    parsed: &WireGuardConfig,
    inventory: &MullvadRelayInventory,
) -> Result<(), Error> {
    let peer = parsed.peers.first().ok_or(Error::UnsupportedConfig(
        "DAITA startup validation requires exactly one configured Mullvad peer",
    ))?;

    peer.endpoint.as_ref().ok_or(Error::UnsupportedConfig(
        "DAITA startup validation requires a configured Mullvad relay endpoint",
    ))?;

    let relay = inventory
        .find_by_public_key(&public_key_inventory_token(peer))
        .ok_or(Error::UnsupportedConfig(
            "DAITA startup validation could not match the selected peer public key to the current Mullvad relay inventory",
        ))?;

    if !relay.daita {
        return Err(Error::UnsupportedConfig(
            "selected Mullvad relay does not advertise DAITA capability",
        ));
    }

    tracing::debug!(
        relay = relay.hostname,
        "validated DAITA-capable Mullvad relay from relay inventory"
    );

    Ok(())
}

fn public_key_inventory_token(peer: &crate::core::config::PeerConfig) -> String {
    STANDARD.encode(peer.public_key.as_bytes())
}

async fn negotiate_ephemeral_peer(
    parent_public_key: PublicKey,
    ephemeral_public_key: PublicKey,
    enable_post_quantum: bool,
    enable_daita: bool,
) -> Result<EphemeralNegotiation, Error> {
    timeout(
        NEGOTIATION_TIMEOUT,
        negotiate_ephemeral_peer_inner(
            parent_public_key,
            ephemeral_public_key,
            enable_post_quantum,
            enable_daita,
        ),
    )
    .await
    .map_err(|_| Error::Timeout)?
}

async fn negotiate_ephemeral_peer_inner(
    parent_public_key: PublicKey,
    ephemeral_public_key: PublicKey,
    enable_post_quantum: bool,
    enable_daita: bool,
) -> Result<EphemeralNegotiation, Error> {
    let (pq_request, kem_keypairs) = if enable_post_quantum {
        let (request, ml_kem_keypair, hqc_keypair) = post_quantum_request();
        (Some(request), Some((ml_kem_keypair, hqc_keypair)))
    } else {
        (None, None)
    };
    let mut client = connect_relay_config_client(CONFIG_SERVICE_GATEWAY).await?;

    let response = client
        .register_peer_v1(proto::EphemeralPeerRequestV1 {
            wg_parent_pubkey: parent_public_key.as_bytes().to_vec(),
            wg_ephemeral_peer_pubkey: ephemeral_public_key.as_bytes().to_vec(),
            post_quantum: pq_request,
            daita: None,
            daita_v2: enable_daita.then(build_daita_request),
        })
        .await
        .map_err(|status| Error::Rpc(Box::new(status)))?
        .into_inner();

    let psk = if let Some((ml_kem_keypair, hqc_keypair)) = kem_keypairs {
        let ciphertexts = response
            .post_quantum
            .ok_or(Error::MissingCiphertexts)?
            .ciphertexts;

        let [ml_kem_ciphertext, hqc_ciphertext] =
            <&[Vec<u8>; 2]>::try_from(ciphertexts.as_slice()).map_err(|_| {
                Error::InvalidCiphertextCount {
                    actual: ciphertexts.len(),
                }
            })?;

        let mut psk = [0u8; 32];

        {
            let mut secret = ml_kem_keypair.decapsulate(ml_kem_ciphertext)?;
            xor_assign(&mut psk, &secret);
            secret.zeroize();
        }

        {
            let mut secret = hqc_keypair.decapsulate(hqc_ciphertext)?;
            xor_assign(&mut psk, &secret);
            secret.zeroize();
        }

        Some(psk)
    } else {
        None
    };

    let daita = response.daita.map(parse_daita_response).transpose()?;
    if daita.is_none() && enable_daita {
        return Err(Error::MissingDaitaResponse);
    }

    Ok(EphemeralNegotiation { psk, daita })
}

fn build_daita_request() -> proto::DaitaRequestV2 {
    proto::DaitaRequestV2 {
        version: DAITA_VERSION,
        platform: i32::from(daita_platform()),
        level: i32::from(proto::DaitaLevel::LevelDefault),
    }
}

const fn daita_platform() -> proto::DaitaPlatform {
    use proto::DaitaPlatform;

    #[cfg(target_os = "windows")]
    {
        DaitaPlatform::WindowsWgGo
    }
    #[cfg(target_os = "linux")]
    {
        DaitaPlatform::LinuxWgGo
    }
    #[cfg(target_os = "macos")]
    {
        DaitaPlatform::MacosWgGo
    }
    #[cfg(target_os = "android")]
    {
        DaitaPlatform::AndroidWgGo
    }
    #[cfg(target_os = "ios")]
    {
        DaitaPlatform::IosWgGo
    }
}

fn parse_daita_response(response: proto::DaitaResponseV2) -> Result<daita::DaitaSettings, Error> {
    let maybenot_machines = response
        .client_machines
        .into_iter()
        .map(|machine| daita::Machine::from_str(&machine))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            let reason = match error {
                daita::Error::Machine(reason) => reason,
                daita::Error::PaddingLimit | daita::Error::BlockingLimit => {
                    "unknown reason".to_string()
                }
            };
            Error::ParseMaybenotMachines { reason }
        })?;

    Ok(daita::DaitaSettings {
        maybenot_machines,
        max_decoy_frac: response.max_padding_frac,
        max_delay_frac: response.max_blocking_frac,
        ..Default::default()
    })
}

fn post_quantum_request() -> (proto::PostQuantumRequestV1, MlKemKeypair, HqcKeypair) {
    let ml_kem_keypair = MlKemKeypair::generate();
    let hqc_keypair = HqcKeypair::generate();

    (
        proto::PostQuantumRequestV1 {
            kem_pubkeys: vec![
                proto::KemPubkeyV1 {
                    algorithm_name: ML_KEM_ALGORITHM_NAME.to_string(),
                    key_data: ml_kem_keypair.encapsulation_key(),
                },
                proto::KemPubkeyV1 {
                    algorithm_name: HQC_ALGORITHM_NAME.to_string(),
                    key_data: hqc_keypair.encapsulation_key(),
                },
            ],
        },
        ml_kem_keypair,
        hqc_keypair,
    )
}

async fn connect_relay_config_client(ip: Ipv4Addr) -> Result<RelayConfigService, Error> {
    let endpoint = Endpoint::from_static("tcp://0.0.0.0:0");
    let addr = SocketAddr::new(IpAddr::V4(ip), CONFIG_SERVICE_PORT);

    let connection = endpoint
        .connect_with_connector(service_fn(move |_| async move {
            let socket = mss_socket()?;
            let stream = socket.connect(addr).await?;
            Ok::<_, std::io::Error>(TokioIo::new(stream))
        }))
        .await
        .map_err(Error::Connect)?;

    Ok(RelayConfigService::new(connection))
}

fn mss_socket() -> Result<TcpSocket, std::io::Error> {
    let socket = TcpSocket::new_v4()?;
    #[cfg(unix)]
    {
        use nix::sys::socket::{setsockopt, sockopt::TcpMaxSeg};
        use std::os::fd::AsFd;

        const CONFIG_CLIENT_MTU: u16 = 576;
        const IPV4_HEADER_SIZE: u16 = 20;
        const MAX_TCP_HEADER_SIZE: u16 = 60;

        let mtu = CONFIG_CLIENT_MTU.saturating_sub(IPV4_HEADER_SIZE);
        let mss = u32::from(mtu.saturating_sub(MAX_TCP_HEADER_SIZE));
        let _ = setsockopt(&socket.as_fd(), TcpMaxSeg, &mss);
    }
    Ok(socket)
}

fn xor_assign(dst: &mut [u8; 32], src: &[u8; 32]) {
    for (dst_byte, src_byte) in dst.iter_mut().zip(src.iter()) {
        *dst_byte ^= src_byte;
    }
}

struct MlKemKeypair {
    encapsulation_key: ml_kem::kem::EncapsulationKey<MlKem1024Params>,
    decapsulation_key: ml_kem::kem::DecapsulationKey<MlKem1024Params>,
}

impl MlKemKeypair {
    fn generate() -> Self {
        let (decapsulation_key, encapsulation_key) = MlKem1024::generate(&mut rand::thread_rng());
        Self {
            encapsulation_key,
            decapsulation_key,
        }
    }

    fn encapsulation_key(&self) -> Vec<u8> {
        self.encapsulation_key.as_bytes().as_slice().to_vec()
    }

    fn decapsulate(&self, ciphertext_slice: &[u8]) -> Result<[u8; 32], Error> {
        let ciphertext_array =
            <Ciphertext<MlKem1024>>::try_from(ciphertext_slice).map_err(|_| {
                Error::InvalidCiphertextLength {
                    algorithm: ML_KEM_ALGORITHM_NAME,
                    actual: ciphertext_slice.len(),
                    expected: <MlKem1024 as KemCore>::CiphertextSize::USIZE,
                }
            })?;
        let shared_secret = self
            .decapsulation_key
            .decapsulate(&ciphertext_array)
            .map_err(|_| Error::MlKemDecapsulationFailed)?;
        Ok(shared_secret.0)
    }
}

struct HqcKeypair {
    public_key: hqc256::PublicKey,
    secret_key: hqc256::SecretKey,
}

impl HqcKeypair {
    fn generate() -> Self {
        let (public_key, secret_key) = hqc256::keypair();
        Self {
            public_key,
            secret_key,
        }
    }

    fn encapsulation_key(&self) -> Vec<u8> {
        self.public_key.as_bytes().to_vec()
    }

    fn decapsulate(&self, ciphertext_slice: &[u8]) -> Result<[u8; 32], Error> {
        let ciphertext = hqc256::Ciphertext::from_bytes(ciphertext_slice).map_err(|_| {
            Error::InvalidCiphertextLength {
                algorithm: HQC_ALGORITHM_NAME,
                actual: ciphertext_slice.len(),
                expected: hqc256::ciphertext_bytes(),
            }
        })?;
        let shared_secret = hqc256::decapsulate(&ciphertext, &self.secret_key);
        Ok(Sha256::digest(shared_secret.as_bytes()).into())
    }
}

#[cfg(test)]
mod tests {
    use base64::Engine as _;

    use crate::backend::wg::relay_inventory::inventory_from_json;
    use crate::core::config::parse_config;

    use super::{
        ensure_supported_ephemeral_config, public_key_inventory_token,
        validate_daita_config_against_inventory,
    };
    use super::STANDARD;

    const PRIVATE_KEY: &str = "0000000000000000000000000000000000000000000000000000000000000000";
    const PUBLIC_KEY_A: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const PUBLIC_KEY_B: &str = "2222222222222222222222222222222222222222222222222222222222222222";

    #[test]
    fn accepts_singlehop_mullvad_tunnel_address() {
        let config = parse_config(&format!(
            "[Interface]\nPrivateKey = {PRIVATE_KEY}\nAddress = 10.64.12.34/32\n\n[Peer]\nPublicKey = {PUBLIC_KEY_A}\nAllowedIPs = 0.0.0.0/0\nEndpoint = 203.0.113.10:51820\n"
        ))
        .expect("config should parse");

        ensure_supported_ephemeral_config(&config).expect("config should be accepted");
    }

    #[test]
    fn rejects_non_mullvad_address_space() {
        let config = parse_config(&format!(
            "[Interface]\nPrivateKey = {PRIVATE_KEY}\nAddress = 10.0.0.2/32\n\n[Peer]\nPublicKey = {PUBLIC_KEY_A}\nAllowedIPs = 0.0.0.0/0\nEndpoint = 203.0.113.10:51820\n"
        ))
        .expect("config should parse");

        let error = ensure_supported_ephemeral_config(&config)
            .expect_err("config should be rejected");
        assert!(error
            .to_string()
            .contains("currently supports only Mullvad tunnel addresses"));
    }

    #[test]
    fn rejects_multipeer_configs() {
        let config = parse_config(&format!(
            "[Interface]\nPrivateKey = {PRIVATE_KEY}\nAddress = 10.64.12.34/32\n\n[Peer]\nPublicKey = {PUBLIC_KEY_A}\nAllowedIPs = 0.0.0.0/0\nEndpoint = 203.0.113.10:51820\n\n[Peer]\nPublicKey = {PUBLIC_KEY_B}\nAllowedIPs = ::/0\nEndpoint = [2001:db8::1]:51820\n"
        ))
        .expect("config should parse");

        let error = ensure_supported_ephemeral_config(&config)
            .expect_err("config should be rejected");
        assert!(error
            .to_string()
            .contains("currently supports only Mullvad single-hop configs"));
    }

    #[test]
    fn accepts_recognized_mullvad_relay_hostname_for_daita() {
        let config = parse_config(&format!(
            "[Interface]\nPrivateKey = {PRIVATE_KEY}\nAddress = 10.64.12.34/32\n\n[Peer]\nPublicKey = {PUBLIC_KEY_A}\nAllowedIPs = 0.0.0.0/0\nEndpoint = se-sto-wg-001.relays.mullvad.net:51820\n"
        ))
        .expect("config should parse");

        let relay_inventory = inventory_from_json(&format!(
            r#"{{
                "wireguard": {{
                    "relays": [
                        {{
                            "hostname": "se-sto-wg-001",
                            "public_key": "{}",
                            "daita": true
                        }}
                    ]
                }}
            }}"#,
            public_key_inventory_token(config.peers.first().unwrap())
        ))
        .expect("inventory should parse");

        validate_daita_config_against_inventory(&config, &relay_inventory)
            .expect("config should be accepted");
    }

    #[test]
    fn accepts_ip_literal_endpoints_when_peer_key_matches_daita_relay() {
        let config = parse_config(&format!(
            "[Interface]\nPrivateKey = {PRIVATE_KEY}\nAddress = 10.64.12.34/32\n\n[Peer]\nPublicKey = {PUBLIC_KEY_A}\nAllowedIPs = 0.0.0.0/0\nEndpoint = 203.0.113.10:51820\n"
        ))
        .expect("config should parse");

        let relay_inventory = inventory_from_json(&format!(
            r#"{{
                "wireguard": {{
                    "relays": [
                        {{
                            "hostname": "se-sto-wg-001",
                            "public_key": "{}",
                            "features": {{ "daita": {{}} }}
                        }}
                    ]
                }}
            }}"#,
            public_key_inventory_token(config.peers.first().unwrap())
        ))
        .expect("inventory should parse");

        validate_daita_config_against_inventory(&config, &relay_inventory)
            .expect("IP literal endpoint should still be accepted");
    }

    #[test]
    fn rejects_non_daita_relay_from_inventory() {
        let config = parse_config(&format!(
            "[Interface]\nPrivateKey = {PRIVATE_KEY}\nAddress = 10.64.12.34/32\n\n[Peer]\nPublicKey = {PUBLIC_KEY_A}\nAllowedIPs = 0.0.0.0/0\nEndpoint = 203.0.113.10:51820\n"
        ))
        .expect("config should parse");

        let relay_inventory = inventory_from_json(&format!(
            r#"{{
                "wireguard": {{
                    "relays": [
                        {{
                            "hostname": "se-sto-wg-001",
                            "public_key": "{}"
                        }}
                    ]
                }}
            }}"#,
            public_key_inventory_token(config.peers.first().unwrap())
        ))
        .expect("inventory should parse");

        let error = validate_daita_config_against_inventory(&config, &relay_inventory)
            .expect_err("config should be rejected");
        assert!(error
            .to_string()
            .contains("does not advertise DAITA capability"));
    }

    #[test]
    fn rejects_unknown_peer_public_key_for_daita() {
        let config = parse_config(&format!(
            "[Interface]\nPrivateKey = {PRIVATE_KEY}\nAddress = 10.64.12.34/32\n\n[Peer]\nPublicKey = {PUBLIC_KEY_A}\nAllowedIPs = 0.0.0.0/0\nEndpoint = 203.0.113.10:51820\n"
        ))
        .expect("config should parse");

        let relay_inventory = inventory_from_json(&format!(
            r#"{{
                "wireguard": {{
                    "relays": [
                        {{
                            "hostname": "se-sto-wg-001",
                            "public_key": "{}",
                            "daita": true
                        }}
                    ]
                }}
            }}"#,
            STANDARD.encode([0x33; 32])
        ))
        .expect("inventory should parse");

        let error = validate_daita_config_against_inventory(&config, &relay_inventory)
            .expect_err("config should be rejected");
        assert!(error
            .to_string()
            .contains("could not match the selected peer public key"));
    }

    #[test]
    fn parses_daita_from_legacy_and_feature_flags() {
        let relay_inventory = inventory_from_json(
            r#"{
                "wireguard": {
                    "relays": [
                        {
                            "hostname": "legacy-daita",
                            "public_key": "legacy",
                            "daita": true
                        },
                        {
                            "hostname": "feature-daita",
                            "public_key": "feature",
                            "features": { "daita": {} }
                        },
                        {
                            "hostname": "plain-relay",
                            "public_key": "plain"
                        }
                    ]
                }
            }"#,
        )
        .expect("inventory should parse");

        let legacy = relay_inventory
            .find_by_public_key("legacy")
            .expect("legacy relay should exist");
        let feature = relay_inventory
            .find_by_public_key("feature")
            .expect("feature relay should exist");
        let plain = relay_inventory
            .find_by_public_key("plain")
            .expect("plain relay should exist");

        assert!(legacy.daita);
        assert!(feature.daita);
        assert!(!plain.daita);
    }
}
