use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use chrono::NaiveDate;
use gpui::{Entity, SharedString, Subscription, Timer, Window};
use gpui_component::theme::ThemeMode;
use gpui_component::{input::InputState, notification::Notification, IconName, WindowExt};
use r_wg::backend::wg::route_plan::RouteApplyReport;
use r_wg::backend::wg::{
    config, Engine, PeerStats, PrivilegedServiceAction, PrivilegedServiceStatus,
};
use r_wg::dns::{DnsMode, DnsPreset};
use serde::{Deserialize, Serialize};

use super::actions::config::endpoint_family_hint_from_config;
use super::persistence::{self, StoragePaths};
use super::themes::{self, AppearancePolicy};

// API-preserving split: keep the public `state` surface stable while separating
// shared constants, config/workspace models, traffic stats, backend diagnostics,
// navigation enums, state containers, and the WgApp facade into focused files.
include!("state/constants.rs");
include!("state/config_domain.rs");
include!("state/traffic.rs");
include!("state/backend.rs");
include!("state/navigation.rs");
include!("state/stores.rs");
include!("state/app.rs");
