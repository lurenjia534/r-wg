use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use gotatun::device::{self, DefaultDeviceTransports, Device, Peer as DevicePeer};
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

#[expect(clippy::allow_attributes)]
mod proto {
    tonic::include_proto!("ephemeralpeer");
}

const CONFIG_SERVICE_PORT: u16 = 1337;
const CONFIG_SERVICE_GATEWAY: Ipv4Addr = Ipv4Addr::new(10, 64, 0, 1);
const NEGOTIATION_TIMEOUT: Duration = Duration::from_secs(8);
const ML_KEM_ALGORITHM_NAME: &str = "ML-KEM-1024";
const HQC_ALGORITHM_NAME: &str = "HQC-256";
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuantumFailureKind {
    UnsupportedConfig,
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

impl fmt::Display for QuantumFailureKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedConfig => write!(f, "unsupported config"),
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
    Connect(tonic::transport::Error),
    Rpc(Box<tonic::Status>),
    Timeout,
    MissingCiphertexts,
    InvalidCiphertextCount {
        actual: usize,
    },
    InvalidCiphertextLength {
        algorithm: &'static str,
        actual: usize,
        expected: usize,
    },
    #[cfg(target_os = "windows")]
    WindowsMtuAdjust(platform::NetworkError),
    #[cfg(target_os = "windows")]
    WindowsMtuRestore(platform::NetworkError),
    Reconfigure(device::Error),
}

impl Error {
    pub(crate) fn kind(&self) -> QuantumFailureKind {
        match self {
            Self::UnsupportedConfig(_) => QuantumFailureKind::UnsupportedConfig,
            Self::Connect(_) => QuantumFailureKind::Connect,
            Self::Rpc(_) => QuantumFailureKind::Rpc,
            Self::Timeout => QuantumFailureKind::Timeout,
            Self::MissingCiphertexts
            | Self::InvalidCiphertextCount { .. }
            | Self::InvalidCiphertextLength { .. } => QuantumFailureKind::InvalidServerResponse,
            #[cfg(target_os = "windows")]
            Self::WindowsMtuAdjust(_) => QuantumFailureKind::WindowsMtuAdjust,
            #[cfg(target_os = "windows")]
            Self::WindowsMtuRestore(_) => QuantumFailureKind::WindowsMtuRestore,
            Self::Reconfigure(_) => QuantumFailureKind::Reconfigure,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedConfig(message) => write!(f, "{message}"),
            Self::Connect(error) => {
                write!(f, "failed to connect to Mullvad config service: {error}")
            }
            Self::Rpc(status) => write!(f, "Mullvad config service RPC failed: {status}"),
            Self::Timeout => write!(
                f,
                "timed out while connecting to Mullvad config service or negotiating ephemeral peer"
            ),
            Self::MissingCiphertexts => write!(f, "Mullvad config service returned no ciphertexts"),
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
            #[cfg(target_os = "windows")]
            Self::WindowsMtuAdjust(error) => {
                write!(
                    f,
                    "failed to lower Windows tunnel MTU before quantum upgrade: {error}"
                )
            }
            #[cfg(target_os = "windows")]
            Self::WindowsMtuRestore(error) => {
                write!(
                    f,
                    "failed to restore Windows tunnel MTU after quantum upgrade: {error}"
                )
            }
            Self::Reconfigure(error) => write!(f, "failed to hot-reconfigure gotatun: {error}"),
        }
    }
}

impl std::error::Error for Error {}

type RelayConfigService = proto::ephemeral_peer_client::EphemeralPeerClient<Channel>;

