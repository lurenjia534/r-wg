use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use gpui_component::theme::ThemeMode;
use r_wg::dns::{DnsMode, DnsPreset};
use serde::{Deserialize, Serialize};

use super::features::themes::AppearancePolicy;
use super::state::{ConfigInspectorTab, ConfigSource, ProxiesViewMode, TrafficPeriod};

pub(crate) const STATE_VERSION: u32 = 3;
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
    pub(crate) theme_policy: Option<AppearancePolicy>,
    #[serde(default)]
    pub(crate) theme_light_key: Option<String>,
    #[serde(default)]
    pub(crate) theme_dark_key: Option<String>,
    #[serde(default)]
    pub(crate) theme_light_name: Option<String>,
    #[serde(default)]
    pub(crate) theme_dark_name: Option<String>,
    #[serde(default)]
    pub(crate) log_auto_follow: Option<bool>,
    #[serde(default)]
    pub(crate) require_connect_password: Option<bool>,
    #[serde(default, alias = "preferred_right_tab")]
    pub(crate) preferred_inspector_tab: Option<ConfigInspectorTab>,
    #[serde(default)]
    pub(crate) preferred_traffic_period: Option<TrafficPeriod>,
    #[serde(default)]
    pub(crate) configs_library_width: Option<f32>,
    #[serde(default)]
    pub(crate) configs_inspector_width: Option<f32>,
    #[serde(default)]
    pub(crate) route_map_inventory_width: Option<f32>,
    #[serde(default)]
    pub(crate) route_map_inspector_width: Option<f32>,
    #[serde(default)]
    pub(crate) proxies_view_mode: Option<ProxiesViewMode>,
    #[serde(default)]
    pub(crate) dns_mode: Option<DnsMode>,
    #[serde(default)]
    pub(crate) dns_preset: Option<DnsPreset>,
    #[serde(default)]
    pub(crate) traffic_global_days: Vec<PersistedTrafficDayBucket>,
    #[serde(default)]
    pub(crate) traffic_global_hours: Vec<PersistedTrafficHourBucket>,
    #[serde(default)]
    pub(crate) traffic_config_days: Vec<PersistedConfigTrafficDayBucket>,
    #[serde(default)]
    pub(crate) traffic_config_hours: Vec<PersistedConfigTrafficHourBucket>,
    pub(crate) configs: Vec<PersistedConfig>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct PersistedConfig {
    pub(crate) id: u64,
    pub(crate) name: String,
    pub(crate) source: PersistedSource,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct PersistedTrafficDayBucket {
    pub(crate) day_key: i32,
    pub(crate) rx_bytes: u64,
    pub(crate) tx_bytes: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct PersistedTrafficHourBucket {
    pub(crate) hour_key: i64,
    pub(crate) rx_bytes: u64,
    pub(crate) tx_bytes: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct PersistedConfigTrafficDayBucket {
    pub(crate) config_id: u64,
    pub(crate) day_key: i32,
    pub(crate) rx_bytes: u64,
    pub(crate) tx_bytes: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct PersistedConfigTrafficHourBucket {
    pub(crate) config_id: u64,
    pub(crate) hour_key: i64,
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
            theme_policy: Some(AppearancePolicy::Dark),
            theme_light_key: Some("curated:tokyonight.json#light-tokyo-day".to_string()),
            theme_dark_key: Some("curated:tokyonight.json#dark-tokyo-night".to_string()),
            theme_light_name: Some("Signal Light".to_string()),
            theme_dark_name: Some("Network Dark".to_string()),
            log_auto_follow: Some(true),
            require_connect_password: Some(true),
            preferred_inspector_tab: Some(ConfigInspectorTab::Preview),
            preferred_traffic_period: Some(TrafficPeriod::Today),
            configs_library_width: Some(300.0),
            configs_inspector_width: Some(332.0),
            route_map_inventory_width: Some(280.0),
            route_map_inspector_width: Some(340.0),
            proxies_view_mode: Some(ProxiesViewMode::List),
            dns_mode: Some(DnsMode::FollowConfig),
            dns_preset: Some(DnsPreset::CloudflareStandard),
            traffic_global_days: vec![PersistedTrafficDayBucket {
                day_key: 20513,
                rx_bytes: 400,
                tx_bytes: 624,
            }],
            traffic_global_hours: vec![PersistedTrafficHourBucket {
                hour_key: 123,
                rx_bytes: 10,
                tx_bytes: 20,
            }],
            traffic_config_days: vec![PersistedConfigTrafficDayBucket {
                config_id: 7,
                day_key: 20513,
                rx_bytes: 200,
                tx_bytes: 300,
            }],
            traffic_config_hours: vec![PersistedConfigTrafficHourBucket {
                config_id: 7,
                hour_key: 123,
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
        assert_eq!(loaded.theme_policy, state.theme_policy);
        assert_eq!(loaded.theme_light_key, state.theme_light_key);
        assert_eq!(loaded.theme_dark_key, state.theme_dark_key);
        assert_eq!(loaded.theme_light_name, state.theme_light_name);
        assert_eq!(loaded.theme_dark_name, state.theme_dark_name);
        assert_eq!(loaded.log_auto_follow, state.log_auto_follow);
        assert_eq!(
            loaded.require_connect_password,
            state.require_connect_password
        );
        assert_eq!(
            loaded.preferred_inspector_tab,
            state.preferred_inspector_tab
        );
        assert_eq!(
            loaded.preferred_traffic_period,
            state.preferred_traffic_period
        );
        assert_eq!(loaded.proxies_view_mode, state.proxies_view_mode);
        assert_eq!(loaded.dns_mode, state.dns_mode);
        assert_eq!(loaded.dns_preset, state.dns_preset);
        assert_eq!(loaded.traffic_global_days.len(), 1);
        assert_eq!(loaded.traffic_global_days[0].rx_bytes, 400);
        assert_eq!(loaded.traffic_global_days[0].tx_bytes, 624);
        assert_eq!(loaded.traffic_global_hours.len(), 1);
        assert_eq!(loaded.traffic_global_hours[0].hour_key, 123);
        assert_eq!(loaded.traffic_config_days.len(), 1);
        assert_eq!(loaded.traffic_config_days[0].config_id, 7);
        assert_eq!(loaded.traffic_config_hours.len(), 1);
        assert_eq!(loaded.traffic_config_hours[0].hour_key, 123);
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
    fn load_legacy_state_without_connect_password_field() {
        let legacy = format!(
            r#"{{
  "version": {version},
  "next_id": 5,
  "selected_id": null,
  "configs": []
}}"#,
            version = STATE_VERSION
        );

        let state: PersistedState =
            serde_json::from_str(&legacy).expect("legacy state should deserialize");

        assert_eq!(state.version, STATE_VERSION);
        assert_eq!(state.require_connect_password, None);
        assert!(state.configs.is_empty());
    }
}
