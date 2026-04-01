use r_wg::core::config;

use crate::ui::features::configs::state::{
    ConfigDraftState, DraftValidationState, EditorOperation,
};
use crate::ui::format::{format_duration, summarize_peers};
use crate::ui::state::WgApp;

/// `ViewData` 是“跨页面复用”的基础派生数据。
///
/// 它只放多个页面都会读到的结果：
/// - 当前选中配置的解析结果；
/// - 配置解析状态徽标；
/// - 最近握手文本。
///
/// 换句话说，`ViewData` 解决的是“全局共享的只读派生信息”，
/// 而不是某个页面自己的展示模型。
/// 渲染所需的派生数据，集中在这里计算，避免散落在各面板。
pub(crate) struct ViewData {
    /// 解析后的配置（若无选中或解析失败则为 None）。
    pub(crate) parsed_config: Option<config::WireGuardConfig>,
    /// 配置解析错误文本。
    pub(crate) parse_error: Option<String>,
    /// 当前 draft 是否有未保存改动。
    pub(crate) draft_dirty: bool,
    /// 当前 draft 是否对应已保存配置。
    pub(crate) has_saved_source: bool,
    /// 当前 draft 是否需要重启运行中的隧道。
    pub(crate) needs_restart: bool,
    /// 最近握手时间的可读文本。
    pub(crate) last_handshake: String,
}

impl ViewData {
    /// 从显式传入的 editor 快照构造共享派生数据。
    pub(crate) fn from_editor(
        app: &WgApp,
        draft: &ConfigDraftState,
        operation: Option<&EditorOperation>,
    ) -> Self {
        let is_loading = matches!(operation, Some(EditorOperation::LoadingConfig));
        let (parsed_config, parse_error) = if is_loading {
            (None, None)
        } else {
            match &draft.validation {
                DraftValidationState::Idle => (None, None),
                DraftValidationState::Valid { parsed, .. } => (Some(parsed.clone()), None),
                DraftValidationState::Invalid { line, message, .. } => (
                    None,
                    Some(match line {
                        Some(line) => format!("line {line}: {message}"),
                        None => message.to_string(),
                    }),
                ),
            }
        };

        let peer_summary = summarize_peers(&app.stats.peer_stats);
        let last_handshake = peer_summary
            .last_handshake
            .map(format_duration)
            .unwrap_or_else(|| "never".to_string());

        Self {
            parsed_config,
            parse_error,
            draft_dirty: draft.is_dirty(),
            has_saved_source: draft.source_id.is_some(),
            needs_restart: draft.needs_restart,
            last_handshake,
        }
    }
}

/// 从解析后的配置里提取第一个本地地址。
pub(crate) fn format_local_ip(data: &ViewData) -> String {
    data.parsed_config
        .as_ref()
        .and_then(|cfg| cfg.interface.addresses.first())
        .map(|addr| format!("{}/{}", addr.addr, addr.cidr))
        .unwrap_or_else(|| "-".to_string())
}

/// 从解析后的配置里提取第一个 DNS 服务器。
pub(crate) fn format_dns(data: &ViewData) -> String {
    data.parsed_config
        .as_ref()
        .and_then(|cfg| cfg.interface.dns_servers.first())
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "-".to_string())
}

/// 从解析后的配置里提取第一个 peer endpoint。
pub(crate) fn format_endpoint(data: &ViewData) -> String {
    data.parsed_config
        .as_ref()
        .and_then(|cfg| cfg.peers.first())
        .and_then(|peer| peer.endpoint.as_ref())
        .map(|endpoint| format!("{}:{}", endpoint.host, endpoint.port))
        .unwrap_or_else(|| "-".to_string())
}

/// 统计配置里所有 peer 的 allowed IPs 总数，并转成概要文本。
pub(crate) fn format_allowed_summary(data: &ViewData) -> String {
    let count = data
        .parsed_config
        .as_ref()
        .map(|cfg| {
            cfg.peers
                .iter()
                .map(|peer| peer.allowed_ips.len())
                .sum::<usize>()
        })
        .unwrap_or(0);
    if count == 0 {
        "-".to_string()
    } else {
        format!("{count} routes")
    }
}
