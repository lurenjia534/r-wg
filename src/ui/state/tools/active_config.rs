use std::path::PathBuf;
use std::sync::Arc;

use gpui::SharedString;
use r_wg::backend::wg::config;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum ActiveConfigSource {
    #[default]
    None,
    Draft,
    SavedSelection,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct ActiveConfigIdentity {
    pub(crate) source: ActiveConfigSource,
    pub(crate) config_id: Option<u64>,
    pub(crate) text_revision: u64,
}

#[derive(Clone, Default)]
pub(crate) enum ActiveConfigParseState {
    #[default]
    None,
    Loading,
    Ready(Arc<config::WireGuardConfig>),
    Invalid(SharedString),
}

#[derive(Clone, Default)]
pub(crate) struct ActiveConfigSnapshot {
    pub(crate) revision: u64,
    pub(crate) identity: Option<ActiveConfigIdentity>,
    pub(crate) source: ActiveConfigSource,
    pub(crate) source_label: SharedString,
    pub(crate) parse_state: ActiveConfigParseState,
}

impl ActiveConfigSnapshot {
    pub(crate) fn parsed_config(&self) -> Option<&config::WireGuardConfig> {
        match &self.parse_state {
            ActiveConfigParseState::Ready(parsed) => Some(parsed.as_ref()),
            _ => None,
        }
    }

    pub(crate) fn parse_error(&self) -> Option<&SharedString> {
        match &self.parse_state {
            ActiveConfigParseState::Invalid(message) => Some(message),
            _ => None,
        }
    }

    pub(crate) fn is_loading(&self) -> bool {
        matches!(self.parse_state, ActiveConfigParseState::Loading)
    }
}

#[derive(Clone)]
pub(crate) struct ActiveConfigTextRequest {
    pub(crate) identity: ActiveConfigIdentity,
    pub(crate) display_name: SharedString,
    pub(crate) inline_text: Option<SharedString>,
    pub(crate) storage_path: Option<PathBuf>,
    pub(crate) source: ActiveConfigSource,
}

pub(crate) struct ResolvedActiveConfigText {
    pub(crate) identity: ActiveConfigIdentity,
    pub(crate) display_name: SharedString,
    pub(crate) text: SharedString,
    pub(crate) source: ActiveConfigSource,
}
