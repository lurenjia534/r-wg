use std::collections::VecDeque;
use std::time::{Duration, Instant};

use chrono::Local;
use gpui::{AppContext, Context};
#[cfg(target_os = "windows")]
use r_wg::backend::wg::EngineError;
use r_wg::backend::wg::EngineStats;
use r_wg::log::events::stats as log_stats;

use super::super::state::{
    day_key_from_date, ConfigInspectorTab, SidebarItem, WgApp, SPARKLINE_SAMPLES,
};

struct SampledRuntimeMetrics {
    iface_rx: Option<u64>,
    iface_tx: Option<u64>,
    process_rss_bytes: Option<u64>,
}

impl WgApp {
    /// 启动统计轮询。
    ///
    /// 关键点：
    /// - 使用 generation 作为“软取消”标记，防止旧轮询继续写入新会话。
    /// - stats 获取在后台线程执行，UI 线程只接收结果并更新状态。
    pub(crate) fn start_stats_polling(&mut self, cx: &mut Context<Self>) {
        // 每次启动使用新的 generation，停止后自动中断旧轮询。
        self.stats.stats_generation = self.stats.stats_generation.wrapping_add(1);
        let generation = self.stats.stats_generation;
        let engine = self.engine.clone();
        let poll_interval = Duration::from_secs(2);

        // 异步轮询 peer 统计，避免阻塞 UI。
        cx.spawn(async move |view, cx| {
            loop {
                let should_continue = view
                    .update(cx, |this, _| {
                        this.runtime.running && this.stats.stats_generation == generation
                    })
                    .unwrap_or(false);
                if !should_continue {
                    break;
                }

                cx.background_executor().timer(poll_interval).await;

                let poll_context = view
                    .update(cx, |this, _| {
                        if !this.runtime.running || this.stats.stats_generation != generation {
                            return None;
                        }
                        Some(this.runtime.running_name.clone())
                    })
                    .unwrap_or(None);
                let Some(running_name) = poll_context else {
                    break;
                };

                let engine = engine.clone();
                let (result, sampled) = cx
                    .background_spawn(async move {
                        let result = engine.stats();
                        let iface = running_name.as_deref().and_then(read_interface_stats);
                        let sampled = SampledRuntimeMetrics {
                            iface_rx: iface.map(|(rx, _)| rx),
                            iface_tx: iface.map(|(_, tx)| tx),
                            process_rss_bytes: read_process_rss_bytes(),
                        };
                        (result, sampled)
                    })
                    .await;

                // 将统计结果回写 UI 状态并 notify，触发依赖 stats 的视图刷新。
                let continue_polling = view
                    .update(cx, |this, cx| {
                        if !this.runtime.running || this.stats.stats_generation != generation {
                            return false;
                        }

                        let mut persist_due = false;
                        let mut status_changed = false;
                        match result {
                            Ok(stats) => {
                                persist_due = this.apply_stats(stats, sampled);
                            }
                            Err(err) => {
                                #[cfg(target_os = "windows")]
                                if matches!(
                                    err,
                                    EngineError::NotRunning | EngineError::ChannelClosed
                                ) {
                                    this.runtime.finish_stop_success();
                                    this.refresh_configs_workspace_row_flags(cx);
                                    this.stats.clear_runtime_metrics();
                                    status_changed = this.set_status("Stopped");
                                    if status_changed {
                                        cx.notify();
                                    }
                                    return false;
                                }
                                status_changed =
                                    this.stats.set_stats_error(format!("Stats failed: {err}"));
                            }
                        }
                        if persist_due {
                            // 仅在必要时落盘，避免每次轮询都写 state.json。
                            this.persist_state_async(cx);
                            this.stats.traffic.mark_persisted(Instant::now());
                        }
                        let should_notify = match this.ui_session.sidebar_active {
                            SidebarItem::Configs => {
                                this.current_configs_inspector_tab(cx)
                                    == ConfigInspectorTab::Activity
                            }
                            SidebarItem::Proxies => false,
                            _ => true,
                        };
                        if should_notify || status_changed || persist_due {
                            cx.notify();
                        }
                        true
                    })
                    .unwrap_or(false);

                if !continue_polling {
                    break;
                }
            }
        })
        .detach();
    }

