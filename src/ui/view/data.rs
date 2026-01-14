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
        let (parsed_config, parse_error) = match selected_config {
            Some(config) => match config::parse_config(&config.text) {
                Ok(config) => (Some(config), None),
                Err(err) => (None, Some(err.to_string())),
            },
            None => (None, None),
        };

        let config_status = if parse_error.is_some() {
            Some(ConfigStatus {
                label: "Invalid",
                color: 0xb14f4a,
            })
        } else if selected_config.is_some() {
            Some(ConfigStatus {
                label: "Valid",
                color: 0x3aa380,
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
