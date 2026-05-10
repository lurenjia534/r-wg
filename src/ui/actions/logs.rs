use std::collections::HashSet;
use std::time::{Duration, Instant};

use gpui::{AppContext, Context, SharedString, Window};
use gpui_component::input::InputState;
use r_wg::log;
use r_wg::log::events::{ipc as log_ipc, ui as log_ui};

use super::super::state::{SidebarItem, WgApp};

const BACKEND_LOG_POLL_INTERVAL: Duration = Duration::from_secs(2);
const MAX_MERGED_LOG_LINES: usize = 2000;

impl WgApp {
    /// 确保日志输入框已创建，避免在没有窗口时初始化。
    pub(crate) fn ensure_log_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ui.log_input.is_some() {
            return;
        }

        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor("text")
                .line_number(false)
                .soft_wrap(true)
                .searchable(false)
                .placeholder("No logs captured")
        });
        self.ui.log_input = Some(input);
    }

    pub(crate) fn ensure_backend_log_polling(&mut self, cx: &mut Context<Self>) {
        if !self.ui_prefs.log_viewer_enabled {
            self.stop_backend_log_polling();
            return;
        }
        if self.ui.backend_log_poll_active {
            return;
        }
        self.ui.backend_log_poll_active = true;
        log_ui::logs_opened();
        self.ui.backend_log_poll_generation = self.ui.backend_log_poll_generation.wrapping_add(1);
        let generation = self.ui.backend_log_poll_generation;
        let tunnel_session = self.services.tunnel_session.clone();

        cx.spawn(async move |view, cx| loop {
            let should_continue = view
                .update(cx, |this, _| {
                    this.ui_session.sidebar_active == SidebarItem::Logs
                        && this.ui.backend_log_poll_generation == generation
                })
                .unwrap_or(false);
            if !should_continue {
                break;
            }

            let should_sync = view
                .update(cx, |this, _| this.begin_backend_log_sync())
                .unwrap_or(None);
            if let Some(sync_generation) = should_sync {
                log_ipc::backend_log_snapshot_requested();
                let tunnel_session = tunnel_session.clone();
                let result = cx
                    .background_spawn(async move { tunnel_session.log_snapshot() })
                    .await;
                let _ = view.update(cx, |this, cx| {
                    this.finish_backend_log_sync(sync_generation, result);
                    cx.notify();
                });
            }

            cx.background_executor()
                .timer(BACKEND_LOG_POLL_INTERVAL)
                .await;
        })
        .detach();
    }

    pub(crate) fn stop_backend_log_polling(&mut self) {
        if !self.ui.backend_log_poll_active {
            return;
        }
        self.ui.backend_log_poll_active = false;
        self.ui.backend_log_poll_generation = self.ui.backend_log_poll_generation.wrapping_add(1);
        self.ui.backend_log_sync_in_flight = false;
    }

    pub(crate) fn clear_all_logs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.ui_prefs.log_viewer_enabled {
            return;
        }
        log_ui::logs_cleared();
        log_ipc::backend_log_clear_requested();
        log::clear();
        self.ui.backend_log_lines.clear();
        self.ui.backend_log_last_error = None;
        self.ui.backend_log_last_sync = None;
        self.ui.backend_log_generation = self.ui.backend_log_generation.wrapping_add(1);
        if let Some(log_input) = self.ui.log_input.clone() {
            log_input.update(cx, |input, cx| {
                input.set_value("", window, cx);
            });
        }
        let tunnel_session = self.services.tunnel_session.clone();
        cx.spawn(async move |view, cx| {
            let result = cx
                .background_spawn(async move {
                    tunnel_session.log_clear()?;
                    tunnel_session.log_snapshot()
                })
                .await;
            let _ = view.update(cx, |this, cx| {
                match result {
                    Ok(lines) => {
                        log_ipc::backend_log_snapshot_received(lines.len());
                        this.ui.backend_log_lines = lines;
                        this.ui.backend_log_last_error = None;
                        this.ui.backend_log_last_sync = Some(Instant::now());
                    }
                    Err(err) => {
                        log_ipc::backend_log_clear_failed(&err);
                        let message = format!("backend log clear failed: {err}");
                        this.ui.backend_log_last_error = Some(SharedString::from(message.clone()));
                        log::event(log::LogLevel::Warn, "ui", format_args!("{message}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn merged_log_lines(&self) -> Vec<String> {
        if !self.ui_prefs.log_viewer_enabled {
            return Vec::new();
        }
        let mut lines = log::snapshot();
        lines.extend(self.ui.backend_log_lines.iter().cloned());
        if let Some(error) = &self.ui.backend_log_last_error {
            lines.push(format!("[{}][r-wg][ui] {}", timestamp_now(), error));
        }
        lines.sort();
        let mut seen = HashSet::new();
        lines.retain(|line| seen.insert(line.clone()));
        if lines.len() > MAX_MERGED_LOG_LINES {
            lines.split_off(lines.len() - MAX_MERGED_LOG_LINES)
        } else {
            lines
        }
    }

    fn begin_backend_log_sync(&mut self) -> Option<u64> {
        if self.ui.backend_log_sync_in_flight {
            return None;
        }
        if self
            .ui
            .backend_log_last_sync
            .is_some_and(|last_sync| last_sync.elapsed() < BACKEND_LOG_POLL_INTERVAL)
        {
            return None;
        }
        self.ui.backend_log_sync_in_flight = true;
        self.ui.backend_log_generation = self.ui.backend_log_generation.wrapping_add(1);
        Some(self.ui.backend_log_generation)
    }

    fn finish_backend_log_sync(
        &mut self,
        generation: u64,
        result: Result<Vec<String>, r_wg::backend::wg::EngineError>,
    ) {
        if self.ui.backend_log_generation != generation {
            return;
        }
        self.ui.backend_log_sync_in_flight = false;
        self.ui.backend_log_last_sync = Some(Instant::now());
        match result {
            Ok(lines) => {
                log_ipc::backend_log_snapshot_received(lines.len());
                self.ui.backend_log_lines = lines;
                self.ui.backend_log_last_error = None;
            }
            Err(err) => {
                log_ipc::backend_log_snapshot_failed(&err);
                let message = format!("backend log sync failed: {err}");
                self.ui.backend_log_last_error = Some(SharedString::from(message.clone()));
                log::event(log::LogLevel::Warn, "ui", format_args!("{message}"));
            }
        }
    }
}

fn timestamp_now() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}
