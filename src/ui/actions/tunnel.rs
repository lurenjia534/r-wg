use std::time::Instant;

use gpui::{AppContext, Context, Window};
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

        let Some(selected) = self.selected_config().cloned() else {
            self.set_error("Select a tunnel first");
            cx.notify();
            return;
        };

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
                let request = StartRequest::new(
                    selected.name.clone(),
                    selected.text.clone(),
                    dns_selection,
                );
                let start_task = cx.background_spawn(async move { engine.start(request) });
                let result = start_task.await;
                view.update(cx, |this, cx| {
                    this.busy = false;
                    match result {
                        Ok(()) => {
                            this.running = true;
                            this.running_name = Some(selected.name.clone());
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
