use std::collections::HashSet;

use crate::ui::persistence::{self, PersistedConfig, StoragePaths};
use crate::ui::state::{ConfigSource, EndpointFamily, TunnelConfig};

pub(super) struct RestoredConfigs {
    pub(super) configs: Vec<TunnelConfig>,
    pub(super) config_ids: HashSet<u64>,
    pub(super) next_config_id: u64,
    pub(super) missing_files: usize,
}

pub(super) fn restore_configs(
    entries: Vec<PersistedConfig>,
    next_id: u64,
    storage: &StoragePaths,
) -> RestoredConfigs {
    let mut configs = Vec::new();
    let mut max_id = 0u64;
    let mut missing_files = 0usize;

    for entry in entries {
        if entry.id == 0 || entry.name.trim().is_empty() {
            continue;
        }
        max_id = max_id.max(entry.id);
        let storage_path = persistence::config_path(storage, entry.id);
        if !storage_path.exists() {
            missing_files += 1;
            continue;
        }
        let source = ConfigSource::from(entry.source);
        configs.push(TunnelConfig {
            id: entry.id,
            name: entry.name.clone(),
            name_lower: entry.name.to_lowercase(),
            text: None,
            source,
            storage_path,
            endpoint_family: EndpointFamily::Unknown,
        });
    }

    let config_ids = configs.iter().map(|cfg| cfg.id).collect();
    RestoredConfigs {
        configs,
        config_ids,
        next_config_id: next_id.max(max_id.saturating_add(1)),
        missing_files,
    }
}
