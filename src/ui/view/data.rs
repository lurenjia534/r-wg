use std::collections::{HashMap, HashSet};

use chrono::{Duration as ChronoDuration, Local, NaiveDate};
use r_wg::backend::wg::config;

use super::super::format::{
    format_bytes, format_duration, format_route_table, summarize_peers, PeerSummary,
};
use super::super::state::{
    ConfigDraftState, DraftValidationState, EditorOperation, TrafficDayStats, TrafficHour,
    TrafficPeriod, TunnelConfig, WgApp,
    TRAFFIC_TREND_DAYS,
};

/// `ConfigStatus` 是配置编辑页和顶部工具栏共享的“解析结果快照”。
///
/// 这里不直接暴露布尔值，而是提前整理成 UI 直接可消费的 label + color：
/// - 这样视图层不需要知道 `Loading / Invalid / Valid / Unknown` 的判定细节；
/// - 也避免多个面板各自复制一份状态映射逻辑。
/// 配置解析状态，用于显示“Valid/Invalid”徽标。
pub(crate) struct ConfigStatus {
    pub(crate) label: &'static str,
    pub(crate) tone: ConfigStatusTone,
}

pub(crate) enum ConfigStatusTone {
    Success,
    Danger,
    Warning,
    Secondary,
}

/// Configs 页的局部渲染快照。
///
/// 这个 ViewModel 的目标不是减少字段数量，而是把 “Configs 页需要的 editor 状态”
/// 从 `WgApp` 根状态里显式裁一份出来：
/// - 视图层后续只依赖这份快照，不再散落读取 `app.editor.*`；
/// - 等 `draft / operation` 真正迁进 `ConfigsWorkspace` 时，Configs 视图不需要再改一轮。
pub(crate) struct ConfigsViewData {
    pub(crate) shared: ViewData,
    pub(crate) draft: ConfigDraftState,
    pub(crate) has_selection: bool,
    pub(crate) is_busy: bool,
    pub(crate) has_saved_source: bool,
    pub(crate) is_running_draft: bool,
    pub(crate) can_save: bool,
    pub(crate) can_restart: bool,
    pub(crate) title: String,
    pub(crate) source_summary: String,
    pub(crate) runtime_note: String,
}

/// `ViewData` 是“跨页面复用”的基础派生数据。
///
/// 它只放多个页面都会读到的结果：
/// - 当前选中配置的解析结果；
/// - 配置解析状态徽标；
/// - peer 统计摘要与最近握手文本。
///
/// 换句话说，`ViewData` 解决的是“全局共享的只读派生信息”，
/// 而不是某个页面自己的展示模型。
/// 渲染所需的派生数据，集中在这里计算，避免散落在各面板。
pub(crate) struct ViewData {
    /// 解析后的配置（若无选中或解析失败则为 None）。
    pub(crate) parsed_config: Option<config::WireGuardConfig>,
    /// 配置解析错误文本。
    pub(crate) parse_error: Option<String>,
    /// 状态徽标（有效/无效）。
    pub(crate) config_status: Option<ConfigStatus>,
    /// 当前 draft 是否有未保存改动。
    pub(crate) draft_dirty: bool,
    /// 当前 draft 是否对应已保存配置。
    pub(crate) has_saved_source: bool,
    /// 当前 draft 是否需要重启运行中的隧道。
    pub(crate) needs_restart: bool,
    /// 统计摘要（总流量/握手时间等）。
    pub(crate) peer_summary: PeerSummary,
    /// 最近握手时间的可读文本。
    pub(crate) last_handshake: String,
}

