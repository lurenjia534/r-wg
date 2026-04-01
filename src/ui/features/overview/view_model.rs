use chrono::Local;
use r_wg::core::config;

use crate::ui::format::{format_bytes, format_duration, format_route_table, summarize_peers};
use crate::ui::state::{
    ConfigSource, TrafficPeriod, TrafficSummaryData, TrafficTrendData, TunnelConfig, WgApp,
};
use crate::ui::view::shared::{
    format_allowed_summary, format_dns, format_endpoint, format_local_ip, ViewData,
};

/// `OverviewData` 是 Overview 页的专属 ViewModel。
///
/// 它的目标很明确：把 Overview 页里所有“从状态取值 -> 计算口径 -> 格式化文案”
/// 的工作，尽量在进入视图前一次性做完，让 `render_overview` 只负责拼装页面。
pub(crate) struct OverviewData {
    pub(crate) runtime: OverviewRuntimeData,
    pub(crate) preview: OverviewPreviewData,
    pub(crate) traffic_period: TrafficPeriod,
    pub(crate) traffic_trend: TrafficTrendData,
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

impl OverviewData {
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

fn format_speed_text(is_running: bool, bytes_per_sec: f64) -> String {
    if !is_running {
        return "0.0 KB/s".to_string();
    }
    format_speed(bytes_per_sec)
}

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

fn format_process_memory(bytes: Option<u64>) -> String {
    bytes.map(format_memory).unwrap_or_else(|| "-".to_string())
}

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

    use gpui_component::theme::ThemeMode;
    use r_wg::application::TunnelSessionService;

    use super::OverviewData;
    use crate::ui::features::themes::AppearancePolicy;
    use crate::ui::state::{ConfigSource, EndpointFamily, TunnelConfig, WgApp};

    fn make_app() -> WgApp {
        WgApp::new(
            TunnelSessionService::new(r_wg::backend::wg::Engine::new()),
            AppearancePolicy::Dark,
            ThemeMode::Dark,
            None,
            None,
            None,
            None,
        )
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

        let overview = OverviewData::new(&app);

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