pub(crate) async fn upgrade_tunnel(
    mode: QuantumMode,
    device: &Device<DefaultDeviceTransports>,
    tun_name: &str,
    parsed: &WireGuardConfig,
    base_peer: &DevicePeer,
) -> Result<(), Error> {
    if matches!(mode, QuantumMode::Off) {
        return Ok(());
    }

    ensure_supported_mullvad_config(parsed)?;

    #[cfg(target_os = "windows")]
    let mtu_guard = platform::windows::lower_tunnel_ipv4_mtu(tun_name, WINDOWS_CONFIG_CLIENT_MTU)
        .await
        .map_err(Error::WindowsMtuAdjust)?;

    let upgrade_result = async {
        let parent_private_key = StaticSecret::from(*parsed.interface.private_key.as_bytes());
        let parent_public_key = PublicKey::from(&parent_private_key);
        let ephemeral_private_key = StaticSecret::random_from_rng(OsRng);
        let ephemeral_public_key = PublicKey::from(&ephemeral_private_key);

        tracing::debug!(tun = tun_name, "negotiating Mullvad ephemeral peer");

        let mut psk = negotiate_preshared_key(parent_public_key, ephemeral_public_key).await?;

        let mut peer = base_peer.clone();
        peer.preshared_key = Some(psk);

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

        psk.zeroize();
        config_result?;
        tracing::debug!(tun = tun_name, "Mullvad quantum tunnel upgrade applied");
        Ok(())
    }
    .await;

    #[cfg(target_os = "windows")]
    let restore_result = mtu_guard.restore().map_err(Error::WindowsMtuRestore);

    #[cfg(target_os = "windows")]
    {
        match (upgrade_result, restore_result) {
            (Err(error), _) => Err(error),
            (Ok(()), Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        upgrade_result
    }
}

fn ensure_supported_mullvad_config(parsed: &WireGuardConfig) -> Result<(), Error> {
    if parsed.peers.len() != 1 {
        return Err(Error::UnsupportedConfig(
            "quantum-resistant upgrade currently supports only Mullvad single-hop configs",
        ));
    }

    if !parsed
        .interface
        .addresses
        .iter()
        .any(is_mullvad_tunnel_address)
    {
        return Err(Error::UnsupportedConfig(
            "quantum-resistant upgrade currently supports only Mullvad tunnel addresses",
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

async fn negotiate_preshared_key(
    parent_public_key: PublicKey,
    ephemeral_public_key: PublicKey,
) -> Result<[u8; 32], Error> {
    timeout(
        NEGOTIATION_TIMEOUT,
        negotiate_preshared_key_inner(parent_public_key, ephemeral_public_key),
    )
    .await
    .map_err(|_| Error::Timeout)?
}

async fn negotiate_preshared_key_inner(
    parent_public_key: PublicKey,
    ephemeral_public_key: PublicKey,
) -> Result<[u8; 32], Error> {
    let (request, ml_kem_keypair, hqc_keypair) = post_quantum_request();
    let mut client = connect_relay_config_client(CONFIG_SERVICE_GATEWAY).await?;

    let response = client
        .register_peer_v1(proto::EphemeralPeerRequestV1 {
            wg_parent_pubkey: parent_public_key.as_bytes().to_vec(),
            wg_ephemeral_peer_pubkey: ephemeral_public_key.as_bytes().to_vec(),
            post_quantum: Some(request),
            daita: None,
            daita_v2: None,
        })
        .await
        .map_err(|status| Error::Rpc(Box::new(status)))?
        .into_inner();

    let ciphertexts = response
        .post_quantum
        .ok_or(Error::MissingCiphertexts)?
        .ciphertexts;

    let [ml_kem_ciphertext, hqc_ciphertext] = <&[Vec<u8>; 2]>::try_from(ciphertexts.as_slice())
        .map_err(|_| Error::InvalidCiphertextCount {
            actual: ciphertexts.len(),
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

    Ok(psk)
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
            .unwrap();
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
    use crate::core::config::parse_config;

    use super::ensure_supported_mullvad_config;

    const PRIVATE_KEY: &str = "0000000000000000000000000000000000000000000000000000000000000000";
    const PUBLIC_KEY_A: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const PUBLIC_KEY_B: &str = "2222222222222222222222222222222222222222222222222222222222222222";

    #[test]
    fn accepts_singlehop_mullvad_tunnel_address() {
        let config = parse_config(&format!(
            "[Interface]\nPrivateKey = {PRIVATE_KEY}\nAddress = 10.64.12.34/32\n\n[Peer]\nPublicKey = {PUBLIC_KEY_A}\nAllowedIPs = 0.0.0.0/0\nEndpoint = 203.0.113.10:51820\n"
        ))
        .expect("config should parse");

        ensure_supported_mullvad_config(&config).expect("config should be accepted");
    }

    #[test]
    fn rejects_non_mullvad_address_space() {
        let config = parse_config(&format!(
            "[Interface]\nPrivateKey = {PRIVATE_KEY}\nAddress = 10.0.0.2/32\n\n[Peer]\nPublicKey = {PUBLIC_KEY_A}\nAllowedIPs = 0.0.0.0/0\nEndpoint = 203.0.113.10:51820\n"
        ))
        .expect("config should parse");

        let error =
            ensure_supported_mullvad_config(&config).expect_err("config should be rejected");
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

        let error =
            ensure_supported_mullvad_config(&config).expect_err("config should be rejected");
        assert!(error
            .to_string()
            .contains("currently supports only Mullvad single-hop configs"));
    }
}
