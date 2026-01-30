use std::time::{Duration, Instant};

use gpui::{AppContext, Context, SharedString, Window};
use r_wg::backend::wg::StartRequest;
use r_wg::dns::DnsSelection;

use super::super::permissions::start_permission_message;
use super::super::state::{PendingStart, TunnelConfig, WgApp};

const RESTART_COOLDOWN: Duration = Duration::from_millis(300);

impl WgApp {
    /// 启动或停止隧道。
    ///
    /// 说明：
    /// - 根据当前运行状态分支处理 start/stop。
    /// - 所有耗时操作都放到后台执行，完成后回到 UI 线程更新状态。
    pub(crate) fn handle_start_stop(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        // 统一入口：所有 Start/Stop 点击都会走这里。
        // busy=true 表示已有异步流程在执行，避免并发触发导致状态错乱。
        if self.busy {
            if self.running {
                // stop 过程中再次点击 start：记录“待启动请求”，
                // 等 stop 完成后自动执行，避免用户需要再次点击。
                if let Some(pending) = self.build_pending_start() {
                    self.pending_start = Some(pending);
                    self.set_status("Stopping... (queued start)");
                    cx.notify();
                }
            }
            return;
        }

        // 根据运行状态决定 start/stop。
        if self.running {
            // 已运行：进入停止流程，并标记 busy，避免重复触发。
            self.busy = true;
            self.set_status("Stopping...");
            cx.notify();

            let engine = self.engine.clone();
            let view = cx.weak_entity();
            // 停止操作放后台执行，完成后回到 UI 线程更新状态。
            cx.spawn(async move |view, cx| {
                let stop_task = cx.background_spawn(async move { engine.stop() });
                let result = stop_task.await;
                view.update(cx, |this, cx| {
                    this.busy = false;
                    match result {
                        Ok(()) => {
                            this.running = false;
                            this.running_name = None;
                            this.running_id = None;
                            this.started_at = None;
                            this.set_status("Stopped");
                            this.clear_stats();
                            this.last_stop_at = Some(Instant::now());

                            // 如果 stop 期间有人点击 start，则现在补发启动。
                            if let Some(pending) = this.pending_start.take() {
                                if let Some(selected) =
                                    this.find_config_by_id(pending.config_id)
                                {
                                    let cached_text =
                                        this.cached_config_text(&selected.storage_path);
                                    let initial_text =
                                        selected.text.clone().or(cached_text);
                                    let delay = this.restart_delay();
                                    this.start_with_config(
                                        selected,
                                        initial_text,
                                        delay,
                                        cx,
                                    );
                                } else {
                                    this.set_error(
                                        "Pending start config not found".to_string(),
                                    );
                                }
                            }
                        }
                        Err(err) => {
                            // 停止失败则清空 pending，避免误触发自动启动。
                            this.pending_start = None;
                            this.set_error(format!("Stop failed: {err}"));
                        }
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
            return;
        }

        let Some(selected_idx) = self.selected else {
            self.set_error("Select a tunnel first");
            cx.notify();
            return;
        };
        let selected = self.configs[selected_idx].clone();
        // 启动前准备配置文本：
        // - 粘贴型配置直接使用内存文本；
        // - 文件型配置优先走缓存，否则异步读取磁盘。
        let cached_text = self.cached_config_text(&selected.storage_path);
        let initial_text = selected.text.clone().or(cached_text);

        // stop 后的冷却时间：避免“刚停就起”的抖动。
        let delay = self.restart_delay();
        self.start_with_config(selected, initial_text, delay, cx);
    }

    /// 计算重启冷却时间。
    ///
    /// - 最近刚 stop：返回剩余等待时长；
    /// - 否则返回 None（立即启动）。
    fn restart_delay(&self) -> Option<Duration> {
        let last_stop = self.last_stop_at?;
        let elapsed = last_stop.elapsed();
        if elapsed >= RESTART_COOLDOWN {
            None
        } else {
            Some(RESTART_COOLDOWN - elapsed)
        }
    }

    /// 构造待启动请求。
    ///
    /// - 优先当前选中的配置；
    /// - 没有选中时回退到当前运行的配置。
    fn build_pending_start(&self) -> Option<PendingStart> {
        if let Some(idx) = self.selected {
            return Some(PendingStart {
                config_id: self.configs[idx].id,
            });
        }
        self.running_id.map(|id| PendingStart { config_id: id })
    }

    /// 根据配置 ID 查找配置（用于 stop 完成后的自动启动）。
    fn find_config_by_id(&self, config_id: u64) -> Option<TunnelConfig> {
        self.configs
            .iter()
            .find(|config| config.id == config_id)
            .cloned()
    }

    /// 执行启动流程（可选冷却延迟）。
    ///
    /// 统一所有启动逻辑，避免在多处复制复杂流程。
    fn start_with_config(
        &mut self,
        selected: TunnelConfig,
        initial_text: Option<SharedString>,
        delay: Option<Duration>,
        cx: &mut Context<Self>,
    ) {
        // 启动前权限检查（Linux cap_net_admin）。
        if let Some(message) = start_permission_message() {
            // 运行前检查权限提示（Linux cap_net_admin）。
            self.set_error(message);
            cx.notify();
            return;
        }

        // 标记 busy，避免重复点击；并更新状态提示。
        self.busy = true;
        self.set_status(format!("Starting {}...", selected.name));
        cx.notify();

        let engine = self.engine.clone();
        let view = cx.weak_entity();
        let dns_selection = DnsSelection::new(self.dns_mode, self.dns_preset);
        cx.spawn(async move |view, cx| {
            // 冷却等待：在后台 sleep，不阻塞 UI。
            if let Some(delay) = delay {
                let delay_task =
                    cx.background_spawn(async move { tokio::time::sleep(delay).await });
                let _ = delay_task.await;
            }

            // 如果文本不存在，需要异步读取文件；读取失败直接返回错误。
            let text_result = match initial_text {
                Some(text) => Ok(text),
                None => {
                    let path = selected.storage_path.clone();
                    let read_task =
                        cx.background_spawn(async move { std::fs::read_to_string(&path) });
                    match read_task.await {
                        Ok(text) => Ok(SharedString::from(text)),
                        Err(err) => Err(format!("Read failed: {err}")),
                    }
                }
            };

            let text = match text_result {
                Ok(text) => text,
                Err(message) => {
                    view.update(cx, |this, cx| {
                        this.busy = false;
                        this.set_error(message);
                        cx.notify();
                    })
                    .ok();
                    return;
                }
            };

            // 成功读取后写入缓存，避免后续启动/复制重复 IO。
            let text_for_cache = text.clone();
            let path_for_cache = selected.storage_path.clone();
            view.update(cx, |this, _| {
                this.cache_config_text(path_for_cache, text_for_cache);
            })
            .ok();

            // 组装 start 请求并交给后台线程。
            let request =
                StartRequest::new(selected.name.clone(), text.to_string(), dns_selection);
            let start_task = cx.background_spawn(async move { engine.start(request) });
            let result = start_task.await;
            view.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(()) => {
                        // 启动成功：刷新运行态与统计。
                        this.running = true;
                        this.running_name = Some(selected.name.clone());
                        this.running_id = Some(selected.id);
                        this.started_at = Some(Instant::now());
                        this.last_stats_at = None;
                        this.last_rx_bytes = 0;
                        this.last_tx_bytes = 0;
                        this.rx_rate_bps = 0.0;
                        this.tx_rate_bps = 0.0;
                        this.reset_rate_history();
                        this.stats_idle_samples = 0;
                        this.last_iface_rx_bytes = 0;
                        this.last_iface_tx_bytes = 0;
                        this.iface_rx_rate_bps = 0.0;
                        this.iface_tx_rate_bps = 0.0;
                        this.set_status(format!("Running {}", selected.name));
                        this.stats_note = "Fetching peer stats...".into();
                        // 启动成功后开始轮询统计。
                        this.start_stats_polling(cx);
                    }
                    Err(err) => {
                        // 启动失败：保持停止态并提示错误。
                        this.set_error(format!("Start failed: {err}"));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