    /// 应用 EngineStats 结果。
    ///
    /// 说明：
    /// - 先汇总 peer 的累计字节数。
    /// - 基于上一次采样计算速率，避免计数回绕导致负值。
    /// - 网卡层统计和 RSS 由后台采样并随本轮结果一起提交。
    fn apply_stats(&mut self, stats: EngineStats, sampled: SampledRuntimeMetrics) -> bool {
        // 聚合统计，用于右侧状态卡片展示。
        let total_rx: u64 = stats.peers.iter().map(|peer| peer.rx_bytes).sum();
        let total_tx: u64 = stats.peers.iter().map(|peer| peer.tx_bytes).sum();

        let mut rx_delta = 0u64;
        let mut tx_delta = 0u64;
        let mut elapsed_secs = None;
        if let Some(last_at) = self.stats.last_stats_at {
            let elapsed = last_at.elapsed().as_secs_f64();
            if elapsed > 0.1 {
                rx_delta = total_rx.saturating_sub(self.stats.last_rx_bytes);
                tx_delta = total_tx.saturating_sub(self.stats.last_tx_bytes);
                self.stats.rx_rate_bps = rx_delta as f64 / elapsed;
                self.stats.tx_rate_bps = tx_delta as f64 / elapsed;
                elapsed_secs = Some(elapsed);
            }
        }

        let iface_rx = sampled.iface_rx;
        let iface_tx = sampled.iface_tx;
        if let (Some(rx), Some(tx), Some(elapsed)) = (iface_rx, iface_tx, elapsed_secs) {
            let rx_delta = rx.saturating_sub(self.stats.last_iface_rx_bytes);
            let tx_delta = tx.saturating_sub(self.stats.last_iface_tx_bytes);
            self.stats.iface_rx_rate_bps = rx_delta as f64 / elapsed;
            self.stats.iface_tx_rate_bps = tx_delta as f64 / elapsed;
            self.stats.last_iface_rx_bytes = rx;
            self.stats.last_iface_tx_bytes = tx;
        } else if let (Some(rx), Some(tx)) = (iface_rx, iface_tx) {
            self.stats.last_iface_rx_bytes = rx;
            self.stats.last_iface_tx_bytes = tx;
        }

        self.stats.last_stats_at = Some(Instant::now());
        self.stats.last_rx_bytes = total_rx;
        self.stats.last_tx_bytes = total_tx;
        self.stats.process_rss_bytes = sampled.process_rss_bytes;
        self.stats.stats_revision = self.stats.stats_revision.wrapping_add(1);
        push_rate_sample(&mut self.stats.rx_rate_history, self.stats.rx_rate_bps);
        push_rate_sample(&mut self.stats.tx_rate_history, self.stats.tx_rate_bps);

        // 本轮统计窗口内的总流量（RX + TX），用于 7 日趋势。
        let persist_due = self.record_traffic(rx_delta, tx_delta);

        self.stats.peer_stats = stats.peers;
        if self.stats.peer_stats.is_empty() {
            self.stats.set_stats_error("No peers reported");
        } else {
            if rx_delta + tx_delta < 1024 {
                self.stats.stats_idle_samples = self.stats.stats_idle_samples.saturating_add(1);
            } else {
                self.stats.stats_idle_samples = 0;
            }
            if self.stats.stats_idle_samples >= 3 {
                self.stats.set_stats_error("No tunnel traffic detected");
            } else {
                self.stats
                    .set_stats_error(format!("Peers: {}", self.stats.peer_stats.len()));
            }
        }

        log_stats::snapshot(
            self.runtime.running_name.as_deref(),
            elapsed_secs,
            total_rx,
            total_tx,
            rx_delta,
            tx_delta,
            self.stats.rx_rate_bps,
            self.stats.tx_rate_bps,
            iface_rx,
            iface_tx,
            self.stats.iface_rx_rate_bps,
            self.stats.iface_tx_rate_bps,
        );

        persist_due
    }

    fn record_traffic(&mut self, rx_bytes: u64, tx_bytes: u64) -> bool {
        if rx_bytes.saturating_add(tx_bytes) == 0 {
            return false;
        }

        // 按本地日期归档，确保跨时区/跨天显示符合用户直觉。
        let now = Local::now();
        let day_key = day_key_from_date(now.date_naive());
        let hour_key = now.timestamp() / 3600;
        let created = self.stats.traffic.record(
            self.runtime.running_id,
            day_key,
            hour_key,
            rx_bytes,
            tx_bytes,
        );

        if created {
            // 新的一天/新的一小时首次写入，立即落盘，避免异常退出丢失起点。
            return true;
        }

        // 同一天内按节流间隔落盘，平衡数据安全与磁盘写入频率。
        match self.stats.traffic.last_persist_at {
            Some(last) => last.elapsed() >= Duration::from_secs(60),
            None => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use gpui_component::theme::ThemeMode;

    use super::{read_process_rss_bytes, SampledRuntimeMetrics};
    use crate::ui::state::{SidebarItem, WgApp};
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

    #[test]
    fn stats_sampling_still_records_traffic_while_proxies_page_is_active() {
        let mut app = make_app();
        app.ui_session.sidebar_active = SidebarItem::Proxies;
        app.runtime.running = true;
        app.runtime.running_id = Some(7);

        let persist_due = app.record_traffic(512, 256);

        assert!(persist_due);
        assert_eq!(app.stats.traffic.global_days.len(), 1);
        assert_eq!(
            app.stats.traffic.global_days[0].rx_bytes + app.stats.traffic.global_days[0].tx_bytes,
            768
        );
        assert_eq!(app.stats.traffic.global_hours.len(), 1);
        assert_eq!(
            app.stats.traffic.config_hours.get(&7).map(Vec::len),
            Some(1)
        );
    }

    #[test]
    fn sampled_runtime_metrics_can_be_built_without_ui_thread_io() {
        let sampled = SampledRuntimeMetrics {
            iface_rx: None,
            iface_tx: None,
            process_rss_bytes: read_process_rss_bytes(),
        };

        assert_eq!(sampled.iface_rx, None);
        assert_eq!(sampled.iface_tx, None);
    }
}

/// 读取内核统计的网卡字节数。
///
/// 说明：直接访问 /sys/class/net，避免外部命令依赖，代价低且稳定。
fn read_interface_stats(tun: &str) -> Option<(u64, u64)> {
    let base = format!("/sys/class/net/{tun}/statistics");
    let rx = std::fs::read_to_string(format!("{base}/rx_bytes")).ok()?;
    let tx = std::fs::read_to_string(format!("{base}/tx_bytes")).ok()?;
    let rx = rx.trim().parse::<u64>().ok()?;
    let tx = tx.trim().parse::<u64>().ok()?;
    Some((rx, tx))
}

/// 写入一个速率采样点。
///
/// 说明：保持固定长度队列，保证 sparkline 始终有稳定的样本数。
fn push_rate_sample(history: &mut VecDeque<f32>, value: f64) {
    let value = if value.is_finite() && value > 0.0 {
        value as f32
    } else {
        0.0
    };
    if history.len() >= SPARKLINE_SAMPLES {
        history.pop_front();
    }
    history.push_back(value);
}

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
