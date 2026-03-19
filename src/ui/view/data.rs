use std::collections::HashSet;

use chrono::{Duration as ChronoDuration, Local, NaiveDate};
use r_wg::backend::wg::config;

use super::super::format::{format_bytes, format_duration, format_route_table, summarize_peers};
use super::super::state::{
    day_key_from_date, ConfigDraftState, ConfigSource, ConfigsState, DraftValidationState,
    EditorOperation, StatsState, TrafficDayBucket, TrafficHourBucket, TrafficPeriod, TunnelConfig,
    WgApp, TRAFFIC_TREND_DAYS,
};

/// Configs 页的局部渲染快照。
///
/// 这个 ViewModel 的目标不是减少字段数量，而是把 “Configs 页需要的 editor 状态”
/// 从 `ConfigsWorkspace` 的局部状态里显式整理成一份视图快照：
/// - 视图层只依赖这份快照，不再散落读取页面状态；
/// - 页面级状态和共享派生信息也在这里清楚分层，避免渲染逻辑回退到根状态。
pub(crate) struct ConfigsViewData {
    pub(crate) shared: ViewData,
    pub(crate) draft: ConfigDraftState,
    pub(crate) has_selection: bool,
    pub(crate) title_editing: bool,
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
    /// 运行态 dashboard 数据。
    pub(crate) runtime: OverviewRuntimeData,
    /// 当前选中配置的预览数据。
    pub(crate) preview: OverviewPreviewData,
    /// Traffic Summary 当前选中的统计周期。
    pub(crate) traffic_period: TrafficPeriod,
    /// 7 日趋势图所需的完整数据集。
    pub(crate) traffic_trend: TrafficTrendData,
    /// Traffic Summary 区块所需的完整数据集。
    pub(crate) traffic_summary: TrafficSummaryData,
}

pub(crate) struct OverviewRuntimeData {
    pub(crate) is_running: bool,
    pub(crate) running_name_text: String,
    pub(crate) last_updated_text: String,
    pub(crate) uptime_text: String,
    pub(crate) memory_text: String,
    pub(crate) rx_total_text: String,
    pub(crate) tx_total_text: String,
    pub(crate) peer_count_text: String,
    pub(crate) handshake_text: String,
    pub(crate) upload_speed_text: String,
    pub(crate) download_speed_text: String,
    pub(crate) upload_total_text: String,
    pub(crate) download_total_text: String,
    pub(crate) upload_series: Vec<f32>,
    pub(crate) download_series: Vec<f32>,
}

