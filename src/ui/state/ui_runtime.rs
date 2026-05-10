use std::time::Instant;

use gpui::{Entity, SharedString};
use gpui_component::input::InputState;

use crate::ui::features::configs::state::ConfigsWorkspace;

use super::{
    BackendDiagnostic, BackendHealth, DaitaResourcesDiagnostic, RouteMapSearchState, ToolsWorkspace,
};

pub(crate) struct UiState {
    pub(crate) log_input: Option<Entity<InputState>>,
    pub(crate) backend_log_lines: Vec<String>,
    pub(crate) backend_log_last_sync: Option<Instant>,
    pub(crate) backend_log_sync_in_flight: bool,
    pub(crate) backend_log_last_error: Option<SharedString>,
    pub(crate) backend_log_generation: u64,
    pub(crate) backend_log_poll_active: bool,
    pub(crate) backend_log_poll_generation: u64,
    pub(crate) proxy_search_input: Option<Entity<InputState>>,
    pub(crate) route_map_search_input: Option<Entity<InputState>>,
    pub(crate) route_map_search: RouteMapSearchState,
    pub(crate) configs_workspace: Option<Entity<ConfigsWorkspace>>,
    pub(crate) tools_workspace: Option<Entity<ToolsWorkspace>>,
    pub(crate) status: SharedString,
    pub(crate) last_error: Option<SharedString>,
    pub(crate) backend: BackendDiagnostic,
    pub(crate) backend_last_error: Option<SharedString>,
    pub(crate) daita_resources: DaitaResourcesDiagnostic,
    pub(crate) theme_appearance_observer_ready: bool,
}

impl UiState {
    pub(super) fn new() -> Self {
        Self {
            log_input: None,
            backend_log_lines: Vec::new(),
            backend_log_last_sync: None,
            backend_log_sync_in_flight: false,
            backend_log_last_error: None,
            backend_log_generation: 0,
            backend_log_poll_active: false,
            backend_log_poll_generation: 0,
            proxy_search_input: None,
            route_map_search_input: None,
            route_map_search: RouteMapSearchState::new(),
            configs_workspace: None,
            tools_workspace: None,
            status: "Ready".into(),
            last_error: None,
            backend: BackendDiagnostic::default_for_platform(),
            backend_last_error: None,
            daita_resources: DaitaResourcesDiagnostic::default_state(),
            theme_appearance_observer_ready: false,
        }
    }

    pub(crate) fn set_status(&mut self, message: impl Into<SharedString>) -> bool {
        let message = message.into();
        if self.status == message {
            return false;
        }
        self.status = message;
        true
    }

    pub(crate) fn set_error(&mut self, message: impl Into<SharedString>) -> bool {
        let message = message.into();
        let changed = self.status != message || self.last_error.as_ref() != Some(&message);
        self.status = message.clone();
        self.last_error = Some(message);
        changed
    }

    pub(crate) fn set_backend_diagnostic(&mut self, diagnostic: BackendDiagnostic) {
        match diagnostic.health {
            BackendHealth::AccessDenied
            | BackendHealth::VersionMismatch { .. }
            | BackendHealth::Unreachable => {
                self.backend_last_error = Some(diagnostic.detail.clone());
            }
            BackendHealth::Running | BackendHealth::Installed | BackendHealth::NotInstalled => {
                self.backend_last_error = None;
            }
            BackendHealth::Unknown | BackendHealth::Checking | BackendHealth::Working { .. } => {}
            #[cfg(not(any(target_os = "linux", target_os = "windows")))]
            BackendHealth::Unsupported => {
                self.backend_last_error = None;
            }
        }
        self.backend = diagnostic;
    }

    pub(crate) fn set_backend_last_error(&mut self, message: impl Into<SharedString>) {
        self.backend_last_error = Some(message.into());
    }

    pub(crate) fn set_daita_resources_diagnostic(&mut self, diagnostic: DaitaResourcesDiagnostic) {
        self.daita_resources = diagnostic;
    }
}
