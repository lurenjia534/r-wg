use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use http_body_util::{BodyExt as _, Empty};
use hyper::body::Bytes;
use hyper::{Request, StatusCode};
use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use serde::{Deserialize, Serialize};
use tokio::time::timeout;

#[cfg(not(target_os = "linux"))]
use std::env;

const RELAY_INVENTORY_TIMEOUT: Duration = Duration::from_secs(15);
const RELAY_INVENTORY_URL: &str = "https://api.mullvad.net/app/v1/relays";
const RELAY_INVENTORY_FILE_NAME: &str = "mullvad-relays.json";

#[derive(Debug)]
pub(crate) enum Error {
    CreateCacheDir(String),
    ReadCache(String),
    WriteCache(String),
    ParseCache(String),
    FetchTimeout,
    FetchTransport(String),
    FetchStatus(StatusCode),
    ParseRemote(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CreateCacheDir(reason) => write!(
                f,
                "failed to create the DAITA relay inventory cache directory: {reason}"
            ),
            Self::ReadCache(reason) => {
                write!(f, "failed to read the cached DAITA relay inventory: {reason}")
            }
            Self::WriteCache(reason) => {
                write!(f, "failed to write the cached DAITA relay inventory: {reason}")
            }
            Self::ParseCache(reason) => {
                write!(f, "failed to parse the cached DAITA relay inventory: {reason}")
            }
            Self::FetchTimeout => write!(
                f,
                "timed out while fetching Mullvad relay inventory from the network"
            ),
            Self::FetchTransport(reason) => {
                write!(f, "failed to fetch Mullvad relay inventory: {reason}")
            }
            Self::FetchStatus(status) => write!(
                f,
                "Mullvad relay inventory returned unexpected HTTP status {status}"
            ),
            Self::ParseRemote(reason) => {
                write!(f, "failed to parse Mullvad relay inventory response: {reason}")
            }
        }
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedRelayInventoryFile {
    fetched_at_unix_secs: u64,
    inventory: MullvadRelayInventory,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct MullvadRelayInventory {
    relays: Vec<MullvadRelayMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MullvadRelayMetadata {
    pub(crate) hostname: String,
    pub(crate) public_key: String,
    pub(crate) daita: bool,
}

impl MullvadRelayInventory {
    pub(crate) fn find_by_public_key(&self, public_key: &str) -> Option<&MullvadRelayMetadata> {
        self.relays
            .iter()
            .find(|relay| relay.public_key == public_key)
    }

    fn relay_count(&self) -> usize {
        self.relays.len()
    }

    fn daita_relay_count(&self) -> usize {
        self.relays.iter().filter(|relay| relay.daita).count()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayInventoryStatusSnapshot {
    pub cache_path: String,
    pub present: bool,
    pub relay_count: usize,
    pub daita_relay_count: usize,
    pub fetched_at_unix_secs: Option<u64>,
}

impl RelayInventoryStatusSnapshot {
    fn missing(cache_path: PathBuf) -> Self {
        Self {
            cache_path: cache_path.display().to_string(),
            present: false,
            relay_count: 0,
            daita_relay_count: 0,
            fetched_at_unix_secs: None,
        }
    }

    fn from_cached(cache_path: PathBuf, cached: &CachedRelayInventoryFile) -> Self {
        Self {
            cache_path: cache_path.display().to_string(),
            present: true,
            relay_count: cached.inventory.relay_count(),
            daita_relay_count: cached.inventory.daita_relay_count(),
            fetched_at_unix_secs: Some(cached.fetched_at_unix_secs),
        }
    }
}

#[derive(Deserialize)]
struct RelayInventoryResponse {
    wireguard: RelayInventoryWireguard,
}

#[derive(Deserialize)]
struct RelayInventoryWireguard {
    relays: Vec<RelayInventoryWireguardRelay>,
}

#[derive(Default, Deserialize)]
struct RelayInventoryFeatures {
    daita: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct RelayInventoryWireguardRelay {
    hostname: String,
    public_key: String,
    #[serde(default)]
    daita: bool,
    #[serde(default)]
    features: RelayInventoryFeatures,
}

impl From<RelayInventoryResponse> for MullvadRelayInventory {
    fn from(response: RelayInventoryResponse) -> Self {
        let relays = response
            .wireguard
            .relays
            .into_iter()
            .map(|relay| MullvadRelayMetadata {
                hostname: relay.hostname.to_ascii_lowercase(),
                public_key: relay.public_key,
                daita: relay.features.daita.is_some() || relay.daita,
            })
            .collect();

        Self { relays }
    }
}

pub(crate) fn load_cached_inventory() -> Result<Option<MullvadRelayInventory>, Error> {
    let cache_path = relay_inventory_cache_path();
    let Some(cached) = read_cached_inventory_file(cache_path.as_path())? else {
        return Ok(None);
    };
    Ok(Some(cached.inventory))
}

pub(crate) fn status_snapshot() -> Result<RelayInventoryStatusSnapshot, Error> {
    let cache_path = relay_inventory_cache_path();
    let Some(cached) = read_cached_inventory_file(cache_path.as_path())? else {
        return Ok(RelayInventoryStatusSnapshot::missing(cache_path));
    };
    Ok(RelayInventoryStatusSnapshot::from_cached(cache_path, &cached))
}

pub(crate) async fn refresh_cache() -> Result<RelayInventoryStatusSnapshot, Error> {
    let inventory = fetch_remote_inventory().await?;
    let cache_path = relay_inventory_cache_path();
    let fetched_at_unix_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let cached = CachedRelayInventoryFile {
        fetched_at_unix_secs,
        inventory,
    };
    write_cached_inventory_file(cache_path.as_path(), &cached)?;
    Ok(RelayInventoryStatusSnapshot::from_cached(cache_path, &cached))
}

#[cfg(test)]
pub(crate) fn inventory_from_json(json: &str) -> Result<MullvadRelayInventory, Error> {
    let response: RelayInventoryResponse =
        serde_json::from_str(json).map_err(|error| Error::ParseRemote(error.to_string()))?;
    Ok(response.into())
}

async fn fetch_remote_inventory() -> Result<MullvadRelayInventory, Error> {
    let https = HttpsConnectorBuilder::new()
        .with_native_roots()
        .map_err(|error| Error::FetchTransport(error.to_string()))?
        .https_only()
        .enable_http1()
        .build();
    let client = Client::builder(TokioExecutor::new()).build::<_, Empty<Bytes>>(https);
    let request = Request::get(RELAY_INVENTORY_URL)
        .body(Empty::<Bytes>::new())
        .expect("static relay inventory request must be valid");

    let response = timeout(RELAY_INVENTORY_TIMEOUT, client.request(request))
        .await
        .map_err(|_| Error::FetchTimeout)?
        .map_err(|error| Error::FetchTransport(error.to_string()))?;

    if response.status() != StatusCode::OK {
        return Err(Error::FetchStatus(response.status()));
    }

    let body = response
        .into_body()
        .collect()
        .await
        .map_err(|error| Error::FetchTransport(error.to_string()))?
        .to_bytes();

    let response: RelayInventoryResponse =
        serde_json::from_slice(&body).map_err(|error| Error::ParseRemote(error.to_string()))?;

    Ok(response.into())
}

fn read_cached_inventory_file(path: &Path) -> Result<Option<CachedRelayInventoryFile>, Error> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(Error::ReadCache(error.to_string())),
    };
    let cached = serde_json::from_str(&text).map_err(|error| Error::ParseCache(error.to_string()))?;
    Ok(Some(cached))
}

fn write_cached_inventory_file(
    path: &Path,
    cached: &CachedRelayInventoryFile,
) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| Error::CreateCacheDir(error.to_string()))?;
    }
    let payload =
        serde_json::to_vec_pretty(cached).map_err(|error| Error::WriteCache(error.to_string()))?;
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, payload).map_err(|error| Error::WriteCache(error.to_string()))?;
    if let Err(error) = fs::rename(&tmp_path, path) {
        if path.exists() {
            fs::remove_file(path).map_err(|remove_error| {
                Error::WriteCache(format!(
                    "failed to replace existing cache file after rename failure: {remove_error}"
                ))
            })?;
            fs::rename(&tmp_path, path)
                .map_err(|rename_error| Error::WriteCache(rename_error.to_string()))?;
        } else {
            return Err(Error::WriteCache(error.to_string()));
        }
    }
    Ok(())
}

fn relay_inventory_cache_path() -> PathBuf {
    relay_inventory_cache_dir().join(RELAY_INVENTORY_FILE_NAME)
}

#[cfg(target_os = "linux")]
fn relay_inventory_cache_dir() -> PathBuf {
    PathBuf::from("/var/cache/r-wg")
}

#[cfg(target_os = "windows")]
fn relay_inventory_cache_dir() -> PathBuf {
    env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir)
        .join("r-wg")
        .join("cache")
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn relay_inventory_cache_dir() -> PathBuf {
    dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(env::temp_dir)
        .join("r-wg")
        .join("cache")
}
