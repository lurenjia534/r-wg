use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use gpui_component::theme::ThemeMode;
use serde::{Deserialize, Serialize};

use super::state::ProxiesViewMode;
use super::state::{ConfigSource, TrafficDay};

pub(crate) const STATE_VERSION: u32 = 1;
const STATE_FILE_NAME: &str = "state.json";
const CONFIGS_DIR_NAME: &str = "configs";

#[derive(Clone)]
pub(crate) struct StoragePaths {
    pub(crate) root: PathBuf,
    pub(crate) configs_dir: PathBuf,
    pub(crate) state_path: PathBuf,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct PersistedState {
    pub(crate) version: u32,
    pub(crate) next_id: u64,
    pub(crate) selected_id: Option<u64>,
    // 兼容旧版 state.json，字段可缺省。
    #[serde(default)]
    pub(crate) theme_mode: Option<ThemeMode>,
    #[serde(default)]
    pub(crate) proxies_view_mode: Option<ProxiesViewMode>,
    #[serde(default)]
    pub(crate) traffic_days: Vec<PersistedTrafficDay>,
    #[serde(default)]
    pub(crate) traffic_days_v2: Vec<PersistedTrafficDayStats>,
    #[serde(default)]
    pub(crate) traffic_hours: Vec<PersistedTrafficHour>,
    #[serde(default)]
    pub(crate) config_traffic_days: Vec<PersistedConfigTrafficDay>,
    #[serde(default)]
    pub(crate) config_traffic_hours: Vec<PersistedConfigTrafficHour>,
    pub(crate) configs: Vec<PersistedConfig>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct PersistedConfig {
    pub(crate) id: u64,
    pub(crate) name: String,
    pub(crate) source: PersistedSource,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct PersistedTrafficDay {
    pub(crate) date: String,
    pub(crate) bytes: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct PersistedTrafficDayStats {
    pub(crate) date: String,
    pub(crate) rx_bytes: u64,
    pub(crate) tx_bytes: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct PersistedTrafficHour {
    pub(crate) hour: i64,
    pub(crate) rx_bytes: u64,
    pub(crate) tx_bytes: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct PersistedConfigTrafficDay {
    pub(crate) config_id: u64,
    pub(crate) date: String,
    pub(crate) rx_bytes: u64,
    pub(crate) tx_bytes: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct PersistedConfigTrafficHour {
    pub(crate) config_id: u64,
    pub(crate) hour: i64,
    pub(crate) rx_bytes: u64,
    pub(crate) tx_bytes: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum PersistedSource {
    File { origin_path: Option<PathBuf> },
    Paste,
}

impl From<&ConfigSource> for PersistedSource {
    fn from(source: &ConfigSource) -> Self {
        match source {
            ConfigSource::File { origin_path } => PersistedSource::File {
                origin_path: origin_path.clone(),
            },
            ConfigSource::Paste => PersistedSource::Paste,
        }
    }
}

impl From<PersistedSource> for ConfigSource {
    fn from(source: PersistedSource) -> Self {
        match source {
            PersistedSource::File { origin_path } => ConfigSource::File { origin_path },
            PersistedSource::Paste => ConfigSource::Paste,
        }
    }
}

impl From<TrafficDay> for PersistedTrafficDay {
    fn from(day: TrafficDay) -> Self {
        Self {
            date: day.date,
            bytes: day.bytes,
        }
    }
}

impl From<PersistedTrafficDay> for TrafficDay {
    fn from(day: PersistedTrafficDay) -> Self {
        Self {
            date: day.date,
            bytes: day.bytes,
        }
    }
}

pub(crate) fn ensure_storage_dirs() -> Result<StoragePaths, String> {
    let root = dirs::data_dir()
        .map(|dir| dir.join("r-wg"))
        .ok_or_else(|| "No data directory available".to_string())?;
    let configs_dir = root.join(CONFIGS_DIR_NAME);
    let state_path = root.join(STATE_FILE_NAME);
    std::fs::create_dir_all(&configs_dir)
        .map_err(|err| format!("Create storage dir failed: {err}"))?;
    Ok(StoragePaths {
        root,
        configs_dir,
        state_path,
    })
}

pub(crate) fn config_path(paths: &StoragePaths, id: u64) -> PathBuf {
    paths.configs_dir.join(format!("{id}.conf"))
}

pub(crate) fn load_state(paths: &StoragePaths) -> Result<Option<PersistedState>, String> {
    let text = match std::fs::read_to_string(&paths.state_path) {
        Ok(text) => text,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(format!("Read state failed: {err}")),
    };
    let state = serde_json::from_str(&text).map_err(|err| format!("Parse state failed: {err}"))?;
    Ok(Some(state))
}

pub(crate) fn save_state(paths: &StoragePaths, state: &PersistedState) -> Result<(), String> {
    let data =
        serde_json::to_vec_pretty(state).map_err(|err| format!("Serialize state failed: {err}"))?;
    write_atomic(&paths.state_path, &data)
}

pub(crate) fn write_config_text(path: &Path, text: &str) -> Result<(), String> {
    write_atomic(path, text.as_bytes())
}

fn write_atomic(path: &Path, contents: &[u8]) -> Result<(), String> {
    let tmp_path = path.with_extension("tmp");
    // 清理逻辑说明：
    // - 先写入临时文件，再原子替换，避免部分写入导致文件损坏；
    // - 如果目标已存在，先删除旧文件再替换，确保最终文件一致；
    // - 任何一步失败都返回错误，调用方会在 UI 中提示。
    std::fs::write(&tmp_path, contents).map_err(|err| format!("Write temp file failed: {err}"))?;
    if let Err(err) = std::fs::rename(&tmp_path, path) {
        if path.exists() {
            std::fs::remove_file(path)
                .map_err(|remove_err| format!("Remove old file failed: {remove_err}"))?;
            std::fs::rename(&tmp_path, path)
                .map_err(|rename_err| format!("Replace file failed: {rename_err}"))?;
            return Ok(());
        }
        return Err(format!("Commit file failed: {err}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn temp_storage_paths(test_name: &str) -> StoragePaths {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("r-wg-{test_name}-{unique}"));
        let configs_dir = root.join(CONFIGS_DIR_NAME);
        let state_path = root.join(STATE_FILE_NAME);
        fs::create_dir_all(&configs_dir).expect("temp configs dir should be created");
        StoragePaths {
            root,
            configs_dir,
            state_path,
        }
    }

    fn sample_state() -> PersistedState {
        PersistedState {
            version: STATE_VERSION,
            next_id: 42,
            selected_id: Some(7),
            theme_mode: Some(ThemeMode::Dark),
            proxies_view_mode: Some(ProxiesViewMode::List),
            traffic_days: vec![PersistedTrafficDay {
                date: "2026-03-01".to_string(),
                bytes: 1024,
            }],
            traffic_days_v2: vec![PersistedTrafficDayStats {
                date: "2026-03-01".to_string(),
                rx_bytes: 400,
                tx_bytes: 624,
            }],
            traffic_hours: vec![PersistedTrafficHour {
                hour: 123,
                rx_bytes: 10,
                tx_bytes: 20,
            }],
            config_traffic_days: vec![PersistedConfigTrafficDay {
                config_id: 7,
                date: "2026-03-01".to_string(),
                rx_bytes: 200,
                tx_bytes: 300,
            }],
            config_traffic_hours: vec![PersistedConfigTrafficHour {
                config_id: 7,
                hour: 123,
                rx_bytes: 4,
                tx_bytes: 5,
            }],
            configs: vec![
                PersistedConfig {
                    id: 7,
                    name: "alpha".to_string(),
                    source: PersistedSource::Paste,
                },
                PersistedConfig {
                    id: 8,
                    name: "beta".to_string(),
                    source: PersistedSource::File {
                        origin_path: Some(PathBuf::from("/tmp/beta.conf")),
                    },
                },
            ],
        }
    }

    #[test]
    fn save_and_load_state_round_trip() {
        let paths = temp_storage_paths("state-round-trip");
        let state = sample_state();

        save_state(&paths, &state).expect("state should save");
        let loaded = load_state(&paths)
            .expect("state should load")
            .expect("saved state should exist");

        assert_eq!(loaded.version, state.version);
        assert_eq!(loaded.next_id, state.next_id);
        assert_eq!(loaded.selected_id, state.selected_id);
        assert_eq!(loaded.theme_mode, state.theme_mode);
        assert_eq!(loaded.proxies_view_mode, state.proxies_view_mode);
        assert_eq!(loaded.traffic_days.len(), 1);
        assert_eq!(loaded.traffic_days[0].date, "2026-03-01");
        assert_eq!(loaded.traffic_days[0].bytes, 1024);
        assert_eq!(loaded.traffic_days_v2.len(), 1);
        assert_eq!(loaded.traffic_days_v2[0].rx_bytes, 400);
        assert_eq!(loaded.traffic_days_v2[0].tx_bytes, 624);
        assert_eq!(loaded.traffic_hours.len(), 1);
        assert_eq!(loaded.traffic_hours[0].hour, 123);
        assert_eq!(loaded.config_traffic_days.len(), 1);
        assert_eq!(loaded.config_traffic_days[0].config_id, 7);
        assert_eq!(loaded.config_traffic_hours.len(), 1);
        assert_eq!(loaded.config_traffic_hours[0].hour, 123);
        assert_eq!(loaded.configs.len(), 2);
        assert_eq!(loaded.configs[0].name, "alpha");
        match &loaded.configs[1].source {
            PersistedSource::File { origin_path } => {
                assert_eq!(origin_path.as_deref(), Some(Path::new("/tmp/beta.conf")));
            }
            PersistedSource::Paste => panic!("expected file source"),
        }

        fs::remove_dir_all(&paths.root).expect("temp storage should be cleaned up");
    }

    #[test]
    fn write_config_text_replaces_existing_contents() {
        let paths = temp_storage_paths("write-config");
        let path = config_path(&paths, 9);

        write_config_text(&path, "first").expect("initial write should succeed");
        write_config_text(&path, "second").expect("rewrite should succeed");

        let text = fs::read_to_string(&path).expect("config file should be readable");
        assert_eq!(text, "second");

        fs::remove_dir_all(&paths.root).expect("temp storage should be cleaned up");
    }
}
