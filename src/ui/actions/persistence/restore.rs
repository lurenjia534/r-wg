use gpui::{Context, SharedString};

use crate::ui::persistence::{PersistedState, StoragePaths, STATE_VERSION};
use crate::ui::state::{
    ConfigsState, SelectionState, StatsState, TrafficStore, TunnelConfig, UiPrefsState, WgApp,
};

use super::configs::restore_configs;
use super::prefs::{restore_prefs, RestoredPrefs};
use super::traffic::restore_traffic_store;

pub(super) struct PersistedStateRestore {
    prefs: RestoredPrefs,
    configs: Vec<TunnelConfig>,
    next_config_id: u64,
    selected_id: Option<u64>,
    traffic: TrafficStore,
    missing_files: usize,
}

pub(super) struct PersistedStateSummary {
    pub(super) selected_id: Option<u64>,
    pub(super) loaded_count: usize,
    pub(super) missing_files: usize,
    pub(super) theme_notice: Option<SharedString>,
    pub(super) theme_prefs_migrated: bool,
}

impl PersistedStateRestore {
    pub(super) fn decode(
        state: PersistedState,
        storage: &StoragePaths,
        cx: &Context<WgApp>,
    ) -> Result<Self, String> {
        if state.version > STATE_VERSION {
            return Err(format!("Unsupported state version: {}", state.version));
        }

        let prefs = restore_prefs(&state, storage, cx);
        let restored_configs = restore_configs(state.configs, state.next_id, storage);

        Ok(Self {
            prefs,
            next_config_id: restored_configs.next_config_id,
            selected_id: state.selected_id,
            traffic: restore_traffic_store(
                state.traffic_global_days,
                state.traffic_global_hours,
                state.traffic_config_days,
                state.traffic_config_hours,
                &restored_configs.config_ids,
            ),
            configs: restored_configs.configs,
            missing_files: restored_configs.missing_files,
        })
    }

    pub(super) fn apply(
        self,
        configs: &mut ConfigsState,
        selection: &mut SelectionState,
        stats: &mut StatsState,
        ui_prefs: &mut UiPrefsState,
    ) -> PersistedStateSummary {
        let theme_notice = self.prefs.theme_notice.clone();
        let theme_prefs_migrated = self.prefs.theme_prefs_migrated;
        self.prefs.apply(ui_prefs);

        configs.configs = self.configs;
        configs.next_config_id = self.next_config_id;

        stats.traffic = self.traffic;
        stats.traffic.reset_persist_state();

        selection.restore_after_persist(self.selected_id, configs);

        PersistedStateSummary {
            selected_id: selection.selected_id,
            loaded_count: configs.len(),
            missing_files: self.missing_files,
            theme_notice,
            theme_prefs_migrated,
        }
    }
}
