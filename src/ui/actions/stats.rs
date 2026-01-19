use std::collections::VecDeque;
use std::time::{Duration, Instant};

use chrono::Local;
use gpui::{AppContext, Context};
use r_wg::backend::wg::EngineStats;
use r_wg::log;

use super::super::state::{
    SidebarItem, TrafficDay, WgApp, SPARKLINE_SAMPLES, TRAFFIC_HISTORY_DAYS,
};

impl WgApp {
    /// 启动统计轮询。
    ///
    /// 关键点：
    /// - 使用 generation 作为“软取消”标记，防止旧轮询继续写入新会话。
    /// - stats 获取在后台线程执行，UI 线程只接收结果并更新状态。
    pub(crate) fn start_stats_polling(&mut self, cx: &mut Context<Self>) {
        // 每次启动使用新的 generation，停止后自动中断旧轮询。
        self.stats_generation = self.stats_generation.wrapping_add(1);
        let generation = self.stats_generation;
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
                        if !this.running || this.stats_generation != generation {
                            return (false, false);
                        }
                        (true, this.sidebar_active == SidebarItem::Proxies)
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
                        if !this.running || this.stats_generation != generation {
                            return false;
                        }

                        let mut persist_due = false;
                        match result {
                            Ok(stats) => {
                                persist_due = this.apply_stats(stats);
                            }
                            Err(err) => {
                                this.stats_note = format!("Stats failed: {err}").into();
                            }
                        }
                        if persist_due {
                            // 仅在必要时落盘，避免每次轮询都写 state.json。
                            this.persist_state_async(cx);
                            this.traffic_last_persist_at = Some(Instant::now());
                            this.traffic_dirty = false;
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
        if let Some(last_at) = self.last_stats_at {
            let elapsed = last_at.elapsed().as_secs_f64();
            if elapsed > 0.1 {
                rx_delta = total_rx.saturating_sub(self.last_rx_bytes);
                tx_delta = total_tx.saturating_sub(self.last_tx_bytes);
                self.rx_rate_bps = rx_delta as f64 / elapsed;
                self.tx_rate_bps = tx_delta as f64 / elapsed;
                elapsed_secs = Some(elapsed);
            }
        }

        let mut iface_rx = None;
        let mut iface_tx = None;
        if let Some(name) = self.running_name.as_deref() {
            if let Some((rx, tx)) = read_interface_stats(name) {
                iface_rx = Some(rx);
                iface_tx = Some(tx);
                if let Some(elapsed) = elapsed_secs {
                    let rx_delta = rx.saturating_sub(self.last_iface_rx_bytes);
                    let tx_delta = tx.saturating_sub(self.last_iface_tx_bytes);
                    self.iface_rx_rate_bps = rx_delta as f64 / elapsed;
                    self.iface_tx_rate_bps = tx_delta as f64 / elapsed;
                }
                self.last_iface_rx_bytes = rx;
                self.last_iface_tx_bytes = tx;
            }
        }

        self.last_stats_at = Some(Instant::now());
        self.last_rx_bytes = total_rx;
        self.last_tx_bytes = total_tx;
        push_rate_sample(&mut self.rx_rate_history, self.rx_rate_bps);
        push_rate_sample(&mut self.tx_rate_history, self.tx_rate_bps);

        // 本轮统计窗口内的总流量（RX + TX），用于 7 日趋势。
        let traffic_delta = rx_delta.saturating_add(tx_delta);
        let persist_due = self.record_daily_traffic(traffic_delta);

        self.peer_stats = stats.peers;
        if self.peer_stats.is_empty() {
            self.stats_note = "No peers reported".into();
        } else {
            if rx_delta + tx_delta < 1024 {
                self.stats_idle_samples = self.stats_idle_samples.saturating_add(1);
            } else {
                self.stats_idle_samples = 0;
            }
            if self.stats_idle_samples >= 3 {
                self.stats_note = "No tunnel traffic detected".into();
            } else {
                self.stats_note = format!("Peers: {}", self.peer_stats.len()).into();
            }
        }

        if log::enabled() {
            let name = self.running_name.as_deref().unwrap_or("-");
            let elapsed_text = elapsed_secs
                .map(|value| format!("{value:.2}s"))
                .unwrap_or_else(|| "-".to_string());
            let iface_text = match (iface_rx, iface_tx) {
                (Some(rx), Some(tx)) => format!(
                    "iface_rx={rx} iface_tx={tx} iface_rx_rate={:.0}B/s iface_tx_rate={:.0}B/s",
                    self.iface_rx_rate_bps, self.iface_tx_rate_bps
                ),
                _ => "iface_stats=unavailable".to_string(),
            };
            log::log(
                "stats",
                format!(
                    "tun={name} elapsed={elapsed_text} rx_total={total_rx} tx_total={total_tx} rx_delta={rx_delta} tx_delta={tx_delta} rx_rate={:.0}B/s tx_rate={:.0}B/s {iface_text}",
                    self.rx_rate_bps, self.tx_rate_bps
                ),
            );
        }

        persist_due
    }

    /// 清空统计状态。
    ///
    /// 说明：停止隧道时调用，避免残留旧会话的数据与提示。
    pub(crate) fn clear_stats(&mut self) {
        self.peer_stats.clear();
        self.stats_note = "Peer stats unavailable".into();
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

    /// 重置速率历史采样。
    ///
    /// 说明：将历史清空并补齐固定长度，确保 sparkline 视觉稳定。
    pub(crate) fn reset_rate_history(&mut self) {
        self.rx_rate_history.clear();
        self.tx_rate_history.clear();
        for _ in 0..SPARKLINE_SAMPLES {
            self.rx_rate_history.push_back(0.0);
            self.tx_rate_history.push_back(0.0);
        }
    }

    fn record_daily_traffic(&mut self, bytes: u64) -> bool {
        // 没有流量就不记录，避免制造无意义的“空写盘”。
        if bytes == 0 {
            return false;
        }

        // 按本地日期归档，确保跨时区/跨天显示符合用户直觉。
        let today = Local::now().format("%Y-%m-%d").to_string();
        let mut created = false;
        if let Some(day) = self.traffic_days.iter_mut().find(|day| day.date == today) {
            day.bytes = day.bytes.saturating_add(bytes);
        } else {
            self.traffic_days.push(TrafficDay {
                date: today,
                bytes,
            });
            created = true;
        }

        self.prune_traffic_days();
        self.traffic_dirty = true;

        if created {
            // 新的一天首次写入，立即落盘，避免程序异常退出导致丢失当天起点。
            return true;
        }

        // 同一天内按节流间隔落盘，平衡数据安全与磁盘写入频率。
        match self.traffic_last_persist_at {
            Some(last) => last.elapsed() >= Duration::from_secs(60),
            None => true,
        }
    }

    fn prune_traffic_days(&mut self) {
        // 保持按时间顺序排列，超出上限时丢弃最旧的记录。
        self.traffic_days.sort_by(|a, b| a.date.cmp(&b.date));
        if self.traffic_days.len() > TRAFFIC_HISTORY_DAYS {
            let remove_count = self.traffic_days.len() - TRAFFIC_HISTORY_DAYS;
            self.traffic_days.drain(0..remove_count);
        }
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
