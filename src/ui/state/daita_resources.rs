use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui::SharedString;
use r_wg::backend::wg::RelayInventoryStatusSnapshot;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DaitaResourcesHealth {
    Unknown,
    Checking,
    Refreshing,
    Ready,
    Missing,
    Error,
}

impl DaitaResourcesHealth {
    pub(crate) fn summary(&self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::Checking => "Checking",
            Self::Refreshing => "Refreshing",
            Self::Ready => "Ready",
            Self::Missing => "Missing",
            Self::Error => "Error",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DaitaResourcesDiagnostic {
    pub(crate) health: DaitaResourcesHealth,
    pub(crate) detail: SharedString,
    pub(crate) checked_at: Option<SystemTime>,
    pub(crate) cache_path: Option<SharedString>,
    pub(crate) relay_count: usize,
    pub(crate) daita_relay_count: usize,
    pub(crate) fetched_at: Option<SystemTime>,
}

impl DaitaResourcesDiagnostic {
    pub(crate) fn default_state() -> Self {
        Self {
            health: DaitaResourcesHealth::Unknown,
            detail: "Check or download Mullvad relay inventory before starting DAITA.".into(),
            checked_at: None,
            cache_path: None,
            relay_count: 0,
            daita_relay_count: 0,
            fetched_at: None,
        }
    }

    pub(crate) fn checking() -> Self {
        Self {
            health: DaitaResourcesHealth::Checking,
            detail: "Checking cached Mullvad relay inventory...".into(),
            checked_at: None,
            cache_path: None,
            relay_count: 0,
            daita_relay_count: 0,
            fetched_at: None,
        }
    }

    pub(crate) fn refreshing(previous: Option<&Self>) -> Self {
        let mut next = previous.cloned().unwrap_or_else(Self::default_state);
        next.health = DaitaResourcesHealth::Refreshing;
        next.detail =
            "Downloading Mullvad relay inventory for DAITA validation through the backend..."
                .into();
        next
    }

    pub(crate) fn from_snapshot(snapshot: RelayInventoryStatusSnapshot) -> Self {
        let fetched_at = snapshot
            .fetched_at_unix_secs
            .map(|secs| UNIX_EPOCH + Duration::from_secs(secs));
        let (health, detail) = if snapshot.present {
            (
                DaitaResourcesHealth::Ready,
                format!(
                    "Cached Mullvad relay inventory is available for DAITA validation ({} relays, {} DAITA-capable).",
                    snapshot.relay_count, snapshot.daita_relay_count
                ),
            )
        } else {
            (
                DaitaResourcesHealth::Missing,
                "No cached Mullvad relay inventory found. Download DAITA resources while connected through a regular Mullvad tunnel first.".to_string(),
            )
        };

        Self {
            health,
            detail: detail.into(),
            checked_at: Some(SystemTime::now()),
            cache_path: Some(snapshot.cache_path.into()),
            relay_count: snapshot.relay_count,
            daita_relay_count: snapshot.daita_relay_count,
            fetched_at,
        }
    }

    pub(crate) fn error(message: impl Into<SharedString>, previous: Option<&Self>) -> Self {
        let mut next = previous.cloned().unwrap_or_else(Self::default_state);
        next.health = DaitaResourcesHealth::Error;
        next.detail = message.into();
        next.checked_at = Some(SystemTime::now());
        next
    }

    pub(crate) fn summary(&self) -> &'static str {
        self.health.summary()
    }

    pub(crate) fn is_busy(&self) -> bool {
        matches!(
            self.health,
            DaitaResourcesHealth::Checking | DaitaResourcesHealth::Refreshing
        )
    }

    pub(crate) fn has_cache(&self) -> bool {
        matches!(
            self.health,
            DaitaResourcesHealth::Ready
                | DaitaResourcesHealth::Refreshing
                | DaitaResourcesHealth::Error
        ) && self.fetched_at.is_some()
    }

    pub(crate) fn with_checked_at(mut self, checked_at: Option<SystemTime>) -> Self {
        self.checked_at = checked_at;
        self
    }
}
