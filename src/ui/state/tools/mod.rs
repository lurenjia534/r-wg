mod active_config;
mod cidr;
mod reachability;

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use gpui::{Entity, SharedString};

use super::WgApp;

pub(crate) use active_config::*;
pub(crate) use cidr::*;
pub(crate) use reachability::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToolsTab {
    Cidr,
    Reachability,
}

impl ToolsTab {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Cidr => "CIDR Exclusion",
            Self::Reachability => "Reachability",
        }
    }
}

#[derive(Clone)]
pub(crate) struct JobCancelHandle {
    cancelled: Arc<AtomicBool>,
}

impl JobCancelHandle {
    pub(crate) fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

pub(crate) enum AsyncJobState<T> {
    Idle,
    Running {
        generation: u64,
        cancel: JobCancelHandle,
    },
    Ready(T),
    Failed(SharedString),
}

impl<T> Default for AsyncJobState<T> {
    fn default() -> Self {
        Self::Idle
    }
}

impl<T> AsyncJobState<T> {
    pub(crate) fn cancel(&mut self) {
        if let Self::Running { cancel, .. } = self {
            cancel.cancel();
        }
    }

    pub(crate) fn is_running(&self) -> bool {
        matches!(self, Self::Running { .. })
    }

    pub(crate) fn set_running(&mut self, generation: u64) -> JobCancelHandle {
        self.cancel();
        let cancel = JobCancelHandle::new();
        *self = Self::Running {
            generation,
            cancel: cancel.clone(),
        };
        cancel
    }

    pub(crate) fn generation(&self) -> Option<u64> {
        match self {
            Self::Running { generation, .. } => Some(*generation),
            _ => None,
        }
    }
}

pub(crate) struct ToolsWorkspace {
    pub(crate) app: Entity<WgApp>,
    pub(crate) active_tab: ToolsTab,
    pub(crate) active_config: ActiveConfigSnapshot,
    pub(crate) active_config_generation: u64,
    pub(crate) active_config_cancel: Option<JobCancelHandle>,
    pub(crate) active_config_refresh_pending: bool,
    pub(crate) cidr: CidrToolState,
    pub(crate) reachability: ReachabilityToolState,
}

impl ToolsWorkspace {
    pub(crate) fn new(app: Entity<WgApp>) -> Self {
        Self {
            app,
            active_tab: ToolsTab::Cidr,
            active_config: ActiveConfigSnapshot::default(),
            active_config_generation: 0,
            active_config_cancel: None,
            active_config_refresh_pending: true,
            cidr: CidrToolState::default(),
            reachability: ReachabilityToolState::default(),
        }
    }

    pub(crate) fn set_active_tab(&mut self, value: ToolsTab) -> bool {
        if self.active_tab == value {
            return false;
        }
        self.active_tab = value;
        true
    }
}
