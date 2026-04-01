use std::collections::{hash_map::DefaultHasher, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use gpui::{Entity, SharedString, Subscription};
use gpui_component::input::InputState;
use r_wg::core::config;

use crate::ui::state::{
    ConfigInspectorTab, ConfigSource, ConfigsPrimaryPane, ConfigsState, EndpointFamily,
    PendingDraftAction, RuntimeState, TunnelConfig, WgApp, DEFAULT_CONFIGS_INSPECTOR_WIDTH,
    DEFAULT_CONFIGS_LIBRARY_WIDTH,
};

use super::endpoint_family::endpoint_family_hint_from_config;

#[derive(Clone)]
pub(crate) enum DraftValidationState {
    Idle,
    Valid {
        parsed: config::WireGuardConfig,
        endpoint_family: EndpointFamily,
    },
    Invalid {
        line: Option<usize>,
        message: SharedString,
    },
}

#[derive(Clone)]
pub(crate) struct ConfigDraftState {
    pub(crate) source_id: Option<u64>,
    pub(crate) revision: u64,
    pub(crate) name: SharedString,
    pub(crate) text: SharedString,
    pub(crate) base_name: SharedString,
    pub(crate) base_text_hash: u64,
    pub(crate) dirty_name: bool,
    pub(crate) dirty_text: bool,
    pub(crate) validation: DraftValidationState,
    pub(crate) needs_restart: bool,
}

impl ConfigDraftState {
    pub(crate) fn new() -> Self {
        Self {
            source_id: None,
            revision: 0,
            name: SharedString::new_static(""),
            text: SharedString::new_static(""),
            base_name: SharedString::new_static(""),
            base_text_hash: 0,
            dirty_name: false,
            dirty_text: false,
            validation: DraftValidationState::Idle,
            needs_restart: false,
        }
    }

    pub(crate) fn is_dirty(&self) -> bool {
        self.dirty_name || self.dirty_text
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EditorOperation {
    LoadingConfig,
    Saving,
    Importing { processed: usize, total: usize },
    Exporting,
    Deleting,
}

pub(crate) struct LoadedConfigState {
    pub(crate) name: String,
    pub(crate) text_hash: u64,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ConfigsLibraryRow {
    pub(crate) id: u64,
    pub(crate) name: String,
    pub(crate) name_lower: String,
    pub(crate) subtitle: String,
    pub(crate) subtitle_lower: String,
    pub(crate) source: ConfigSource,
    pub(crate) source_label: &'static str,
    pub(crate) endpoint_family: EndpointFamily,
    pub(crate) family_label: &'static str,
    pub(crate) is_running: bool,
    pub(crate) is_dirty: bool,
}

pub(crate) struct ConfigsWorkspace {
    pub(crate) app: Entity<WgApp>,
    pub(crate) draft: ConfigDraftState,
    pub(crate) operation: Option<EditorOperation>,
    pub(crate) pending_action: Option<PendingDraftAction>,
    pub(crate) validation_generation: u64,
    pub(crate) has_selection: bool,
    pub(crate) primary_pane: ConfigsPrimaryPane,
    pub(crate) inspector_tab: ConfigInspectorTab,
    pub(crate) library_rows: Arc<Vec<ConfigsLibraryRow>>,
    pub(crate) library_width: f32,
    pub(crate) inspector_width: f32,
    pub(crate) title_editing: bool,
    pub(crate) library_search_input: Option<Entity<InputState>>,
    pub(crate) name_input: Option<Entity<InputState>>,
    pub(crate) config_input: Option<Entity<InputState>>,
    pub(crate) library_search_subscription: Option<Subscription>,
    pub(crate) name_input_subscription: Option<Subscription>,
    pub(crate) config_input_subscription: Option<Subscription>,
    initialized: bool,
}

impl ConfigsWorkspace {
    pub(crate) fn new(app: Entity<WgApp>) -> Self {
        Self {
            app,
            draft: ConfigDraftState::new(),
            operation: None,
            pending_action: None,
            validation_generation: 0,
            has_selection: false,
            primary_pane: ConfigsPrimaryPane::Editor,
            inspector_tab: ConfigInspectorTab::Preview,
            library_rows: Arc::new(Vec::new()),
            library_width: DEFAULT_CONFIGS_LIBRARY_WIDTH,
            inspector_width: DEFAULT_CONFIGS_INSPECTOR_WIDTH,
            title_editing: false,
            library_search_input: None,
            name_input: None,
            config_input: None,
            library_search_subscription: None,
            name_input_subscription: None,
            config_input_subscription: None,
            initialized: false,
        }
    }

    pub(crate) fn initialize_from_app(&mut self, app: &WgApp) {
        if !self.initialized {
            self.has_selection = app.selection.selected_id.is_some();
            self.inspector_tab = app.ui_prefs.preferred_inspector_tab;
            self.library_width = app.ui_prefs.configs_library_width;
            self.inspector_width = app.ui_prefs.configs_inspector_width;
            self.library_rows = build_configs_library_rows(&app.configs, &app.runtime, &self.draft);
            self.initialized = true;
        }
    }

    pub(crate) fn set_library_rows(&mut self, rows: Arc<Vec<ConfigsLibraryRow>>) -> bool {
        if self.library_rows == rows {
            return false;
        }
        self.library_rows = rows;
        true
    }

    pub(crate) fn upsert_library_row(
        &mut self,
        config: &TunnelConfig,
        running_id: Option<u64>,
        running_name: Option<&str>,
    ) -> bool {
        let next_row =
            build_configs_library_row_with_runtime(config, running_id, running_name, &self.draft);
        let rows = Arc::make_mut(&mut self.library_rows);
        if let Some(existing) = rows.iter_mut().find(|row| row.id == config.id) {
            if *existing == next_row {
                return false;
            }
            *existing = next_row;
            return true;
        }
        rows.push(next_row);
        true
    }

    pub(crate) fn remove_library_rows(&mut self, ids: &HashSet<u64>) -> bool {
        if ids.is_empty() {
            return false;
        }
        let rows = Arc::make_mut(&mut self.library_rows);
        let before = rows.len();
        rows.retain(|row| !ids.contains(&row.id));
        before != rows.len()
    }

    pub(crate) fn append_library_rows(
        &mut self,
        configs: &[TunnelConfig],
        running_id: Option<u64>,
        running_name: Option<&str>,
    ) -> bool {
        if configs.is_empty() {
            return false;
        }
        let rows = Arc::make_mut(&mut self.library_rows);
        rows.extend(configs.iter().map(|config| {
            build_configs_library_row_with_runtime(config, running_id, running_name, &self.draft)
        }));
        true
    }

    pub(crate) fn refresh_library_row_flags(
        &mut self,
        running_id: Option<u64>,
        running_name: Option<&str>,
    ) -> bool {
        let dirty_source_id = self.draft.source_id;
        let dirty = self.draft.is_dirty();
        let mut changed = false;
        for row in Arc::make_mut(&mut self.library_rows) {
            let is_running = running_id == Some(row.id) || running_name == Some(row.name.as_str());
            let is_dirty = dirty_source_id == Some(row.id) && dirty;
            if row.is_running != is_running {
                row.is_running = is_running;
                changed = true;
            }
            if row.is_dirty != is_dirty {
                row.is_dirty = is_dirty;
                changed = true;
            }
        }
        changed
    }

    pub(crate) fn refresh_draft_flags(&mut self, running_id: Option<u64>) {
        let text_hash = workspace_text_hash(self.draft.text.as_ref());
        self.draft.dirty_name = self.draft.name != self.draft.base_name;
        self.draft.dirty_text = text_hash != self.draft.base_text_hash;
        self.draft.needs_restart = self.draft.is_dirty() && running_id == self.draft.source_id;
    }

    pub(crate) fn sync_draft_from_values(
        &mut self,
        name: SharedString,
        text: SharedString,
        running_id: Option<u64>,
    ) -> bool {
        if self.draft.name == name && self.draft.text == text {
            return false;
        }
        let text_changed = self.draft.text != text;
        self.draft.name = name;
        self.draft.text = text;
        self.draft.revision = self.draft.revision.wrapping_add(1);
        self.refresh_draft_flags(running_id);
        if text_changed {
            self.draft.validation = DraftValidationState::Idle;
        }
        true
    }

    pub(crate) fn apply_draft_validation(&mut self, running_id: Option<u64>) {
        let text = self.draft.text.clone();
        self.refresh_draft_flags(running_id);
        self.draft.validation = if text.as_ref().trim().is_empty() {
            DraftValidationState::Idle
        } else {
            match config::parse_config(text.as_ref()) {
                Ok(parsed) => DraftValidationState::Valid {
                    endpoint_family: endpoint_family_hint_from_config(&parsed),
                    parsed,
                },
                Err(err) => DraftValidationState::Invalid {
                    line: err.line,
                    message: err.message.into(),
                },
            }
        };
    }

    pub(crate) fn set_saved_draft(
        &mut self,
        source_id: u64,
        name: SharedString,
        text: SharedString,
    ) {
        self.draft = ConfigDraftState {
            source_id: Some(source_id),
            revision: 1,
            name: name.clone(),
            text: text.clone(),
            base_name: name,
            base_text_hash: workspace_text_hash(text.as_ref()),
            dirty_name: false,
            dirty_text: false,
            validation: DraftValidationState::Idle,
            needs_restart: false,
        };
        self.primary_pane = ConfigsPrimaryPane::Editor;
        self.title_editing = false;
    }

    pub(crate) fn set_unsaved_draft(&mut self, name: SharedString, text: SharedString) {
        self.draft = ConfigDraftState {
            source_id: None,
            revision: 1,
            name,
            text,
            base_name: SharedString::new_static(""),
            base_text_hash: 0,
            dirty_name: true,
            dirty_text: true,
            validation: DraftValidationState::Idle,
            needs_restart: false,
        };
        self.primary_pane = ConfigsPrimaryPane::Editor;
        self.title_editing = false;
    }

    pub(crate) fn set_primary_pane(&mut self, value: ConfigsPrimaryPane) -> bool {
        if self.primary_pane == value {
            return false;
        }
        self.primary_pane = value;
        true
    }

    pub(crate) fn set_inspector_tab(&mut self, value: ConfigInspectorTab) -> bool {
        if self.inspector_tab == value {
            return false;
        }
        self.inspector_tab = value;
        true
    }

    pub(crate) fn set_title_editing(&mut self, value: bool) -> bool {
        if self.title_editing == value {
            return false;
        }
        self.title_editing = value;
        true
    }

    pub(crate) fn has_inputs(&self) -> bool {
        self.library_search_input.is_some()
            && self.name_input.is_some()
            && self.config_input.is_some()
    }

    pub(crate) fn set_panel_widths(&mut self, library_width: f32, inspector_width: f32) -> bool {
        let changed =
            self.library_width != library_width || self.inspector_width != inspector_width;
        if changed {
            self.library_width = library_width;
            self.inspector_width = inspector_width;
        }
        changed
    }
}

pub(crate) fn build_configs_library_rows(
    configs: &ConfigsState,
    runtime: &RuntimeState,
    draft: &ConfigDraftState,
) -> Arc<Vec<ConfigsLibraryRow>> {
    Arc::new(
        configs
            .iter()
            .map(|config| {
                build_configs_library_row_with_runtime(
                    config,
                    runtime.running_id,
                    runtime.running_name.as_deref(),
                    draft,
                )
            })
            .collect(),
    )
}

fn workspace_text_hash(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

fn build_configs_library_row_with_runtime(
    config: &TunnelConfig,
    running_id: Option<u64>,
    running_name: Option<&str>,
    draft: &ConfigDraftState,
) -> ConfigsLibraryRow {
    let subtitle = match &config.source {
        ConfigSource::File { origin_path } => origin_path
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .map(|name| format!("Imported • {name}"))
            .unwrap_or_else(|| "Imported config".to_string()),
        ConfigSource::Paste => "Saved in app storage".to_string(),
    };

    ConfigsLibraryRow {
        id: config.id,
        name: config.name.clone(),
        name_lower: config.name.to_lowercase(),
        subtitle: subtitle.clone(),
        subtitle_lower: subtitle.to_lowercase(),
        source: config.source.clone(),
        source_label: configs_source_search_label(&config.source),
        endpoint_family: config.endpoint_family,
        family_label: configs_family_search_label(config.endpoint_family),
        is_running: running_id == Some(config.id) || running_name == Some(config.name.as_str()),
        is_dirty: draft.source_id == Some(config.id) && draft.is_dirty(),
    }
}

fn configs_source_search_label(source: &ConfigSource) -> &'static str {
    match source {
        ConfigSource::File { .. } => "imported",
        ConfigSource::Paste => "saved",
    }
}

fn configs_family_search_label(family: EndpointFamily) -> &'static str {
    match family {
        EndpointFamily::V4 => "ipv4",
        EndpointFamily::V6 => "ipv6",
        EndpointFamily::Dual => "dual",
        EndpointFamily::Unknown => "unknown",
    }
}