/// `OverviewData` 是 Overview 页的专属 ViewModel。
///
/// 它的目标很明确：把 Overview 页里所有“从状态取值 -> 计算口径 -> 格式化文案”
/// 的工作，尽量在进入视图前一次性做完，让 `render_overview` 只负责拼装页面。
///
/// 这个模型里既包含纯文本，也包含图表直接使用的序列数据：
/// - 文本类：速度、总流量、DNS、Endpoint、Route、内存、Uptime；
/// - 图表类：上传/下载 sparkline 序列、7 日趋势、Traffic Summary；
/// - 控制类：当前运行态、当前选择的流量周期。
pub(crate) struct OverviewData {
    /// 从隧道启动时间换算出来的运行时长文案，例如 `12:34` 或 `1:02`。
    pub(crate) uptime_text: String,
    /// 当前进程常驻内存文案；读取失败时为 `-`。
    pub(crate) memory_text: String,
    /// Peer 统计聚合后的累计下载总量（RX）。
    pub(crate) rx_total_text: String,
    /// Peer 统计聚合后的累计上传总量（TX）。
    pub(crate) tx_total_text: String,
    /// Peer 数量文本，供状态卡片直接展示。
    pub(crate) peer_count_text: String,
    /// 最近握手时间文本，已经过 `format_duration` 处理。
    pub(crate) handshake_text: String,
    /// 当前上传速率文案；当隧道未运行时固定回落为 0。
    pub(crate) upload_speed_text: String,
    /// 当前下载速率文案；当隧道未运行时固定回落为 0。
    pub(crate) download_speed_text: String,
    /// 上传累计总量文本，用于 Traffic Stats 卡片底部。
    pub(crate) upload_total_text: String,
    /// 下载累计总量文本，用于 Traffic Stats 卡片底部。
    pub(crate) download_total_text: String,
    /// 上传 sparkline 原始采样值，视图层只负责转成 chart point。
    pub(crate) upload_series: Vec<f32>,
    /// 下载 sparkline 原始采样值，视图层只负责转成 chart point。
    pub(crate) download_series: Vec<f32>,
    /// 当前配置的本地地址摘要。
    pub(crate) local_ip_text: String,
    /// 当前配置的首个 DNS 服务器摘要。
    pub(crate) dns_text: String,
    /// 当前配置的首个 peer endpoint 摘要。
    pub(crate) endpoint_text: String,
    /// Allowed IPs 的汇总结果，按“路由条数”展示。
    pub(crate) allowed_text: String,
    /// 当前运行中的隧道名称；未运行时为 `-`。
    pub(crate) network_name_text: String,
    /// 配置中的 route table 文本。
    pub(crate) route_table_text: String,
    /// 当前是否处于运行态。Overview 页只消费这个布尔值，不再直接读 `app.runtime`。
    pub(crate) is_running: bool,
    /// Traffic Summary 当前选中的统计周期。
    pub(crate) traffic_period: TrafficPeriod,
    /// 7 日趋势图所需的完整数据集。
    pub(crate) traffic_trend: TrafficTrendData,
    /// Traffic Summary 区块所需的完整数据集。
    pub(crate) traffic_summary: TrafficSummaryData,
}

/// 趋势图上的单个点。
///
/// `label` 和 `is_today` 都属于“显示语义”：
/// - `label` 决定横轴文字；
/// - `is_today` 决定柱形是否高亮。
#[derive(Clone)]
pub(crate) struct TrafficTrendPoint {
    pub(crate) label: String,
    pub(crate) bytes: u64,
    pub(crate) is_today: bool,
}

/// Overview 页 7 日趋势图的完整输入。
///
/// `average_bytes` 是整段窗口的日均值，不是“有流量那几天”的平均值。
/// 也就是说，零流量天数会参与平均，这样图上的平均线才符合用户对“最近 7 天”
/// 的直觉。
pub(crate) struct TrafficTrendData {
    pub(crate) points: Vec<TrafficTrendPoint>,
    pub(crate) average_bytes: f64,
}

/// Traffic Summary 区块的完整输入。
///
/// `total_rx/total_tx` 表示当前周期内的总体流量；
/// `ranked` 则是按配置聚合后的排行列表，已经做过筛零、排序和截断。
#[derive(Clone)]
pub(crate) struct TrafficSummaryData {
    pub(crate) total_rx: u64,
    pub(crate) total_tx: u64,
    pub(crate) ranked: Vec<TrafficRankItem>,
}

