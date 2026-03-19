use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::net::IpAddr;
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use gpui::{Entity, SharedString, Subscription, Window};
use gpui_component::theme::{Theme, ThemeMode};
use gpui_component::{input::InputState, notification::Notification, IconName, WindowExt};
use r_wg::backend::wg::{
    config, Engine, PeerStats, PrivilegedServiceAction, PrivilegedServiceStatus,
};
use r_wg::dns::{DnsMode, DnsPreset};
use serde::{Deserialize, Serialize};

use super::persistence::{self, StoragePaths};

/// 速度曲线采样点数量（固定窗口）。
pub(crate) const SPARKLINE_SAMPLES: usize = 24;
/// 7 日流量趋势展示天数。
pub(crate) const TRAFFIC_TREND_DAYS: usize = 7;
/// 持久化的流量历史天数（限制 state.json 体积）。
pub(crate) const TRAFFIC_HISTORY_DAYS: usize = 30;
/// Traffic Summary 的滚动天数（过去 30 天 + 前 30 天）。
pub(crate) const TRAFFIC_ROLLING_DAYS: usize = 60;
/// Traffic Summary 的滚动小时数（过去 24 小时，预留 48 小时）。
pub(crate) const TRAFFIC_HOURLY_HISTORY: usize = 48;
/// stop -> start 的最短冷却时间。
pub(crate) const RESTART_COOLDOWN: Duration = Duration::from_millis(300);
pub(crate) const DEFAULT_CONFIGS_LIBRARY_WIDTH: f32 = 300.0;
pub(crate) const DEFAULT_CONFIGS_INSPECTOR_WIDTH: f32 = 332.0;

/// 配置来源：文件或粘贴文本。
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum ConfigSource {
    File { origin_path: Option<PathBuf> },
    Paste,
}

/// Endpoint 地址族标识（基于配置文本解析）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum EndpointFamily {
    V4,
    V6,
    Dual,
    Unknown,
}

/// 隧道配置条目：用于配置列表与编辑器。
#[derive(Clone)]
pub(crate) struct TunnelConfig {
    /// 持久化 ID（用于内部文件名）。
    pub(crate) id: u64,
    /// 配置名称（用于列表与启动）。
    pub(crate) name: String,
    /// 小写版本的名称，用于搜索过滤，避免每次渲染都重复分配/转换。
    pub(crate) name_lower: String,
    /// 配置文本：文件导入时懒加载，因此可能为空。
    pub(crate) text: Option<SharedString>,
    /// 配置来源：文件路径或粘贴内容。
    pub(crate) source: ConfigSource,
    /// 内部存储路径：用于持久化读写。
    pub(crate) storage_path: PathBuf,
    /// Endpoint 地址族 metadata，供 Proxies 页直接读取，避免渲染时派生。
    pub(crate) endpoint_family: EndpointFamily,
}

/// 延迟启动请求（用于 stop -> start 过渡期间）。
#[derive(Clone, Copy)]
pub(crate) struct PendingStart {
    pub(crate) config_id: u64,
}

impl TunnelConfig {
    pub(crate) fn label(&self) -> String {
        match &self.source {
            ConfigSource::File { origin_path } => {
                let file = origin_path
                    .as_ref()
                    .and_then(|path| path.file_name())
                    .and_then(|name| name.to_str())
                    .unwrap_or("file");
                format!("{} ({})", self.name, file)
            }
            ConfigSource::Paste => format!("{} (pasted)", self.name),
        }
    }
}

/// 最近一次载入到输入框的配置，避免重复 set_value。
pub(crate) struct LoadedConfigState {
    pub(crate) name: String,
    pub(crate) text_hash: u64,
}

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
    /// 这份 draft 对应的已保存配置 ID；None 表示尚未保存的新 draft。
    pub(crate) source_id: Option<u64>,
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

pub(crate) struct EditorState {
    pub(crate) draft: ConfigDraftState,
    pub(crate) operation: Option<EditorOperation>,
}

impl EditorState {
    fn new() -> Self {
        Self {
            draft: ConfigDraftState::new(),
            operation: None,
        }
    }
}