pub(crate) struct OverviewPreviewData {
    pub(crate) has_selection: bool,
    pub(crate) selected_name_text: String,
    pub(crate) source_text: String,
    pub(crate) context_text: String,
    pub(crate) local_ip_text: String,
    pub(crate) dns_text: String,
    pub(crate) endpoint_text: String,
    pub(crate) allowed_text: String,
    pub(crate) route_table_text: String,
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

impl ConfigsViewData {
    pub(crate) fn from_editor(
        app: &WgApp,
        draft: ConfigDraftState,
        operation: Option<EditorOperation>,
        has_selection: bool,
        title_editing: bool,
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
            title_editing,
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
    pub(crate) fn new(app: &WgApp) -> Self {
        let now = Local::now();
        let peer_summary = summarize_peers(&app.stats.peer_stats);
        let last_handshake = peer_summary
            .last_handshake
            .map(format_duration)
            .unwrap_or_else(|| "never".to_string());
        let preview = build_overview_preview(app);

        Self {
            runtime: OverviewRuntimeData {
                is_running: app.runtime.running,
                running_name_text: app
                    .runtime
                    .running_name
                    .clone()
                    .unwrap_or_else(|| "-".to_string()),
                last_updated_text: format_last_updated(app.stats.last_stats_at),
                uptime_text: format_uptime(app.stats.started_at),
                memory_text: format_process_memory(app.stats.process_rss_bytes),
                rx_total_text: format_bytes(peer_summary.rx_bytes),
                tx_total_text: format_bytes(peer_summary.tx_bytes),
                peer_count_text: peer_summary.peer_count.to_string(),
                handshake_text: last_handshake,
                upload_speed_text: format_speed_text(app.runtime.running, app.stats.tx_rate_bps),
                download_speed_text: format_speed_text(app.runtime.running, app.stats.rx_rate_bps),
                upload_total_text: format_bytes(peer_summary.tx_bytes),
                download_total_text: format_bytes(peer_summary.rx_bytes),
                upload_series: app.stats.tx_rate_history.iter().copied().collect(),
                download_series: app.stats.rx_rate_history.iter().copied().collect(),
            },
            preview,
            traffic_period: app.ui_session.traffic_period,
            traffic_trend: app.stats.overview_traffic_trend(now.date_naive()),
            traffic_summary: app.stats.overview_traffic_summary(
                &app.configs,
                app.ui_session.traffic_period,
                now.date_naive(),
                now.timestamp() / 3600,
            ),
        }
    }
}

fn build_overview_preview(app: &WgApp) -> OverviewPreviewData {
    let selected = app.selected_config().cloned();
    let Some(selected) = selected else {
        return OverviewPreviewData {
            has_selection: false,
            selected_name_text: "No config selected".to_string(),
            source_text: "-".to_string(),
            context_text: "Pick a saved config to show a stable preview here.".to_string(),
            local_ip_text: "-".to_string(),
            dns_text: "-".to_string(),
            endpoint_text: "-".to_string(),
            allowed_text: "-".to_string(),
            route_table_text: "-".to_string(),
        };
    };

    let cached_text = app.peek_cached_config_text(&selected.storage_path);
    let parsed_config = selected
        .text
        .clone()
        .or(cached_text)
        .and_then(|text| config::parse_config(text.as_ref()).ok());
    let data = ViewData {
        parsed_config,
        parse_error: None,
        draft_dirty: false,
        has_saved_source: true,
        needs_restart: false,
        last_handshake: String::new(),
    };
    let is_running_config = app.runtime.running_id == Some(selected.id)
        || app.runtime.running_name.as_deref() == Some(selected.name.as_str());

    OverviewPreviewData {
        has_selection: true,
        selected_name_text: selected.name.clone(),
        source_text: overview_source_summary(&selected),
        context_text: if is_running_config {
            "Selected config matches the running tunnel.".to_string()
        } else {
            "Preview comes from the selected saved config, not the running tunnel.".to_string()
        },
        local_ip_text: format_local_ip(&data),
        dns_text: format_dns(&data),
        endpoint_text: format_endpoint(&data),
        allowed_text: format_allowed_summary(&data),
        route_table_text: data
            .parsed_config
            .as_ref()
            .map(|cfg| format_route_table(cfg.interface.table))
            .unwrap_or_else(|| "-".to_string()),
    }
}

impl StatsState {
    pub(crate) fn overview_traffic_summary(
        &self,
        configs: &ConfigsState,
        period: TrafficPeriod,
        today: NaiveDate,
        current_hour: i64,
    ) -> TrafficSummaryData {
        const MAX_RANK_ITEMS: usize = 7;
        self.overview_traffic_summary_at(configs, period, today, current_hour, MAX_RANK_ITEMS)
    }

