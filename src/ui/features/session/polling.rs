use std::collections::VecDeque;
use std::time::{Duration, Instant};

use chrono::Local;
use gpui::{AppContext, Context};
#[cfg(target_os = "windows")]
use r_wg::backend::wg::EngineError;
use r_wg::backend::wg::EngineStats;
use r_wg::log::events::stats as log_stats;

use crate::ui::state::{
    day_key_from_date, ConfigInspectorTab, SidebarItem, WgApp, SPARKLINE_SAMPLES,
};

pub(crate) struct SampledRuntimeMetrics {
    pub(crate) iface_rx: Option<u64>,
    pub(crate) iface_tx: Option<u64>,
    pub(crate) process_rss_bytes: Option<u64>,
}

pub(crate) fn start_stats_polling(app: &mut WgApp, cx: &mut Context<WgApp>) {
    app.stats.stats_generation = app.stats.stats_generation.wrapping_add(1);
    let generation = app.stats.stats_generation;
    let tunnel_session = app.tunnel_session.clone();
    let poll_interval = Duration::from_secs(2);

    cx.spawn(async move |view, cx| loop {
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

        let tunnel_session = tunnel_session.clone();
        let (result, sampled) = cx
            .background_spawn(async move {
                let result = tunnel_session.stats();
                let iface = running_name.as_deref().and_then(read_interface_stats);
                let sampled = SampledRuntimeMetrics {
                    iface_rx: iface.map(|(rx, _)| rx),
                    iface_tx: iface.map(|(_, tx)| tx),
                    process_rss_bytes: read_process_rss_bytes(),
                };
                (result, sampled)
            })
            .await;

        let continue_polling = view
            .update(cx, |this, cx| {
                if !this.runtime.running || this.stats.stats_generation != generation {
                    return false;
                }

                let mut persist_due = false;
                let mut status_changed = false;
                match result {
                    Ok(stats) => {
                        persist_due = apply_stats(this, stats, sampled);
                    }
                    Err(err) => {
                        #[cfg(target_os = "windows")]
                        if matches!(err, EngineError::NotRunning | EngineError::ChannelClosed) {
                            super::controller::complete_stop_success(this, cx);
                            cx.notify();
                            return false;
                        }
                        status_changed = this.stats.set_stats_error(format!("Stats failed: {err}"));
                    }
                }
                if persist_due {
                    this.persist_state_async(cx);
                    this.stats.traffic.mark_persisted(Instant::now());
                }
                let should_notify = match this.ui_session.sidebar_active {
                    SidebarItem::Configs => {
                        this.current_configs_inspector_tab(cx) == ConfigInspectorTab::Activity
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
    })
    .detach();
}

pub(crate) fn apply_stats(
    app: &mut WgApp,
    stats: EngineStats,
    sampled: SampledRuntimeMetrics,
) -> bool {
    let total_rx: u64 = stats.peers.iter().map(|peer| peer.rx_bytes).sum();
    let total_tx: u64 = stats.peers.iter().map(|peer| peer.tx_bytes).sum();

    let mut rx_delta = 0u64;
    let mut tx_delta = 0u64;
    let mut elapsed_secs = None;
    if let Some(last_at) = app.stats.last_stats_at {
        let elapsed = last_at.elapsed().as_secs_f64();
        if elapsed > 0.1 {
            rx_delta = total_rx.saturating_sub(app.stats.last_rx_bytes);
            tx_delta = total_tx.saturating_sub(app.stats.last_tx_bytes);
            app.stats.rx_rate_bps = rx_delta as f64 / elapsed;
            app.stats.tx_rate_bps = tx_delta as f64 / elapsed;
            elapsed_secs = Some(elapsed);
        }
    }

    let iface_rx = sampled.iface_rx;
    let iface_tx = sampled.iface_tx;
    if let (Some(rx), Some(tx), Some(elapsed)) = (iface_rx, iface_tx, elapsed_secs) {
        let rx_delta = rx.saturating_sub(app.stats.last_iface_rx_bytes);
        let tx_delta = tx.saturating_sub(app.stats.last_iface_tx_bytes);
        app.stats.iface_rx_rate_bps = rx_delta as f64 / elapsed;
        app.stats.iface_tx_rate_bps = tx_delta as f64 / elapsed;
        app.stats.last_iface_rx_bytes = rx;
        app.stats.last_iface_tx_bytes = tx;
    } else if let (Some(rx), Some(tx)) = (iface_rx, iface_tx) {
        app.stats.last_iface_rx_bytes = rx;
        app.stats.last_iface_tx_bytes = tx;
    }

    app.stats.last_stats_at = Some(Instant::now());
    app.stats.last_rx_bytes = total_rx;
    app.stats.last_tx_bytes = total_tx;
    app.stats.process_rss_bytes = sampled.process_rss_bytes;
    app.stats.stats_revision = app.stats.stats_revision.wrapping_add(1);
    push_rate_sample(&mut app.stats.rx_rate_history, app.stats.rx_rate_bps);
    push_rate_sample(&mut app.stats.tx_rate_history, app.stats.tx_rate_bps);

    let persist_due = record_traffic(app, rx_delta, tx_delta);

    app.stats.peer_stats = stats.peers;
    if app.stats.peer_stats.is_empty() {
        app.stats.set_stats_error("No peers reported");
    } else {
        if rx_delta + tx_delta < 1024 {
            app.stats.stats_idle_samples = app.stats.stats_idle_samples.saturating_add(1);
        } else {
            app.stats.stats_idle_samples = 0;
        }
        if app.stats.stats_idle_samples >= 3 {
            app.stats.set_stats_error("No tunnel traffic detected");
        } else {
            app.stats
                .set_stats_error(format!("Peers: {}", app.stats.peer_stats.len()));
        }
    }

    log_stats::snapshot(log_stats::SnapshotArgs {
        tun_name: app.runtime.running_name.as_deref(),
        elapsed_secs,
        total_rx,
        total_tx,
        rx_delta,
        tx_delta,
        rx_rate_bps: app.stats.rx_rate_bps,
        tx_rate_bps: app.stats.tx_rate_bps,
        iface_rx,
        iface_tx,
        iface_rx_rate_bps: app.stats.iface_rx_rate_bps,
        iface_tx_rate_bps: app.stats.iface_tx_rate_bps,
    });

    persist_due
}

pub(crate) fn record_traffic(app: &mut WgApp, rx_bytes: u64, tx_bytes: u64) -> bool {
    if rx_bytes.saturating_add(tx_bytes) == 0 {
        return false;
    }

    let now = Local::now();
    let day_key = day_key_from_date(now.date_naive());
    let hour_key = now.timestamp() / 3600;
    let created = app.stats.traffic.record(
        app.runtime.running_id,
        day_key,
        hour_key,
        rx_bytes,
        tx_bytes,
    );

    if created {
        return true;
    }

    match app.stats.traffic.last_persist_at {
        Some(last) => last.elapsed() >= Duration::from_secs(60),
        None => true,
    }
}

fn read_interface_stats(tun: &str) -> Option<(u64, u64)> {
    let base = format!("/sys/class/net/{tun}/statistics");
    let rx = std::fs::read_to_string(format!("{base}/rx_bytes")).ok()?;
    let tx = std::fs::read_to_string(format!("{base}/tx_bytes")).ok()?;
    let rx = rx.trim().parse::<u64>().ok()?;
    let tx = tx.trim().parse::<u64>().ok()?;
    Some((rx, tx))
}

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

pub(crate) fn read_process_rss_bytes() -> Option<u64> {
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
        None
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