/// Traffic Summary 中单个配置的流量排行项。
#[derive(Clone)]
pub(crate) struct TrafficRankItem {
    pub(crate) name: String,
    pub(crate) rx_bytes: u64,
    pub(crate) tx_bytes: u64,
}

impl TrafficRankItem {
    /// 排行以总流量为准，因此这里提供统一的 `rx + tx` 口径。
    pub(crate) fn total_bytes(&self) -> u64 {
        self.rx_bytes.saturating_add(self.tx_bytes)
    }
}

impl ViewData {
    /// 从应用状态构造渲染数据。
    pub(crate) fn new(app: &WgApp) -> Self {
        Self::from_editor(app, &app.editor.draft, app.editor.operation.as_ref())
    }

    /// 从显式传入的 editor 快照构造共享派生数据。
    pub(crate) fn from_editor(
        app: &WgApp,
        draft: &ConfigDraftState,
        operation: Option<&EditorOperation>,
    ) -> Self {
        let is_loading = matches!(operation, Some(EditorOperation::LoadingConfig));
        let (parsed_config, parse_error, config_status) = if is_loading {
            (
                None,
                None,
                Some(ConfigStatus {
                    label: "Loading",
                    tone: ConfigStatusTone::Secondary,
                }),
            )
        } else {
            match &draft.validation {
                DraftValidationState::Idle => {
                    if draft.name.is_empty() && draft.text.is_empty() {
                        (None, None, None)
                    } else {
                        (
                            None,
                            None,
                            Some(ConfigStatus {
                                label: "Draft",
                                tone: ConfigStatusTone::Warning,
                            }),
                        )
                    }
                }
                DraftValidationState::Valid { parsed, .. } => (
                    Some(parsed.clone()),
                    None,
                    Some(ConfigStatus {
                        label: "Valid",
                        tone: ConfigStatusTone::Success,
                    }),
                ),
                DraftValidationState::Invalid { line, message, .. } => (
                    None,
                    Some(match line {
                        Some(line) => format!("line {line}: {message}"),
                        None => message.to_string(),
                    }),
                    Some(ConfigStatus {
                        label: "Invalid",
                        tone: ConfigStatusTone::Danger,
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
            config_status,
            draft_dirty: draft.is_dirty(),
            has_saved_source: draft.source_id.is_some(),
            needs_restart: draft.needs_restart,
            peer_summary,
            last_handshake,
        }
    }
}

impl ConfigsViewData {
    pub(crate) fn from_editor(
        app: &WgApp,
        draft: ConfigDraftState,
        operation: Option<EditorOperation>,
        has_selection: bool,
    ) -> Self {
        let shared = ViewData::from_editor(app, &draft, operation.as_ref());
        let has_saved_source = draft.source_id.is_some();
        let is_busy = operation.is_some();
        let is_running_draft = app.runtime.running && app.runtime.running_id == draft.source_id;
        let source_summary = draft
            .source_id
            .and_then(|id| app.configs.get_by_id(id))
            .map(config_origin_summary)
            .unwrap_or_else(|| "Unsaved draft".to_string());
        let title = current_draft_title(&draft);
        let runtime_note = if is_running_draft && draft.is_dirty() {
            "Editing a running tunnel. Saved changes take effect after restart.".to_string()
        } else if is_running_draft {
            "This tunnel is currently running.".to_string()
        } else {
            "Changes affect the saved config after you save them.".to_string()
        };
        let can_save = !is_busy
            && !matches!(draft.validation, DraftValidationState::Idle)
            && (draft.is_dirty() || !has_saved_source);
        let can_restart = !is_busy
            && is_running_draft
            && draft.is_dirty()
            && matches!(draft.validation, DraftValidationState::Valid { .. });

        Self {
            shared,
            draft,
            has_selection,
            is_busy,
            has_saved_source,
            is_running_draft,
            can_save,
            can_restart,
            title,
            source_summary,
            runtime_note,
        }
    }
}

impl OverviewData {
    /// 构造 Overview 页的完整展示模型。
    ///
    /// 这里有两个输入：
    /// - `app`：提供运行态、速率历史、流量汇总原始数据；
    /// - `data`：提供已经共享计算好的解析结果和 peer 摘要。
    ///
    /// 这么分层的原因是：
    /// - `ViewData` 继续承担“跨页面共享”；
    /// - `OverviewData` 只承担 “Overview 页额外需要的页面级派生”。
    pub(crate) fn new(app: &WgApp, data: &ViewData) -> Self {
        Self {
            uptime_text: format_uptime(app.stats.started_at),
            memory_text: format_memory_usage(),
            rx_total_text: format_bytes(data.peer_summary.rx_bytes),
            tx_total_text: format_bytes(data.peer_summary.tx_bytes),
            peer_count_text: data.peer_summary.peer_count.to_string(),
            handshake_text: data.last_handshake.clone(),
            upload_speed_text: format_speed_text(app.runtime.running, app.stats.tx_rate_bps),
            download_speed_text: format_speed_text(app.runtime.running, app.stats.rx_rate_bps),
            upload_total_text: format_bytes(data.peer_summary.tx_bytes),
            download_total_text: format_bytes(data.peer_summary.rx_bytes),
            upload_series: app.stats.tx_rate_history.iter().copied().collect(),
            download_series: app.stats.rx_rate_history.iter().copied().collect(),
            local_ip_text: format_local_ip(data),
            dns_text: format_dns(data),
            endpoint_text: format_endpoint(data),
            allowed_text: format_allowed_summary(data),
            network_name_text: app
                .runtime
                .running_name
                .clone()
                .unwrap_or_else(|| "-".to_string()),
            route_table_text: data
                .parsed_config
                .as_ref()
                .map(|cfg| format_route_table(cfg.interface.table))
                .unwrap_or_else(|| "-".to_string()),
            is_running: app.runtime.running,
            traffic_period: app.ui_session.traffic_period,
            traffic_trend: build_traffic_trend(app),
            traffic_summary: build_traffic_summary(app),
        }
    }
}

/// 构造 Traffic Summary 的入口。
///
/// 使用当前本地时间作为“统计窗口基准点”：
/// - Today: 最近 24 小时；
/// - ThisMonth: 过去 30 天；
/// - LastMonth: 再往前 30 天。
fn build_traffic_summary(app: &WgApp) -> TrafficSummaryData {
    const MAX_RANK_ITEMS: usize = 7;
    let now = Local::now();
    build_traffic_summary_at(
        app,
        now.date_naive(),
        now.timestamp() / 3600,
        MAX_RANK_ITEMS,
    )
}

fn build_traffic_summary_at(
    app: &WgApp,
    today: NaiveDate,
    current_hour: i64,
    max_rank_items: usize,
) -> TrafficSummaryData {
    // 这里的统计口径有两层：
    // 1. 先按当前 period 求整体 total_rx / total_tx；
    // 2. 再按配置分别求同周期总量，用于排行。
    //
    // 排行里会跳过零流量项，避免“空配置”挤占榜单空间。
    let (total_rx, total_tx, ranked) = match app.ui_session.traffic_period {
        TrafficPeriod::Today => {
            // Today 采用滚动 24 小时窗口，而不是“从当天 00:00 到现在”。
            let min_hour = current_hour.saturating_sub(23);
            let (total_rx, total_tx) = sum_hours(&app.stats.traffic_hours, min_hour, current_hour);
            let ranked = app
                .configs
                .iter()
                .filter_map(|cfg| {
                    let hours = app.stats.config_traffic_hours.get(&cfg.id)?;
                    let (rx, tx) = sum_hours(hours, min_hour, current_hour);
                    let total = rx.saturating_add(tx);
                    if total == 0 {
                        None
                    } else {
                        Some(TrafficRankItem {
                            name: cfg.name.clone(),
                            rx_bytes: rx,
                            tx_bytes: tx,
                        })
                    }
                })
                .collect::<Vec<_>>();
            (total_rx, total_tx, ranked)
        }
        TrafficPeriod::ThisMonth => {
            // ThisMonth 口径：包含 today 在内的最近 30 个自然日。
            let dates = build_date_set(today, 0, 30);
            let (total_rx, total_tx) = sum_days(&app.stats.traffic_days_v2, &dates);
            let ranked = app
                .configs
                .iter()
                .filter_map(|cfg| {
                    let days = app.stats.config_traffic_days.get(&cfg.id)?;
                    let (rx, tx) = sum_days(days, &dates);
                    let total = rx.saturating_add(tx);
                    if total == 0 {
                        None
                    } else {
                        Some(TrafficRankItem {
                            name: cfg.name.clone(),
                            rx_bytes: rx,
                            tx_bytes: tx,
                        })
                    }
                })
                .collect::<Vec<_>>();
            (total_rx, total_tx, ranked)
        }
        TrafficPeriod::LastMonth => {
            // LastMonth 口径：排除最近 30 天，取再往前的 30 个自然日。
            let dates = build_date_set(today, 30, 30);
            let (total_rx, total_tx) = sum_days(&app.stats.traffic_days_v2, &dates);
            let ranked = app
                .configs
                .iter()
                .filter_map(|cfg| {
                    let days = app.stats.config_traffic_days.get(&cfg.id)?;
                    let (rx, tx) = sum_days(days, &dates);
                    let total = rx.saturating_add(tx);
                    if total == 0 {
                        None
                    } else {
                        Some(TrafficRankItem {
                            name: cfg.name.clone(),
                            rx_bytes: rx,
                            tx_bytes: tx,
                        })
                    }
                })
                .collect::<Vec<_>>();
            (total_rx, total_tx, ranked)
        }
    };

    // 最终排行统一按总流量倒序，并截断到 UI 预设数量。
    let mut ranked = ranked;
    ranked.sort_by(|a, b| b.total_bytes().cmp(&a.total_bytes()));
    ranked.truncate(max_rank_items);

    TrafficSummaryData {
        total_rx,
        total_tx,
        ranked,
    }
}

/// 构造一个日期集合，供“按天”统计窗口过滤使用。
///
/// 例如：
/// - `start_offset = 0, days = 30` 表示最近 30 天；
/// - `start_offset = 30, days = 30` 表示再往前 30 天。
fn build_date_set(today: NaiveDate, start_offset: i64, days: i64) -> HashSet<String> {
    let mut set = HashSet::with_capacity(days as usize);
    for offset in start_offset..start_offset + days {
        let date = today - ChronoDuration::days(offset);
        set.insert(date.format("%Y-%m-%d").to_string());
    }
    set
}

/// 在给定日期集合里，累计按天统计的 RX/TX。
fn sum_days(days: &[TrafficDayStats], dates: &HashSet<String>) -> (u64, u64) {
    let mut rx = 0u64;
    let mut tx = 0u64;
    for day in days {
        if dates.contains(&day.date) {
            rx = rx.saturating_add(day.rx_bytes);
            tx = tx.saturating_add(day.tx_bytes);
        }
    }
    (rx, tx)
}

/// 在给定小时窗口里，累计按小时统计的 RX/TX。
fn sum_hours(hours: &[TrafficHour], min_hour: i64, max_hour: i64) -> (u64, u64) {
    let mut rx = 0u64;
    let mut tx = 0u64;
    for hour in hours {
        if hour.hour >= min_hour && hour.hour <= max_hour {
            rx = rx.saturating_add(hour.rx_bytes);
            tx = tx.saturating_add(hour.tx_bytes);
        }
    }
    (rx, tx)
}

/// 构造 7 日趋势图的入口。
fn build_traffic_trend(app: &WgApp) -> TrafficTrendData {
    build_traffic_trend_at(app, Local::now().date_naive())
}

fn build_traffic_trend_at(app: &WgApp, today: NaiveDate) -> TrafficTrendData {
    // 原始数据里同一天可能出现多条累计记录，这里先按日期合并，
    // 避免图表上同一天被重复计入。
    let mut by_date: HashMap<NaiveDate, u64> = HashMap::new();
    for day in &app.stats.traffic_days {
        if let Ok(date) = NaiveDate::parse_from_str(&day.date, "%Y-%m-%d") {
            let entry = by_date.entry(date).or_insert(0);
            *entry = entry.saturating_add(day.bytes);
        }
    }

    // 图表始终展示固定长度的最近 7 天：
    // - 没有流量的日期也会补 0；
    // - 最后一个点永远是 today，并打上高亮标记。
    let mut points = Vec::with_capacity(TRAFFIC_TREND_DAYS);
    for offset in (0..TRAFFIC_TREND_DAYS).rev() {
        let date = today - ChronoDuration::days(offset as i64);
        let bytes = by_date.get(&date).copied().unwrap_or(0);
        let label = date.format("%a").to_string();
        points.push(TrafficTrendPoint {
            label,
            bytes,
            is_today: offset == 0,
        });
    }

    // 平均值按“固定 7 天窗口”计算，而不是按“非零流量天数”计算。
    let total: u64 = points.iter().map(|point| point.bytes).sum();
    let average_bytes = total as f64 / TRAFFIC_TREND_DAYS as f64;

    TrafficTrendData {
        points,
        average_bytes,
    }
}

/// 把启动时间转成 Overview 卡片显示的 uptime 文案。
///
/// 格式策略：
/// - 小于 1 小时：`分:秒`
/// - 大于等于 1 小时：`时:分`
fn format_uptime(started_at: Option<std::time::Instant>) -> String {
    let Some(start) = started_at else {
        return "0:00".to_string();
    };
    let elapsed = start.elapsed();
    let total_secs = elapsed.as_secs();
    let minutes = total_secs / 60;
    let seconds = total_secs % 60;
    if minutes >= 60 {
        let hours = minutes / 60;
        let mins = minutes % 60;
        format!("{hours}:{mins:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// 把原始字节速率转成文本。
///
/// 这里显式传入 `is_running`，是为了把“未运行时强制显示 0”这个 UI 约束
/// 写死在 ViewModel 层，而不是留给视图层自己判断。
fn format_speed_text(is_running: bool, bytes_per_sec: f64) -> String {
    if !is_running {
        return "0.0 KB/s".to_string();
    }
    format_speed(bytes_per_sec)
}

/// 字节速率格式化工具。
fn format_speed(bytes_per_sec: f64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * KB;
    if bytes_per_sec >= MB {
        format!("{:.1} MB/s", bytes_per_sec / MB)
    } else if bytes_per_sec >= KB {
        format!("{:.1} KB/s", bytes_per_sec / KB)
    } else {
        format!("{:.0} B/s", bytes_per_sec)
    }
}

fn current_draft_title(draft: &ConfigDraftState) -> String {
    let name = draft.name.as_ref().trim();
    if !name.is_empty() {
        return name.to_string();
    }
    if draft.source_id.is_none() {
        return "New Draft".to_string();
    }
    "Untitled Config".to_string()
}

fn config_origin_summary(config: &TunnelConfig) -> String {
    match &config.source {
        super::super::state::ConfigSource::File { origin_path } => origin_path
            .as_ref()
            .map(|path| format!("Imported from {}", path.display()))
            .unwrap_or_else(|| "Imported config".to_string()),
        super::super::state::ConfigSource::Paste => "Created in app storage".to_string(),
    }
}

/// 从解析后的配置里提取第一个本地地址。
fn format_local_ip(data: &ViewData) -> String {
    data.parsed_config
        .as_ref()
        .and_then(|cfg| cfg.interface.addresses.first())
        .map(|addr| format!("{}/{}", addr.addr, addr.cidr))
        .unwrap_or_else(|| "-".to_string())
}

/// 从解析后的配置里提取第一个 DNS 服务器。
fn format_dns(data: &ViewData) -> String {
    data.parsed_config
        .as_ref()
        .and_then(|cfg| cfg.interface.dns_servers.first())
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "-".to_string())
}

/// 从解析后的配置里提取第一个 peer endpoint。
fn format_endpoint(data: &ViewData) -> String {
    data.parsed_config
        .as_ref()
        .and_then(|cfg| cfg.peers.first())
        .and_then(|peer| peer.endpoint.as_ref())
        .map(|endpoint| format!("{}:{}", endpoint.host, endpoint.port))
        .unwrap_or_else(|| "-".to_string())
}

/// 统计配置里所有 peer 的 allowed IPs 总数，并转成概要文本。
fn format_allowed_summary(data: &ViewData) -> String {
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

/// 读取当前进程常驻内存并格式化为文案。
fn format_memory_usage() -> String {
    match read_process_rss_bytes() {
        Some(bytes) => format_memory(bytes),
        None => "-".to_string(),
    }
}

/// 读取当前进程 RSS。
///
/// 这里故意保留在 ViewModel 层，而不是视图层里现算：
/// - 视图只消费“已经格式化好的文案”；
/// - 平台差异（Linux `/proc`、Windows API）被封在这一层。
fn read_process_rss_bytes() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("VmRSS:") {
                let mut parts = rest.split_whitespace();
                let kb = parts.next()?.parse::<u64>().ok()?;
                return Some(kb.saturating_mul(1024));
            }
        }
        return None;
    }

    #[cfg(target_os = "windows")]
    {
        use windows::Win32::System::ProcessStatus::{
            GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
        };
        use windows::Win32::System::Threading::GetCurrentProcess;

        let process = unsafe { GetCurrentProcess() };
        let mut counters = PROCESS_MEMORY_COUNTERS::default();
        counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;

        unsafe {
            GetProcessMemoryInfo(process, &mut counters, counters.cb).ok()?;
        }
        return Some(counters.WorkingSetSize as u64);
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        None
    }
}

/// 把字节数格式化成较稳定的内存展示文案。
fn format_memory(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * KB;
    const GB: f64 = 1024.0 * MB;

    let value = bytes as f64;
    if value >= GB {
        format!("{:.1} GB", value / GB)
    } else if value >= MB {
        format!("{:.0} MB", value / MB)
    } else if value >= KB {
        format!("{:.0} KB", value / KB)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use chrono::NaiveDate;
    use gpui_component::theme::ThemeMode;

    use super::{build_traffic_summary_at, build_traffic_trend_at};
    use crate::ui::state::{
        ConfigSource, EndpointFamily, TrafficDay, TrafficDayStats, TrafficHour, TrafficPeriod,
        TunnelConfig, WgApp, TRAFFIC_TREND_DAYS,
    };

    fn make_app() -> WgApp {
        WgApp::new(r_wg::backend::wg::Engine::new(), ThemeMode::Dark)
    }

    fn make_config(id: u64, name: &str) -> TunnelConfig {
        TunnelConfig {
            id,
            name: name.to_string(),
            name_lower: name.to_ascii_lowercase(),
            text: None,
            source: ConfigSource::Paste,
            storage_path: PathBuf::from(format!("/tmp/{id}.conf")),
            endpoint_family: EndpointFamily::Unknown,
        }
    }

    #[test]
    fn traffic_summary_today_uses_last_24_hours_and_sorts_rankings() {
        let mut app = make_app();
        let current_hour = 1_000;
        app.ui_session.traffic_period = TrafficPeriod::Today;
        app.configs.configs = vec![make_config(1, "alpha"), make_config(2, "beta")];
        app.stats.traffic_hours = vec![
            TrafficHour {
                hour: current_hour,
                rx_bytes: 50,
                tx_bytes: 5,
            },
            TrafficHour {
                hour: current_hour - 23,
                rx_bytes: 10,
                tx_bytes: 1,
            },
            TrafficHour {
                hour: current_hour - 24,
                rx_bytes: 999,
                tx_bytes: 999,
            },
        ];
        app.stats.config_traffic_hours.insert(
            1,
            vec![
                TrafficHour {
                    hour: current_hour,
                    rx_bytes: 20,
                    tx_bytes: 10,
                },
                TrafficHour {
                    hour: current_hour - 24,
                    rx_bytes: 500,
                    tx_bytes: 500,
                },
            ],
        );
        app.stats.config_traffic_hours.insert(
            2,
            vec![TrafficHour {
                hour: current_hour - 1,
                rx_bytes: 40,
                tx_bytes: 20,
            }],
        );

        let summary = build_traffic_summary_at(
            &app,
            NaiveDate::from_ymd_opt(2026, 3, 6).expect("valid test date"),
            current_hour,
            7,
        );

        assert_eq!(summary.total_rx, 60);
        assert_eq!(summary.total_tx, 6);
        assert_eq!(summary.ranked.len(), 2);
        assert_eq!(summary.ranked[0].name, "beta");
        assert_eq!(summary.ranked[0].total_bytes(), 60);
        assert_eq!(summary.ranked[1].name, "alpha");
        assert_eq!(summary.ranked[1].total_bytes(), 30);
    }

    #[test]
    fn traffic_summary_month_windows_split_current_and_previous_periods() {
        let today = NaiveDate::from_ymd_opt(2026, 3, 6).expect("valid test date");
        let mut app = make_app();
        app.configs.configs = vec![make_config(7, "alpha")];
        app.stats.traffic_days_v2 = vec![
            TrafficDayStats {
                date: "2026-03-06".to_string(),
                rx_bytes: 10,
                tx_bytes: 20,
            },
            TrafficDayStats {
                date: "2026-02-10".to_string(),
                rx_bytes: 30,
                tx_bytes: 40,
            },
            TrafficDayStats {
                date: "2026-01-20".to_string(),
                rx_bytes: 500,
                tx_bytes: 600,
            },
        ];
        app.stats.config_traffic_days.insert(
            7,
            vec![
                TrafficDayStats {
                    date: "2026-03-06".to_string(),
                    rx_bytes: 10,
                    tx_bytes: 20,
                },
                TrafficDayStats {
                    date: "2026-02-10".to_string(),
                    rx_bytes: 30,
                    tx_bytes: 40,
                },
                TrafficDayStats {
                    date: "2026-01-20".to_string(),
                    rx_bytes: 500,
                    tx_bytes: 600,
                },
            ],
        );

        app.ui_session.traffic_period = TrafficPeriod::ThisMonth;
        let this_month = build_traffic_summary_at(&app, today, 0, 7);
        assert_eq!(this_month.total_rx, 40);
        assert_eq!(this_month.total_tx, 60);
        assert_eq!(this_month.ranked[0].total_bytes(), 100);

        app.ui_session.traffic_period = TrafficPeriod::LastMonth;
        let last_month = build_traffic_summary_at(&app, today, 0, 7);
        assert_eq!(last_month.total_rx, 500);
        assert_eq!(last_month.total_tx, 600);
        assert_eq!(last_month.ranked[0].total_bytes(), 1100);
    }

    #[test]
    fn traffic_trend_aggregates_same_day_entries_and_marks_today() {
        let today = NaiveDate::from_ymd_opt(2026, 3, 6).expect("valid test date");
        let mut app = make_app();
        app.stats.traffic_days = vec![
            TrafficDay {
                date: "2026-03-06".to_string(),
                bytes: 100,
            },
            TrafficDay {
                date: "2026-03-06".to_string(),
                bytes: 50,
            },
            TrafficDay {
                date: "2026-03-04".to_string(),
                bytes: 30,
            },
            TrafficDay {
                date: "2026-02-20".to_string(),
                bytes: 999,
            },
        ];

        let trend = build_traffic_trend_at(&app, today);

        assert_eq!(trend.points.len(), TRAFFIC_TREND_DAYS);
        let today_point = trend.points.last().expect("today should be present");
        assert!(today_point.is_today);
        assert_eq!(today_point.bytes, 150);
    }
}
