use std::time::Duration;

use gpui::{AppContext, Context, SharedString, Window};
use r_wg::backend::wg::StartRequest;
use r_wg::dns::DnsSelection;

use super::super::permissions::start_permission_message;
use super::super::state::{TunnelConfig, WgApp};
use super::super::tray;

impl WgApp {
    /// 启动或停止隧道。
    ///
    /// 说明：
    /// - 根据当前运行状态分支处理 start/stop。
    /// - 所有耗时操作都放到后台执行，完成后回到 UI 线程更新状态。
    pub(crate) fn handle_start_stop(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.handle_start_stop_core(cx);
    }

    fn handle_start_stop_core(&mut self, cx: &mut Context<Self>) {
        // 统一入口：所有 Start/Stop 点击都会走这里。
        // busy=true 表示已有异步流程在执行，避免并发触发导致状态错乱。
        if self.runtime.busy {
            if self.runtime.running {
                // stop 过程中再次点击 start：记录“待启动请求”，
                // 等 stop 完成后自动执行，避免用户需要再次点击。
                if self.runtime.queue_pending_start(
                    self.selection
                        .build_pending_start(&self.configs, &self.runtime),
                ) {
                    self.set_status("Stopping... (queued start)");
                    cx.notify();
                }
            }
            return;
        }

        // 根据运行状态决定 start/stop。
        if self.runtime.running {
            // 已运行：进入停止流程，并标记 busy，避免重复触发。
            self.runtime.begin_stop();
            self.set_status("Stopping...");
            cx.notify();

            let engine = self.engine.clone();
            // 停止操作放后台执行，完成后回到 UI 线程更新状态。
            cx.spawn(async move |view, cx| {
                let stop_task = cx.background_spawn(async move { engine.stop() });
                let result = stop_task.await;
                view.update(cx, |this, cx| {
                    match result {
                        Ok(()) => {
                            this.runtime.finish_stop_success();
                            this.set_status("Stopped");
                            // 停止成功后发送系统通知，让最小化到托盘时也能感知状态变更。
                            tray::notify_system("r-wg", "Tunnel disconnected", false);
                            this.stats.clear_runtime_metrics();

                            // 如果 stop 期间有人点击 start，则现在补发启动。
                            if let Some(pending) = this.runtime.pending_start.take() {
                                if let Some(selected) = this.configs.find_by_id(pending.config_id) {
                                    let cached_text =
                                        this.cached_config_text(&selected.storage_path);
                                    let initial_text = selected.text.clone().or(cached_text);
                                    let delay = this.runtime.restart_delay();
                                    this.start_with_config(selected, initial_text, delay, cx);
                                } else {
                                    this.set_error("Pending start config not found".to_string());
                                }
                            }
                        }
                        Err(err) => {
                            // 停止失败则清空 pending，避免误触发自动启动。
                            this.runtime.finish_stop_failure();
                            let message = format!("Stop failed: {err}");
                            this.set_error(message.clone());
                            // 停止失败属于用户需感知事件，使用错误通知提高可见性。
                            tray::notify_system("r-wg", &message, true);
                        }
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
            return;
        }

        let Some(selected_idx) = self.selection.selected else {
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
        let delay = self.runtime.restart_delay();
        self.start_with_config(selected, initial_text, delay, cx);
    }

    /// 托盘菜单：仅在未运行时启动。
    pub(crate) fn handle_start_from_tray(&mut self, cx: &mut Context<Self>) {
        if self.runtime.running || self.runtime.busy {
            return;
        }
        self.handle_start_stop_core(cx);
    }

    /// 托盘菜单：仅在运行中停止。
    pub(crate) fn handle_stop_from_tray(&mut self, cx: &mut Context<Self>) {
        if !self.runtime.running || self.runtime.busy {
            return;
        }
        self.handle_start_stop_core(cx);
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
        self.runtime.begin_start();
        self.set_status(format!("Starting {}...", selected.name));
        cx.notify();

        let engine = self.engine.clone();
        let dns_selection = DnsSelection::new(self.ui_prefs.dns_mode, self.ui_prefs.dns_preset);
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
                        this.runtime.finish_start_attempt();
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
            let request = StartRequest::new(selected.name.clone(), text.to_string(), dns_selection);
            let start_task = cx.background_spawn(async move { engine.start(request) });
            let result = start_task.await;
            view.update(cx, |this, cx| {
                this.runtime.finish_start_attempt();
                match result {
                    Ok(()) => {
                        // 启动成功：刷新运行态与统计。
                        this.runtime.mark_started(&selected);
                        this.stats.reset_for_start();
                        this.set_status(format!("Running {}", selected.name));
                        // 启动成功后通知当前已连接的隧道名称，便于多配置场景快速确认。
                        tray::notify_system(
                            "r-wg",
                            &format!("Tunnel connected: {}", selected.name),
                            false,
                        );
                        // 启动成功后开始轮询统计。
                        this.start_stats_polling(cx);
                    }
                    Err(err) => {
                        // 启动失败：保持停止态并提示错误。
                        let message = format!("Start failed: {err}");
                        this.set_error(message.clone());
                        // 启动失败直接推送错误原因，避免用户必须切回主窗口查看。
                        tray::notify_system("r-wg", &message, true);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Instant;

    use gpui_component::theme::ThemeMode;

    use super::*;
    use crate::ui::state::{ConfigSource, TunnelConfig, RESTART_COOLDOWN};

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
        }
    }

    #[test]
    fn restart_delay_is_none_without_last_stop() {
        let app = make_app();
        assert_eq!(app.runtime.restart_delay(), None);
    }

    #[test]
    fn restart_delay_returns_remaining_cooldown() {
        let mut app = make_app();
        app.runtime.last_stop_at = Instant::now().checked_sub(Duration::from_millis(100));

        let delay = app
            .runtime
            .restart_delay()
            .expect("cooldown should still be active");
        assert!(delay > Duration::from_millis(150));
        assert!(delay <= Duration::from_millis(250));
    }

    #[test]
    fn restart_delay_is_none_after_cooldown_elapsed() {
        let mut app = make_app();
        app.runtime.last_stop_at =
            Instant::now().checked_sub(RESTART_COOLDOWN + Duration::from_millis(10));

        assert_eq!(app.runtime.restart_delay(), None);
    }

    #[test]
    fn build_pending_start_prefers_selected_config() {
        let mut app = make_app();
        app.configs.configs = vec![make_config(11, "alpha"), make_config(22, "beta")];
        app.selection.selected = Some(1);
        app.runtime.running_id = Some(11);

        let pending = app
            .selection
            .build_pending_start(&app.configs, &app.runtime)
            .expect("selected config should win");
        assert_eq!(pending.config_id, 22);
    }

    #[test]
    fn build_pending_start_falls_back_to_running_config() {
        let mut app = make_app();
        app.runtime.running_id = Some(77);

        let pending = app
            .selection
            .build_pending_start(&app.configs, &app.runtime)
            .expect("running config should be used when nothing is selected");
        assert_eq!(pending.config_id, 77);
    }
}