pub(crate) struct ConfigsWorkspace {
    pub(crate) app: Entity<WgApp>,
    pub(crate) draft: ConfigDraftState,
    pub(crate) operation: Option<EditorOperation>,
    pub(crate) pending_action: Option<PendingDraftAction>,
    pub(crate) validation_generation: u64,
    pub(crate) has_selection: bool,
    pub(crate) inspector_tab: ConfigInspectorTab,
    pub(crate) library_rows: Vec<ConfigsLibraryRow>,
    pub(crate) library_width: f32,
    pub(crate) inspector_width: f32,
    pub(crate) name_input: Option<Entity<InputState>>,
    pub(crate) config_input: Option<Entity<InputState>>,
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
            inspector_tab: ConfigInspectorTab::Preview,
            library_rows: Vec::new(),
            library_width: DEFAULT_CONFIGS_LIBRARY_WIDTH,
            inspector_width: DEFAULT_CONFIGS_INSPECTOR_WIDTH,
            name_input: None,
            config_input: None,
            name_input_subscription: None,
            config_input_subscription: None,
            initialized: false,
        }
    }

    pub(crate) fn sync_from_app(&mut self, app: &WgApp) {
        if !self.initialized {
            self.has_selection = app.selection.selected_id.is_some();
            self.inspector_tab = app.ui_prefs.preferred_inspector_tab;
            self.library_width = app.ui_prefs.configs_library_width;
            self.inspector_width = app.ui_prefs.configs_inspector_width;
            self.initialized = true;
        }

        let next_rows = app
            .configs
            .iter()
            .map(|config| ConfigsLibraryRow {
                id: config.id,
                name: config.name.clone(),
                subtitle: match &config.source {
                    ConfigSource::File { origin_path } => origin_path
                        .as_ref()
                        .and_then(|path| path.file_name())
                        .and_then(|name| name.to_str())
                        .map(|name| format!("Imported • {name}"))
                        .unwrap_or_else(|| "Imported config".to_string()),
                    ConfigSource::Paste => "Saved in app storage".to_string(),
                },
                source: config.source.clone(),
                endpoint_family: config.endpoint_family,
                is_running: app.runtime.running_id == Some(config.id)
                    || app.runtime.running_name.as_deref() == Some(config.name.as_str()),
                is_dirty: self.draft.source_id == Some(config.id) && self.draft.is_dirty(),
            })
            .collect::<Vec<_>>();

        if self.library_rows != next_rows {
            self.library_rows = next_rows;
        }
    }

    pub(crate) fn sync_editor_snapshot(
        &mut self,
        draft: ConfigDraftState,
        operation: Option<EditorOperation>,
        pending_action: Option<PendingDraftAction>,
        validation_generation: u64,
        has_selection: bool,
    ) {
        self.draft = draft;
        self.operation = operation;
        self.pending_action = pending_action;
        self.validation_generation = validation_generation;
        self.has_selection = has_selection;
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
                    endpoint_family: workspace_endpoint_family_hint_from_config(&parsed),
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
            name: name.clone(),
            text: text.clone(),
            base_name: name,
            base_text_hash: workspace_text_hash(text.as_ref()),
            dirty_name: false,
            dirty_text: false,
            validation: DraftValidationState::Idle,
            needs_restart: false,
        };
    }

    pub(crate) fn set_unsaved_draft(&mut self, name: SharedString, text: SharedString) {
        self.draft = ConfigDraftState {
            source_id: None,
            name,
            text,
            base_name: SharedString::new_static(""),
            base_text_hash: 0,
            dirty_name: true,
            dirty_text: true,
            validation: DraftValidationState::Idle,
            needs_restart: false,
        };
    }

    pub(crate) fn set_inspector_tab(&mut self, value: ConfigInspectorTab) -> bool {
        if self.inspector_tab == value {
            return false;
        }
        self.inspector_tab = value;
        true
    }

    pub(crate) fn has_inputs(&self) -> bool {
        self.name_input.is_some() && self.config_input.is_some()
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

fn workspace_text_hash(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

fn workspace_endpoint_family_hint_from_config(cfg: &config::WireGuardConfig) -> EndpointFamily {
    let mut has_v4 = false;
    let mut has_v6 = false;

    for peer in &cfg.peers {
        let Some(endpoint) = &peer.endpoint else {
            continue;
        };
        let host = endpoint.host.trim();
        if host.is_empty() {
            continue;
        }
        if let Ok(addr) = host.parse::<IpAddr>() {
            if addr.is_ipv4() {
                has_v4 = true;
            } else {
                has_v6 = true;
            }
            continue;
        }
        if host.contains(':') {
            has_v6 = true;
        }
    }

    match (has_v4, has_v6) {
        (true, true) => EndpointFamily::Dual,
        (true, false) => EndpointFamily::V4,
        (false, true) => EndpointFamily::V6,
        (false, false) => EndpointFamily::Unknown,
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ConfigsLibraryRow {
    pub(crate) id: u64,
    pub(crate) name: String,
    pub(crate) subtitle: String,
    pub(crate) source: ConfigSource,
    pub(crate) endpoint_family: EndpointFamily,
    pub(crate) is_running: bool,
    pub(crate) is_dirty: bool,
}

/// 日流量统计（按本地日期汇总）。
#[derive(Clone)]
pub(crate) struct TrafficDay {
    pub(crate) date: String,
    pub(crate) bytes: u64,
}

/// 按天统计的 RX/TX 统计（用于 30 天窗口）。
#[derive(Clone)]
pub(crate) struct TrafficDayStats {
    pub(crate) date: String,
    pub(crate) rx_bytes: u64,
    pub(crate) tx_bytes: u64,
}

/// 按小时统计的 RX/TX 统计（用于 24 小时窗口）。
#[derive(Clone)]
pub(crate) struct TrafficHour {
    pub(crate) hour: i64,
    pub(crate) rx_bytes: u64,
    pub(crate) tx_bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConfigInspectorTab {
    #[serde(alias = "status")]
    Preview,
    #[serde(alias = "logs")]
    Activity,
    Diagnostics,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TrafficPeriod {
    Today,
    ThisMonth,
    LastMonth,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum BackendHealth {
    Unknown,
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    Unsupported,
    Checking,
    Running,
    Installed,
    NotInstalled,
    AccessDenied,
    VersionMismatch {
        expected: u32,
        actual: u32,
    },
    Unreachable,
    Working {
        action: PrivilegedServiceAction,
    },
}

impl BackendHealth {
    pub(crate) fn summary(&self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            #[cfg(not(any(target_os = "linux", target_os = "windows")))]
            Self::Unsupported => "Unsupported",
            Self::Checking => "Checking",
            Self::Running => "Running",
            Self::Installed => "Installed",
            Self::NotInstalled => "Not installed",
            Self::AccessDenied => "Access denied",
            Self::VersionMismatch { .. } => "Version mismatch",
            Self::Unreachable => "Unreachable",
            Self::Working { action } => match action {
                PrivilegedServiceAction::Install => "Installing",
                PrivilegedServiceAction::Repair => "Repairing",
                PrivilegedServiceAction::Remove => "Removing",
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BackendDiagnostic {
    pub(crate) health: BackendHealth,
    pub(crate) detail: SharedString,
    pub(crate) checked_at: Option<SystemTime>,
}

impl BackendDiagnostic {
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    pub(crate) fn default_for_platform() -> Self {
        Self {
            health: BackendHealth::Unknown,
            detail: "Refresh to probe the privileged backend service.".into(),
            checked_at: None,
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    pub(crate) fn default_for_platform() -> Self {
        Self::unsupported()
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    pub(crate) fn unsupported() -> Self {
        Self {
            health: BackendHealth::Unsupported,
            detail: "Privileged backend management is not supported on this platform.".into(),
            checked_at: None,
        }
    }

    pub(crate) fn checking() -> Self {
        Self {
            health: BackendHealth::Checking,
            detail: "Probing privileged backend service...".into(),
            checked_at: None,
        }
    }

    pub(crate) fn working(action: PrivilegedServiceAction) -> Self {
        let detail = match action {
            PrivilegedServiceAction::Install => "Installing the privileged backend service...",
            PrivilegedServiceAction::Repair => "Repairing the privileged backend service...",
            PrivilegedServiceAction::Remove => "Removing the privileged backend service...",
        };

        Self {
            health: BackendHealth::Working { action },
            detail: detail.into(),
            checked_at: None,
        }
    }

    pub(crate) fn from_probe_status(status: PrivilegedServiceStatus) -> Self {
        match status {
            PrivilegedServiceStatus::Running => Self {
                health: BackendHealth::Running,
                detail:
                    "The privileged backend service is running and ready to handle tunnel control."
                        .into(),
                checked_at: None,
            },
            PrivilegedServiceStatus::Installed => Self {
                health: BackendHealth::Installed,
                detail:
                    "The privileged backend service is installed but not currently reporting a live control channel."
                        .into(),
                checked_at: None,
            },
            PrivilegedServiceStatus::NotInstalled => Self {
                health: BackendHealth::NotInstalled,
                detail:
                    "Install the privileged backend to enable tunnel control from the unprivileged UI."
                        .into(),
                checked_at: None,
            },
            PrivilegedServiceStatus::AccessDenied => Self {
                health: BackendHealth::AccessDenied,
                detail:
                    "The backend service exists, but this user cannot access its control channel."
                        .into(),
                checked_at: None,
            },
            PrivilegedServiceStatus::VersionMismatch { expected, actual } => Self {
                health: BackendHealth::VersionMismatch { expected, actual },
                detail: format!(
                    "GUI expects protocol v{expected}, but the running service reports v{actual}. Repair the backend installation."
                )
                .into(),
                checked_at: None,
            },
            PrivilegedServiceStatus::Unreachable(message) => Self {
                health: BackendHealth::Unreachable,
                detail: message.into(),
                checked_at: None,
            },
            #[cfg(not(any(target_os = "linux", target_os = "windows")))]
            PrivilegedServiceStatus::Unsupported => Self::unsupported(),
        }
        .checked_now()
    }

    pub(crate) fn summary(&self) -> &'static str {
        self.health.summary()
    }

    pub(crate) fn badge_label(&self) -> &'static str {
        match self.health {
            BackendHealth::Running => "Backend ready",
            BackendHealth::Checking => "Checking backend",
            BackendHealth::Working { action } => match action {
                PrivilegedServiceAction::Install => "Installing",
                PrivilegedServiceAction::Repair => "Repairing",
                PrivilegedServiceAction::Remove => "Removing",
            },
            _ => self.summary(),
        }
    }

    pub(crate) fn is_busy(&self) -> bool {
        matches!(
            self.health,
            BackendHealth::Checking | BackendHealth::Working { .. }
        )
    }

    pub(crate) fn allows_action(&self, action: PrivilegedServiceAction) -> bool {
        match action {
            PrivilegedServiceAction::Install => {
                matches!(self.health, BackendHealth::NotInstalled)
            }
            PrivilegedServiceAction::Repair => matches!(
                self.health,
                BackendHealth::Installed
                    | BackendHealth::Running
                    | BackendHealth::AccessDenied
                    | BackendHealth::VersionMismatch { .. }
                    | BackendHealth::Unreachable
            ),
            PrivilegedServiceAction::Remove => matches!(
                self.health,
                BackendHealth::Installed
                    | BackendHealth::Running
                    | BackendHealth::AccessDenied
                    | BackendHealth::VersionMismatch { .. }
            ),
        }
    }

    pub(crate) fn is_working_action(&self, action: PrivilegedServiceAction) -> bool {
        matches!(self.health, BackendHealth::Working { action: current } if current == action)
    }

    pub(crate) fn with_checked_at(mut self, checked_at: Option<SystemTime>) -> Self {
        self.checked_at = checked_at;
        self
    }

    fn checked_now(mut self) -> Self {
        self.checked_at = Some(SystemTime::now());
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProxiesViewMode {
    List,
    Gallery,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProxyRunningFilter {
    All,
    Running,
    Idle,
}

/// 左侧导航栏的选中项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SidebarItem {
    Overview,
    TrafficStats,
    Connections,
    Logs,
    Proxies,
    Rules,
    Dns,
    Providers,
    Configs,
    Advanced,
    Topology,
    RouteMap,
    About,
}

impl SidebarItem {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::TrafficStats => "Traffic Stats",
            Self::Connections => "Connections",
            Self::Logs => "Logs",
            Self::Proxies => "Proxies",
            Self::Rules => "Rules",
            Self::Dns => "DNS",
            Self::Providers => "Providers",
            Self::Configs => "Configs",
            Self::Advanced => "Preferences",
            Self::Topology => "Topology",
            Self::RouteMap => "Route Map",
            Self::About => "About",
        }
    }

    pub(crate) fn icon(self) -> IconName {
        match self {
            Self::Overview => IconName::LayoutDashboard,
            Self::TrafficStats => IconName::ChartPie,
            Self::Connections => IconName::Globe,
            Self::Logs => IconName::SquareTerminal,
            Self::Proxies => IconName::Globe,
            Self::Rules => IconName::Menu,
            Self::Dns => IconName::Search,
            Self::Providers => IconName::Building2,
            Self::Configs => IconName::File,
            Self::Advanced => IconName::Settings2,
            Self::Topology => IconName::Frame,
            Self::RouteMap => IconName::Map,
            Self::About => IconName::Info,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PendingDraftAction {
    SelectConfig(u64),
    ActivateSidebar(SidebarItem),
    NewDraft,
    Import,
    Paste,
    DeleteCurrent,
    RestartTunnel,
}

pub(crate) struct ConfigsState {
    /// 全部隧道配置。
    pub(crate) configs: Vec<TunnelConfig>,
    /// 配置持久化目录与 state.json 路径。
    pub(crate) storage: Option<StoragePaths>,
    /// 下一个配置 ID（用于内部文件名）。
    pub(crate) next_config_id: u64,
}

impl ConfigsState {
    fn new() -> Self {
        Self {
            configs: Vec::new(),
            storage: None,
            next_config_id: 1,
        }
    }

    pub(crate) fn ensure_storage(&mut self) -> Result<StoragePaths, String> {
        if let Some(storage) = &self.storage {
            return Ok(storage.clone());
        }
        let storage = persistence::ensure_storage_dirs()?;
        self.storage = Some(storage.clone());
        Ok(storage)
    }

    pub(crate) fn alloc_config_id(&mut self) -> u64 {
        let id = self.next_config_id.max(1);
        self.next_config_id = id.saturating_add(1);
        id
    }

    pub(crate) fn find_by_id(&self, config_id: u64) -> Option<TunnelConfig> {
        self.get_by_id(config_id).cloned()
    }

    pub(crate) fn find_index_by_id(&self, config_id: u64) -> Option<usize> {
        self.iter().position(|config| config.id == config_id)
    }

    pub(crate) fn get_by_id(&self, config_id: u64) -> Option<&TunnelConfig> {
        self.iter().find(|config| config.id == config_id)
    }

    pub(crate) fn get_mut_by_id(&mut self, config_id: u64) -> Option<&mut TunnelConfig> {
        self.iter_mut().find(|config| config.id == config_id)
    }
}

impl Deref for ConfigsState {
    type Target = Vec<TunnelConfig>;

    fn deref(&self) -> &Self::Target {
        &self.configs
    }
}

impl DerefMut for ConfigsState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.configs
    }
}

impl<'a> IntoIterator for &'a ConfigsState {
    type Item = &'a TunnelConfig;
    type IntoIter = std::slice::Iter<'a, TunnelConfig>;

    fn into_iter(self) -> Self::IntoIter {
        self.configs.iter()
    }
}

impl<'a> IntoIterator for &'a mut ConfigsState {
    type Item = &'a mut TunnelConfig;
    type IntoIter = std::slice::IterMut<'a, TunnelConfig>;

    fn into_iter(self) -> Self::IntoIter {
        self.configs.iter_mut()
    }
}

pub(crate) struct SelectionState {
    /// 是否已触发持久化加载，避免重复启动加载任务。
    pub(crate) persistence_loaded: bool,
    /// 当前单选中的配置 ID。
    pub(crate) selected_id: Option<u64>,
    /// 正在异步加载的配置 ID（用于防止 UI 写入旧数据）。
    pub(crate) loading_config_id: Option<u64>,
    /// 正在异步加载的配置路径（用于防止索引复用带来的错写）。
    pub(crate) loading_config_path: Option<PathBuf>,
    /// 最近一次写入输入框的配置标记，用于跳过重复 set_value。
    pub(crate) loaded_config: Option<LoadedConfigState>,
    /// 文本缓存：按路径缓存最近读取的配置文本，减少重复 IO。
    pub(crate) config_text_cache: HashMap<PathBuf, SharedString>,
    /// 文本缓存顺序：用于简易 LRU 淘汰。
    pub(crate) config_text_cache_order: VecDeque<PathBuf>,
    /// Endpoint metadata 正在后台计算中的配置 ID。
    pub(crate) endpoint_family_loading: HashSet<u64>,
    /// 代理列表结构化筛选：国家。
    pub(crate) proxy_country_filter: Option<String>,
    /// 代理列表结构化筛选：城市。
    pub(crate) proxy_city_filter: Option<String>,
    /// 代理列表结构化筛选：协议类型。
    pub(crate) proxy_protocol_filter: Option<String>,
    /// 代理列表结构化筛选：运行状态。
    pub(crate) proxy_running_filter: ProxyRunningFilter,
    /// 代理/节点多选模式开关。
    pub(crate) proxy_select_mode: bool,
    /// 代理/节点多选：选中的配置 ID 列表。
    pub(crate) proxy_selected_ids: HashSet<u64>,
}

impl SelectionState {
    fn new() -> Self {
        Self {
            persistence_loaded: false,
            selected_id: None,
            loading_config_id: None,
            loading_config_path: None,
            loaded_config: None,
            config_text_cache: HashMap::new(),
            config_text_cache_order: VecDeque::new(),
            endpoint_family_loading: HashSet::new(),
            proxy_country_filter: None,
            proxy_city_filter: None,
            proxy_protocol_filter: None,
            proxy_running_filter: ProxyRunningFilter::All,
            proxy_select_mode: false,
            proxy_selected_ids: HashSet::new(),
        }
    }

    pub(crate) fn begin_persistence_load(&mut self) -> bool {
        if self.persistence_loaded {
            return false;
        }
        self.persistence_loaded = true;
        true
    }

    pub(crate) fn build_pending_start(
        &self,
        configs: &ConfigsState,
        runtime: &RuntimeState,
    ) -> Option<PendingStart> {
        if let Some(config_id) = self.selected_id {
            return configs.get_by_id(config_id).map(|config| PendingStart {
                config_id: config.id,
            });
        }
        runtime.running_id.map(|id| PendingStart { config_id: id })
    }

    pub(crate) fn restore_after_persist(
        &mut self,
        selected_id: Option<u64>,
        configs: &ConfigsState,
    ) {
        self.selected_id = selected_id.filter(|id| configs.get_by_id(*id).is_some());
        self.loaded_config = None;
        self.loading_config_id = None;
        self.loading_config_path = None;
    }
}

pub(crate) struct RuntimeState {
    /// 是否处于运行中。
    pub(crate) running: bool,
    /// 是否有异步流程正在执行。
    pub(crate) busy: bool,
    /// 停止过程中记录的“待启动”请求。
    pub(crate) pending_start: Option<PendingStart>,
    /// 最近一次停止完成的时间（用于冷却启动）。
    pub(crate) last_stop_at: Option<Instant>,
    pub(crate) running_name: Option<String>,
    pub(crate) running_id: Option<u64>,
}

impl RuntimeState {
    fn new() -> Self {
        Self {
            running: false,
            busy: false,
            pending_start: None,
            last_stop_at: None,
            running_name: None,
            running_id: None,
        }
    }

    pub(crate) fn restart_delay(&self) -> Option<Duration> {
        let last_stop = self.last_stop_at?;
        let elapsed = last_stop.elapsed();
        if elapsed >= RESTART_COOLDOWN {
            None
        } else {
            Some(RESTART_COOLDOWN - elapsed)
        }
    }

    pub(crate) fn queue_pending_start(&mut self, pending: Option<PendingStart>) -> bool {
        let Some(pending) = pending else {
            return false;
        };
        self.pending_start = Some(pending);
        true
    }

    pub(crate) fn begin_stop(&mut self) {
        self.busy = true;
    }

    pub(crate) fn finish_stop_success(&mut self) {
        self.busy = false;
        self.running = false;
        self.running_name = None;
        self.running_id = None;
        self.last_stop_at = Some(Instant::now());
    }

    pub(crate) fn finish_stop_failure(&mut self) {
        self.busy = false;
        self.pending_start = None;
    }

    pub(crate) fn begin_start(&mut self) {
        self.busy = true;
    }

    pub(crate) fn finish_start_attempt(&mut self) {
        self.busy = false;
    }

    pub(crate) fn mark_started(&mut self, selected: &TunnelConfig) {
        self.running = true;
        self.running_name = Some(selected.name.clone());
        self.running_id = Some(selected.id);
    }
}

pub(crate) struct StatsState {
    /// 最近一次拉取到的 Peer 统计。
    pub(crate) peer_stats: Vec<PeerStats>,
    // 统计展示（用于右侧面板与图表）。
    pub(crate) stats_note: SharedString,
    pub(crate) stats_generation: u64,
    // 速率/流量采样窗口。
    pub(crate) started_at: Option<Instant>,
    pub(crate) last_stats_at: Option<Instant>,
    pub(crate) last_rx_bytes: u64,
    pub(crate) last_tx_bytes: u64,
    pub(crate) rx_rate_bps: f64,
    pub(crate) tx_rate_bps: f64,
    pub(crate) rx_rate_history: VecDeque<f32>,
    pub(crate) tx_rate_history: VecDeque<f32>,
    pub(crate) stats_idle_samples: u8,
    pub(crate) last_iface_rx_bytes: u64,
    pub(crate) last_iface_tx_bytes: u64,
    pub(crate) iface_rx_rate_bps: f64,
    pub(crate) iface_tx_rate_bps: f64,
    // 7 日流量趋势（按天累计）。
    pub(crate) traffic_days: Vec<TrafficDay>,
    pub(crate) traffic_days_v2: Vec<TrafficDayStats>,
    pub(crate) traffic_hours: Vec<TrafficHour>,
    pub(crate) config_traffic_days: HashMap<u64, Vec<TrafficDayStats>>,
    pub(crate) config_traffic_hours: HashMap<u64, Vec<TrafficHour>>,
    pub(crate) traffic_dirty: bool,
    pub(crate) traffic_last_persist_at: Option<Instant>,
}

impl StatsState {
    fn new() -> Self {
        Self {
            peer_stats: Vec::new(),
            stats_note: "Peer stats unavailable".into(),
            stats_generation: 0,
            started_at: None,
            last_stats_at: None,
            last_rx_bytes: 0,
            last_tx_bytes: 0,
            rx_rate_bps: 0.0,
            tx_rate_bps: 0.0,
            rx_rate_history: init_rate_history(),
            tx_rate_history: init_rate_history(),
            stats_idle_samples: 0,
            last_iface_rx_bytes: 0,
            last_iface_tx_bytes: 0,
            iface_rx_rate_bps: 0.0,
            iface_tx_rate_bps: 0.0,
            traffic_days: Vec::new(),
            traffic_days_v2: Vec::new(),
            traffic_hours: Vec::new(),
            config_traffic_days: HashMap::new(),
            config_traffic_hours: HashMap::new(),
            traffic_dirty: false,
            traffic_last_persist_at: None,
        }
    }

    pub(crate) fn reset_rate_history(&mut self) {
        self.rx_rate_history = init_rate_history();
        self.tx_rate_history = init_rate_history();
    }

    pub(crate) fn clear_runtime_metrics(&mut self) {
        self.peer_stats.clear();
        self.stats_note = "Peer stats unavailable".into();
        self.started_at = None;
        self.last_stats_at = None;
        self.last_rx_bytes = 0;
        self.last_tx_bytes = 0;
        self.rx_rate_bps = 0.0;
        self.tx_rate_bps = 0.0;
        self.reset_rate_history();
        self.stats_idle_samples = 0;
        self.last_iface_rx_bytes = 0;
        self.last_iface_tx_bytes = 0;
        self.iface_rx_rate_bps = 0.0;
        self.iface_tx_rate_bps = 0.0;
    }

    pub(crate) fn reset_for_start(&mut self) {
        self.started_at = Some(Instant::now());
        self.last_stats_at = None;
        self.last_rx_bytes = 0;
        self.last_tx_bytes = 0;
        self.rx_rate_bps = 0.0;
        self.tx_rate_bps = 0.0;
        self.reset_rate_history();
        self.stats_idle_samples = 0;
        self.last_iface_rx_bytes = 0;
        self.last_iface_tx_bytes = 0;
        self.iface_rx_rate_bps = 0.0;
        self.iface_tx_rate_bps = 0.0;
        self.stats_note = "Fetching peer stats...".into();
    }

    pub(crate) fn set_stats_error(&mut self, message: impl Into<SharedString>) -> bool {
        let message = message.into();
        if self.stats_note == message {
            return false;
        }
        self.stats_note = message;
        true
    }
}

pub(crate) struct UiPrefsState {
    pub(crate) log_auto_follow: bool,
    pub(crate) preferred_inspector_tab: ConfigInspectorTab,
    pub(crate) preferred_traffic_period: TrafficPeriod,
    pub(crate) configs_library_width: f32,
    pub(crate) configs_inspector_width: f32,
    pub(crate) proxies_view_mode: ProxiesViewMode,
    pub(crate) theme_mode: ThemeMode,
    pub(crate) dns_mode: DnsMode,
    pub(crate) dns_preset: DnsPreset,
}

impl UiPrefsState {
    fn new(theme_mode: ThemeMode) -> Self {
        Self {
            log_auto_follow: true,
            preferred_inspector_tab: ConfigInspectorTab::Preview,
            preferred_traffic_period: TrafficPeriod::Today,
            configs_library_width: DEFAULT_CONFIGS_LIBRARY_WIDTH,
            configs_inspector_width: DEFAULT_CONFIGS_INSPECTOR_WIDTH,
            proxies_view_mode: ProxiesViewMode::List,
            theme_mode,
            dns_mode: DnsMode::FollowConfig,
            dns_preset: DnsPreset::CloudflareStandard,
        }
    }
}

pub(crate) struct UiSessionState {
    pub(crate) traffic_period: TrafficPeriod,
    pub(crate) sidebar_active: SidebarItem,
}

impl UiSessionState {
    fn from_prefs(prefs: &UiPrefsState) -> Self {
        Self {
            traffic_period: prefs.preferred_traffic_period,
            sidebar_active: SidebarItem::Overview,
        }
    }

    pub(crate) fn sync_from_prefs(&mut self, prefs: &UiPrefsState) {
        self.traffic_period = prefs.preferred_traffic_period;
    }
}

pub(crate) struct PersistenceState {
    next_revision: u64,
    queued_revision: Option<u64>,
    pub(crate) worker_active: bool,
}

impl PersistenceState {
    fn new() -> Self {
        Self {
            next_revision: 0,
            queued_revision: None,
            worker_active: false,
        }
    }

    pub(crate) fn enqueue(&mut self) -> u64 {
        self.next_revision = self.next_revision.saturating_add(1);
        self.queued_revision = Some(self.next_revision);
        self.next_revision
    }

    pub(crate) fn take_queued_revision(&mut self) -> Option<u64> {
        self.queued_revision.take()
    }

    pub(crate) fn has_pending(&self) -> bool {
        self.queued_revision.is_some()
    }
}

pub(crate) struct UiState {
    pub(crate) log_input: Option<Entity<InputState>>,
    pub(crate) proxy_search_input: Option<Entity<InputState>>,
    pub(crate) configs_workspace: Option<Entity<ConfigsWorkspace>>,
    // 日志状态与提示。
    pub(crate) status: SharedString,
    pub(crate) last_error: Option<SharedString>,
    pub(crate) backend: BackendDiagnostic,
    pub(crate) backend_last_error: Option<SharedString>,
}

impl UiState {
    fn new() -> Self {
        Self {
            log_input: None,
            proxy_search_input: None,
            configs_workspace: None,
            status: "Ready".into(),
            last_error: None,
            backend: BackendDiagnostic::default_for_platform(),
            backend_last_error: None,
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
}

pub(crate) struct WgApp {
    pub(crate) engine: Engine,
    pub(crate) configs: ConfigsState,
    pub(crate) selection: SelectionState,
    pub(crate) editor: EditorState,
    pub(crate) runtime: RuntimeState,
    pub(crate) stats: StatsState,
    pub(crate) persistence: PersistenceState,
    pub(crate) ui_prefs: UiPrefsState,
    pub(crate) ui_session: UiSessionState,
    pub(crate) ui: UiState,
}

impl WgApp {
    pub(crate) fn new(engine: Engine, theme_mode: ThemeMode) -> Self {
        let ui_prefs = UiPrefsState::new(theme_mode);
        Self {
            engine,
            configs: ConfigsState::new(),
            selection: SelectionState::new(),
            editor: EditorState::new(),
            runtime: RuntimeState::new(),
            stats: StatsState::new(),
            persistence: PersistenceState::new(),
            ui_session: UiSessionState::from_prefs(&ui_prefs),
            ui_prefs,
            ui: UiState::new(),
        }
    }

    pub(crate) fn current_configs_inspector_tab(
        &self,
        cx: &mut gpui::Context<Self>,
    ) -> ConfigInspectorTab {
        self.ui
            .configs_workspace
            .as_ref()
            .map(|workspace| workspace.read(cx).inspector_tab)
            .unwrap_or(self.ui_prefs.preferred_inspector_tab)
    }

    pub(crate) fn persist_preferred_inspector_tab(
        &mut self,
        value: ConfigInspectorTab,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        if self.ui_prefs.preferred_inspector_tab == value {
            return false;
        }
        self.ui_prefs.preferred_inspector_tab = value;
        self.persist_state_async(cx);
        true
    }

    pub(crate) fn persist_configs_panel_widths(
        &mut self,
        library_width: f32,
        inspector_width: f32,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        let library_width = library_width.clamp(240.0, 420.0);
        let inspector_width = inspector_width.clamp(280.0, 440.0);
        if self.ui_prefs.configs_library_width == library_width
            && self.ui_prefs.configs_inspector_width == inspector_width
        {
            return false;
        }
        self.ui_prefs.configs_library_width = library_width;
        self.ui_prefs.configs_inspector_width = inspector_width;
        self.persist_state_async(cx);
        true
    }

    pub(crate) fn set_theme_mode_pref(
        &mut self,
        value: ThemeMode,
        window: Option<&mut Window>,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.ui_prefs.theme_mode != value {
            self.ui_prefs.theme_mode = value;
            let refresh_all_windows = window.is_none();
            Theme::change(value, window, cx);
            if refresh_all_windows {
                cx.refresh_windows();
            }
            self.persist_state_async(cx);
        }
        cx.notify();
    }

    pub(crate) fn set_log_auto_follow_pref(&mut self, value: bool, cx: &mut gpui::Context<Self>) {
        if self.ui_prefs.log_auto_follow != value {
            self.ui_prefs.log_auto_follow = value;
            self.persist_state_async(cx);
        }
        cx.notify();
    }

    pub(crate) fn set_dns_mode_pref(&mut self, value: DnsMode, cx: &mut gpui::Context<Self>) {
        if self.ui_prefs.dns_mode != value {
            self.ui_prefs.dns_mode = value;
            self.persist_state_async(cx);
        }
        cx.notify();
    }

    pub(crate) fn set_dns_preset_pref(&mut self, value: DnsPreset, cx: &mut gpui::Context<Self>) {
        if self.ui_prefs.dns_preset != value {
            self.ui_prefs.dns_preset = value;
            self.persist_state_async(cx);
        }
        cx.notify();
    }

    pub(crate) fn set_preferred_inspector_tab(
        &mut self,
        value: ConfigInspectorTab,
        cx: &mut gpui::Context<Self>,
    ) {
        self.persist_preferred_inspector_tab(value, cx);
        if let Some(workspace) = self.ui.configs_workspace.clone() {
            let _ = workspace.update(cx, |workspace, cx| {
                if workspace.set_inspector_tab(value) {
                    cx.notify();
                }
            });
        }
        cx.notify();
    }

    pub(crate) fn set_preferred_traffic_period(
        &mut self,
        value: TrafficPeriod,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.ui_prefs.preferred_traffic_period != value {
            self.ui_prefs.preferred_traffic_period = value;
            self.persist_state_async(cx);
        }
        self.ui_session.traffic_period = value;
        cx.notify();
    }

    pub(crate) fn set_session_traffic_period(
        &mut self,
        value: TrafficPeriod,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.ui_session.traffic_period != value {
            self.ui_session.traffic_period = value;
            cx.notify();
        }
    }

    pub(crate) fn set_proxies_view_mode_pref(
        &mut self,
        value: ProxiesViewMode,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.ui_prefs.proxies_view_mode != value {
            self.ui_prefs.proxies_view_mode = value;
            self.persist_state_async(cx);
        }
        cx.notify();
    }

    pub(crate) fn set_sidebar_active(&mut self, value: SidebarItem, cx: &mut gpui::Context<Self>) {
        if self.ui_session.sidebar_active != value {
            self.ui_session.sidebar_active = value;
            cx.notify();
        }
    }

    pub(crate) fn push_success_toast(
        &mut self,
        message: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        window.push_notification(Notification::success(message.into()), cx);
    }
}

fn init_rate_history() -> VecDeque<f32> {
    // 预填充 0，保持曲线长度稳定。
    let mut history = VecDeque::with_capacity(SPARKLINE_SAMPLES);
    for _ in 0..SPARKLINE_SAMPLES {
        history.push_back(0.0);
    }
    history
}
