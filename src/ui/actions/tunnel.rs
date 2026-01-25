use std::time::Instant;

use gpui::{AppContext, Context, SharedString, Window};
use r_wg::backend::wg::StartRequest;
use r_wg::dns::DnsSelection;

use super::super::permissions::start_permission_message;
use super::super::state::WgApp;

impl WgApp {
    /// 启动或停止隧道。
    ///
    /// 说明：
    /// - 根据当前运行状态分支处理 start/stop。
    /// - 所有耗时操作都放到后台执行，完成后回到 UI 线程更新状态。
    pub(crate) fn handle_start_stop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // 根据运行状态决定 start/stop。
        if self.running {
            self.busy = true;
            self.set_status("Stopping...");
            cx.notify();

            let engine = self.engine.clone();
            let view = cx.weak_entity();
            window
                .spawn(cx, async move |cx| {
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
                            }
                            Err(err) => {
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
        // 启动前按需读取文本：
        // - 粘贴型配置直接使用内存文本；
        // - 文件型配置优先走缓存，否则异步读取磁盘。
        let cached_text = self.cached_config_text(&selected.storage_path);
        let initial_text = selected.text.clone().or(cached_text);

        if let Some(message) = start_permission_message() {
            // 运行前检查权限提示（Linux cap_net_admin）。
            self.set_error(message);
            cx.notify();
            return;
        }

        self.busy = true;
        self.set_status(format!("Starting {}...", selected.name));
        cx.notify();

        let engine = self.engine.clone();
        let view = cx.weak_entity();
        let dns_selection = DnsSelection::new(self.dns_mode, self.dns_preset);
        window
            .spawn(cx, async move |cx| {
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

                let request =
                    StartRequest::new(selected.name.clone(), text.to_string(), dns_selection);
                let start_task = cx.background_spawn(async move { engine.start(request) });
                let result = start_task.await;
                view.update(cx, |this, cx| {
                    this.busy = false;
                    match result {
                        Ok(()) => {
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