    fn overview_traffic_summary_at(
        &self,
        configs: &ConfigsState,
        period: TrafficPeriod,
        today: NaiveDate,
        current_hour: i64,
        max_rank_items: usize,
    ) -> TrafficSummaryData {
        // 这里的统计口径有两层：
        // 1. 先按当前 period 求整体 total_rx / total_tx；
        // 2. 再按配置分别求同周期总量，用于排行。
        //
        // 排行里会跳过零流量项，避免“空配置”挤占榜单空间。
        let (total_rx, total_tx, ranked) = match period {
            TrafficPeriod::Today => {
                // Today 采用滚动 24 小时窗口，而不是“从当天 00:00 到现在”。
                let min_hour = current_hour.saturating_sub(23);
                let (total_rx, total_tx) =
                    sum_hours(&self.traffic.global_hours, min_hour, current_hour);
                let ranked = configs
                    .iter()
                    .filter_map(|cfg| {
                        let hours = self.traffic.config_hours.get(&cfg.id)?;
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
                let dates = build_day_key_set(today, 0, 30);
                let (total_rx, total_tx) = sum_days(&self.traffic.global_days, &dates);
                let ranked = configs
                    .iter()
                    .filter_map(|cfg| {
                        let days = self.traffic.config_days.get(&cfg.id)?;
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
                let dates = build_day_key_set(today, 30, 30);
                let (total_rx, total_tx) = sum_days(&self.traffic.global_days, &dates);
                let ranked = configs
                    .iter()
                    .filter_map(|cfg| {
                        let days = self.traffic.config_days.get(&cfg.id)?;
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

    pub(crate) fn overview_traffic_trend(&self, today: NaiveDate) -> TrafficTrendData {
        // 图表始终展示固定长度的最近 7 天：
        // - 没有流量的日期也会补 0；
        // - 最后一个点永远是 today，并打上高亮标记。
        let mut points = Vec::with_capacity(TRAFFIC_TREND_DAYS);
        for offset in (0..TRAFFIC_TREND_DAYS).rev() {
            let date = today - ChronoDuration::days(offset as i64);
            let day_key = day_key_from_date(date);
            let bytes = self
                .traffic
                .global_days
                .iter()
                .find(|day| day.day_key == day_key)
                .map(|day| day.rx_bytes.saturating_add(day.tx_bytes))
                .unwrap_or(0);
            let label = date.format("%a").to_string();
            points.push(TrafficTrendPoint {
                label,
                bytes,
                is_today: offset == 0,
            });
        }

        let total: u64 = points.iter().map(|point| point.bytes).sum();
        let average_bytes = total as f64 / TRAFFIC_TREND_DAYS as f64;

        TrafficTrendData {
            points,
            average_bytes,
        }
    }
}

/// 构造一个 day_key 集合，供“按天”统计窗口过滤使用。
///
/// 例如：
/// - `start_offset = 0, days = 30` 表示最近 30 天；
/// - `start_offset = 30, days = 30` 表示再往前 30 天。
fn build_day_key_set(today: NaiveDate, start_offset: i64, days: i64) -> HashSet<i32> {
    let mut set = HashSet::with_capacity(days as usize);
    for offset in start_offset..start_offset + days {
        let date = today - ChronoDuration::days(offset);
        set.insert(day_key_from_date(date));
    }
    set
}

/// 在给定日期集合里，累计按天统计的 RX/TX。
fn sum_days(days: &[TrafficDayBucket], dates: &HashSet<i32>) -> (u64, u64) {
    let mut rx = 0u64;
    let mut tx = 0u64;
    for day in days {
        if dates.contains(&day.day_key) {
            rx = rx.saturating_add(day.rx_bytes);
            tx = tx.saturating_add(day.tx_bytes);
        }
    }
    (rx, tx)
}

/// 在给定小时窗口里，累计按小时统计的 RX/TX。
fn sum_hours(hours: &[TrafficHourBucket], min_hour: i64, max_hour: i64) -> (u64, u64) {
    let mut rx = 0u64;
    let mut tx = 0u64;
    for hour in hours {
        if hour.hour_key >= min_hour && hour.hour_key <= max_hour {
            rx = rx.saturating_add(hour.rx_bytes);
            tx = tx.saturating_add(hour.tx_bytes);
        }
    }
    (rx, tx)
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

fn format_last_updated(last_stats_at: Option<std::time::Instant>) -> String {
    let Some(last_stats_at) = last_stats_at else {
        return "Waiting".to_string();
    };
    let secs = last_stats_at.elapsed().as_secs();
    match secs {
        0..=4 => "Just now".to_string(),
        5..=59 => format!("{secs}s ago"),
        60..=3599 => format!("{}m ago", secs / 60),
        _ => format!("{}h ago", secs / 3600),
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

fn overview_source_summary(config: &TunnelConfig) -> String {
    match &config.source {
        ConfigSource::File { origin_path } => origin_path
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .map(|name| format!("Imported • {name}"))
            .unwrap_or_else(|| "Imported config".to_string()),
        ConfigSource::Paste => "Saved in app storage".to_string(),
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

/// 将 stats 线程采样到的 RSS 统一格式化为文案。
fn format_process_memory(bytes: Option<u64>) -> String {
    bytes.map(format_memory).unwrap_or_else(|| "-".to_string())
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

    use super::OverviewData;
    use crate::ui::state::{
        day_key_from_date, ConfigSource, EndpointFamily, TrafficDayBucket, TrafficHourBucket,
        TrafficPeriod, TunnelConfig, WgApp, TRAFFIC_TREND_DAYS,
    };
    use crate::ui::themes::AppearancePolicy;

    fn make_app() -> WgApp {
        WgApp::new(
            r_wg::backend::wg::Engine::new(),
            AppearancePolicy::Dark,
            ThemeMode::Dark,
            None,
            None,
            None,
            None,
        )
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
        app.stats.traffic.global_hours = vec![
            TrafficHourBucket {
                hour_key: current_hour,
                rx_bytes: 50,
                tx_bytes: 5,
            },
            TrafficHourBucket {
                hour_key: current_hour - 23,
                rx_bytes: 10,
                tx_bytes: 1,
            },
            TrafficHourBucket {
                hour_key: current_hour - 24,
                rx_bytes: 999,
                tx_bytes: 999,
            },
        ];
        app.stats.traffic.config_hours.insert(
            1,
            vec![
                TrafficHourBucket {
                    hour_key: current_hour,
                    rx_bytes: 20,
                    tx_bytes: 10,
                },
                TrafficHourBucket {
                    hour_key: current_hour - 24,
                    rx_bytes: 500,
                    tx_bytes: 500,
                },
            ],
        );
        app.stats.traffic.config_hours.insert(
            2,
            vec![TrafficHourBucket {
                hour_key: current_hour - 1,
                rx_bytes: 40,
                tx_bytes: 20,
            }],
        );

        let summary = app.stats.overview_traffic_summary_at(
            &app.configs,
            app.ui_session.traffic_period,
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
        app.stats.traffic.global_days = vec![
            TrafficDayBucket {
                day_key: day_key_from_date(
                    NaiveDate::from_ymd_opt(2026, 3, 6).expect("valid test date"),
                ),
                rx_bytes: 10,
                tx_bytes: 20,
            },
            TrafficDayBucket {
                day_key: day_key_from_date(
                    NaiveDate::from_ymd_opt(2026, 2, 10).expect("valid test date"),
                ),
                rx_bytes: 30,
                tx_bytes: 40,
            },
            TrafficDayBucket {
                day_key: day_key_from_date(
                    NaiveDate::from_ymd_opt(2026, 1, 20).expect("valid test date"),
                ),
                rx_bytes: 500,
                tx_bytes: 600,
            },
        ];
        app.stats.traffic.config_days.insert(
            7,
            vec![
                TrafficDayBucket {
                    day_key: day_key_from_date(
                        NaiveDate::from_ymd_opt(2026, 3, 6).expect("valid test date"),
                    ),
                    rx_bytes: 10,
                    tx_bytes: 20,
                },
                TrafficDayBucket {
                    day_key: day_key_from_date(
                        NaiveDate::from_ymd_opt(2026, 2, 10).expect("valid test date"),
                    ),
                    rx_bytes: 30,
                    tx_bytes: 40,
                },
                TrafficDayBucket {
                    day_key: day_key_from_date(
                        NaiveDate::from_ymd_opt(2026, 1, 20).expect("valid test date"),
                    ),
                    rx_bytes: 500,
                    tx_bytes: 600,
                },
            ],
        );

        app.ui_session.traffic_period = TrafficPeriod::ThisMonth;
        let this_month = app.stats.overview_traffic_summary_at(
            &app.configs,
            app.ui_session.traffic_period,
            today,
            0,
            7,
        );
        assert_eq!(this_month.total_rx, 40);
        assert_eq!(this_month.total_tx, 60);
        assert_eq!(this_month.ranked[0].total_bytes(), 100);

        app.ui_session.traffic_period = TrafficPeriod::LastMonth;
        let last_month = app.stats.overview_traffic_summary_at(
            &app.configs,
            app.ui_session.traffic_period,
            today,
            0,
            7,
        );
        assert_eq!(last_month.total_rx, 500);
        assert_eq!(last_month.total_tx, 600);
        assert_eq!(last_month.ranked[0].total_bytes(), 1100);
    }

    #[test]
    fn traffic_trend_aggregates_same_day_entries_and_marks_today() {
        let today = NaiveDate::from_ymd_opt(2026, 3, 6).expect("valid test date");
        let mut app = make_app();
        app.stats.traffic.global_days = vec![
            TrafficDayBucket {
                day_key: day_key_from_date(
                    NaiveDate::from_ymd_opt(2026, 3, 6).expect("valid test date"),
                ),
                rx_bytes: 100,
                tx_bytes: 50,
            },
            TrafficDayBucket {
                day_key: day_key_from_date(
                    NaiveDate::from_ymd_opt(2026, 3, 4).expect("valid test date"),
                ),
                rx_bytes: 20,
                tx_bytes: 10,
            },
            TrafficDayBucket {
                day_key: day_key_from_date(
                    NaiveDate::from_ymd_opt(2026, 2, 20).expect("valid test date"),
                ),
                rx_bytes: 999,
                tx_bytes: 0,
            },
        ];

        let trend = app.stats.overview_traffic_trend(today);

        assert_eq!(trend.points.len(), TRAFFIC_TREND_DAYS);
        let today_point = trend.points.last().expect("today should be present");
        assert!(today_point.is_today);
        assert_eq!(today_point.bytes, 150);
    }

    #[test]
    fn overview_snapshot_separates_running_runtime_from_selected_preview() {
        let mut app = make_app();
        app.configs.configs = vec![
            TunnelConfig {
                id: 7,
                name: "alpha".to_string(),
                name_lower: "alpha".to_string(),
                text: Some(
                    "[Interface]\nPrivateKey = 0000000000000000000000000000000000000000000000000000000000000000\nAddress = 10.0.0.2/32\nDNS = 1.1.1.1\nTable = off\n\n[Peer]\nPublicKey = 1111111111111111111111111111111111111111111111111111111111111111\nAllowedIPs = 0.0.0.0/0\nEndpoint = 203.0.113.10:51820\n"
                        .into(),
                ),
                source: ConfigSource::Paste,
                storage_path: PathBuf::from("/tmp/alpha.conf"),
                endpoint_family: EndpointFamily::Unknown,
            },
            TunnelConfig {
                id: 9,
                name: "beta".to_string(),
                name_lower: "beta".to_string(),
                text: None,
                source: ConfigSource::Paste,
                storage_path: PathBuf::from("/tmp/beta.conf"),
                endpoint_family: EndpointFamily::Unknown,
            },
        ];
        app.selection.selected_id = Some(7);
        app.runtime.running = true;
        app.runtime.running_id = Some(9);
        app.runtime.running_name = Some("beta".to_string());

        let overview = OverviewData::new(&mut app);

        assert!(overview.runtime.is_running);
        assert_eq!(overview.runtime.running_name_text, "beta");
        assert!(overview.preview.has_selection);
        assert_eq!(overview.preview.selected_name_text, "alpha");
        assert_eq!(overview.preview.local_ip_text, "10.0.0.2/32");
        assert_eq!(overview.preview.dns_text, "1.1.1.1");
        assert_eq!(overview.preview.route_table_text, "off");
        assert_eq!(overview.preview.allowed_text, "1 routes");
        assert!(overview
            .preview
            .context_text
            .contains("not the running tunnel"));
    }
}
