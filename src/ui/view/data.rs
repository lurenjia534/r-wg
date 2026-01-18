use r_wg::backend::wg::config;

use super::super::format::{format_duration, summarize_peers, PeerSummary};
use super::super::state::WgApp;

/// 配置解析状态，用于显示“Valid/Invalid”徽标。
pub(crate) struct ConfigStatus {
    pub(crate) label: &'static str,
    pub(crate) color: u32,
}

/// 渲染所需的派生数据，集中在这里计算，避免散落在各面板。
pub(crate) struct ViewData {
    /// 解析后的配置（若无选中或解析失败则为 None）。
    pub(crate) parsed_config: Option<config::WireGuardConfig>,
    /// 配置解析错误文本。
    pub(crate) parse_error: Option<String>,
    /// 状态徽标（有效/无效）。
    pub(crate) config_status: Option<ConfigStatus>,
    /// 统计摘要（总流量/握手时间等）。
    pub(crate) peer_summary: PeerSummary,
    /// 最近握手时间的可读文本。
    pub(crate) last_handshake: String,
    /// 左侧栏“Running/Idle”显示文本。
    pub(crate) running_label: String,
}

impl ViewData {
    /// 从应用状态构造渲染数据。
    pub(crate) fn new(app: &WgApp) -> Self {
        let selected_config = app.selected_config();
        // 优先从解析缓存取结果：
        // - 缓存只针对当前选中配置；
        // - 避免在每次渲染时重复 parse_config。
        let (parsed_config, parse_error) = match (selected_config, app.parse_cache.as_ref()) {
            (Some(config), Some(cache)) if cache.name == config.name => {
                (cache.parsed.clone(), cache.error.clone())
            }
            _ => (None, None),
        };
        // 选中项处于“异步加载中”时显示 Loading 状态。
        let is_loading = app.selected.is_some() && app.loading_config == app.selected;

        // 解析状态的展示逻辑：
        // - Loading：文本尚未读完；
        // - Invalid：解析失败；
        // - Valid：解析成功；
        // - Unknown：已选中但暂未解析（例如文本还未加载）。
        let config_status = if is_loading {
            Some(ConfigStatus {
                label: "Loading",
                color: 0x64748b,
            })
        } else if parse_error.is_some() {
            Some(ConfigStatus {
                label: "Invalid",
                color: 0xb14f4a,
            })
        } else if selected_config.is_some() && parsed_config.is_some() {
            Some(ConfigStatus {
                label: "Valid",
                color: 0x3aa380,
            })
        } else if selected_config.is_some() {
            Some(ConfigStatus {
                label: "Unknown",
                color: 0x94a3b8,
            })
        } else {
            None
        };

        let peer_summary = summarize_peers(&app.peer_stats);
        let last_handshake = peer_summary
            .last_handshake
            .map(format_duration)
            .unwrap_or_else(|| "never".to_string());

        let running_label = match &app.running_name {
            Some(name) => format!("Running: {name}"),
            None => "Idle".to_string(),
        };

        Self {
            parsed_config,
            parse_error,
            config_status,
            peer_summary,
            last_handshake,
            running_label,
        }
    }
}
