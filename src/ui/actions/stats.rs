use std::collections::VecDeque;
use std::time::{Duration, Instant};

use chrono::Local;
use gpui::{AppContext, Context};
use r_wg::backend::wg::EngineStats;
use r_wg::log::events::stats as log_stats;

use super::super::state::{
    SidebarItem, TrafficDay, TrafficDayStats, TrafficHour, WgApp, SPARKLINE_SAMPLES,
    TRAFFIC_HISTORY_DAYS, TRAFFIC_HOURLY_HISTORY, TRAFFIC_ROLLING_DAYS,
};

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
        // Proxies 列表在滚动时非常敏感，降低轮询频率并跳过统计刷新，
        // 避免每次 notify 触发大列表重建造成卡顿。
        let proxies_interval = Duration::from_secs(6);

        // 异步轮询 peer 统计，避免阻塞 UI。
        cx.spawn(async move |view, cx| {
            loop {
                // 先在 UI 线程读取状态，避免在后台线程里直接访问视图数据。
                let (should_continue, in_proxies) = view
                    .update(cx, |this, _| {
                        if !this.runtime.running || this.stats.stats_generation != generation {
                            return (false, false);
                        }
                        (true, this.ui_prefs.sidebar_active == SidebarItem::Proxies)
                    })
                    .unwrap_or((false, false));
                if !should_continue {
                    break;
                }

                // Proxies 页面：降频并跳过 stats 拉取与 notify。
                let interval = if in_proxies {
                    proxies_interval
                } else {
                    poll_interval
                };
                cx.background_executor().timer(interval).await;
                if in_proxies {
                    // 直接进入下一轮等待，避免触发统计刷新和 UI 重绘。
                    continue;
                }

                let engine = engine.clone();
                let result = cx.background_spawn(async move { engine.stats() }).await;

                // 将统计结果回写 UI 状态并 notify，触发依赖 stats 的视图刷新。
                let continue_polling = view
                    .update(cx, |this, cx| {
                        if !this.runtime.running || this.stats.stats_generation != generation {
                            return false;
                        }

                        let mut persist_due = false;
                        match result {
                            Ok(stats) => {
                                persist_due = this.apply_stats(stats);
                            }
                            Err(err) => {
                                this.stats.set_stats_error(format!("Stats failed: {err}"));
                            }
                        }
                        if persist_due {
                            // 仅在必要时落盘，避免每次轮询都写 state.json。
                            this.persist_state_async(cx);
                            this.stats.traffic_last_persist_at = Some(Instant::now());
                            this.stats.traffic_dirty = false;
                        }
                        cx.notify();
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
    /// - 尝试读取网卡层统计，作为辅助对比。
    pub(crate) fn apply_stats(&mut self, stats: EngineStats) -> bool {
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

        let mut iface_rx = None;
        let mut iface_tx = None;
        if let Some(name) = self.runtime.running_name.as_deref() {
            if let Some((rx, tx)) = read_interface_stats(name) {
                iface_rx = Some(rx);
                iface_tx = Some(tx);
                if let Some(elapsed) = elapsed_secs {
                    let rx_delta = rx.saturating_sub(self.stats.last_iface_rx_bytes);
                    let tx_delta = tx.saturating_sub(self.stats.last_iface_tx_bytes);
                    self.stats.iface_rx_rate_bps = rx_delta as f64 / elapsed;
                    self.stats.iface_tx_rate_bps = tx_delta as f64 / elapsed;
                }
                self.stats.last_iface_rx_bytes = rx;
                self.stats.last_iface_tx_bytes = tx;
            }
        }

        self.stats.last_stats_at = Some(Instant::now());
        self.stats.last_rx_bytes = total_rx;
        self.stats.last_tx_bytes = total_tx;
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
        let total = rx_bytes.saturating_add(tx_bytes);
        // 没有流量就不记录，避免制造无意义的“空写盘”。
        if total == 0 {
            return false;
        }

        // 按本地日期归档，确保跨时区/跨天显示符合用户直觉。
        let now = Local::now();
        let today = now.format("%Y-%m-%d").to_string();
        let hour = now.timestamp() / 3600;
        let mut created = false;

        // 旧版总量统计（用于 7 日趋势）。
        if update_traffic_day_total(&mut self.stats.traffic_days, &today, total) {
            created = true;
        }
        self.prune_traffic_days();

        // 新版整体统计（按天 + 按小时）。
        if update_traffic_day_stats(&mut self.stats.traffic_days_v2, &today, rx_bytes, tx_bytes) {
            created = true;
        }
        if update_traffic_hour_stats(&mut self.stats.traffic_hours, hour, rx_bytes, tx_bytes) {
            created = true;
        }

        // 按配置统计：仅在运行中时记录。
        if let Some(config_id) = self.runtime.running_id {
            let days = self
                .stats
                .config_traffic_days
                .entry(config_id)
                .or_insert_with(Vec::new);
            if update_traffic_day_stats(days, &today, rx_bytes, tx_bytes) {
                created = true;
            }
            let hours = self
                .stats
                .config_traffic_hours
                .entry(config_id)
                .or_insert_with(Vec::new);
            if update_traffic_hour_stats(hours, hour, rx_bytes, tx_bytes) {
                created = true;
            }
        }

        self.stats.traffic_dirty = true;

        if created {
            // 新的一天/新的一小时首次写入，立即落盘，避免异常退出丢失起点。
            return true;
        }

        // 同一天内按节流间隔落盘，平衡数据安全与磁盘写入频率。
        match self.stats.traffic_last_persist_at {
            Some(last) => last.elapsed() >= Duration::from_secs(60),
            None => true,
        }
    }

    fn prune_traffic_days(&mut self) {
        // 保持按时间顺序排列，超出上限时丢弃最旧的记录。
        self.stats.traffic_days.sort_by(|a, b| a.date.cmp(&b.date));
        if self.stats.traffic_days.len() > TRAFFIC_HISTORY_DAYS {
            let remove_count = self.stats.traffic_days.len() - TRAFFIC_HISTORY_DAYS;
            self.stats.traffic_days.drain(0..remove_count);
        }
    }
}

fn update_traffic_day_total(days: &mut Vec<TrafficDay>, date: &str, bytes: u64) -> bool {
    if let Some(day) = days.iter_mut().find(|day| day.date == date) {
        day.bytes = day.bytes.saturating_add(bytes);
        return false;
    }
    days.push(TrafficDay {
        date: date.to_string(),
        bytes,
    });
    true
}

fn update_traffic_day_stats(
    days: &mut Vec<TrafficDayStats>,
    date: &str,
    rx_bytes: u64,
    tx_bytes: u64,
) -> bool {
    if let Some(day) = days.iter_mut().find(|day| day.date == date) {
        day.rx_bytes = day.rx_bytes.saturating_add(rx_bytes);
        day.tx_bytes = day.tx_bytes.saturating_add(tx_bytes);
        return false;
    }
    days.push(TrafficDayStats {
        date: date.to_string(),
        rx_bytes,
        tx_bytes,
    });
    prune_traffic_day_stats(days);
    true
}

fn update_traffic_hour_stats(
    hours: &mut Vec<TrafficHour>,
    hour: i64,
    rx_bytes: u64,
    tx_bytes: u64,
) -> bool {
    if let Some(bucket) = hours.iter_mut().find(|bucket| bucket.hour == hour) {
        bucket.rx_bytes = bucket.rx_bytes.saturating_add(rx_bytes);
        bucket.tx_bytes = bucket.tx_bytes.saturating_add(tx_bytes);
        return false;
    }
    hours.push(TrafficHour {
        hour,
        rx_bytes,
        tx_bytes,
    });
    prune_traffic_hours(hours);
    true
}

fn prune_traffic_day_stats(days: &mut Vec<TrafficDayStats>) {
    days.sort_by(|a, b| a.date.cmp(&b.date));
    if days.len() > TRAFFIC_ROLLING_DAYS {
        let remove_count = days.len() - TRAFFIC_ROLLING_DAYS;
        days.drain(0..remove_count);
    }
}

fn prune_traffic_hours(hours: &mut Vec<TrafficHour>) {
    hours.sort_by(|a, b| a.hour.cmp(&b.hour));
    if hours.len() > TRAFFIC_HOURLY_HISTORY {
        let remove_count = hours.len() - TRAFFIC_HOURLY_HISTORY;
        hours.drain(0..remove_count);
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
