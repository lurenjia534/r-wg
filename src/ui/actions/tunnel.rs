//! 隧道操作动作模块
//!
//! 本模块是 WgApp 的动作方法封装，将 UI 事件转发到 session controller 处理。

use gpui::{Context, Window};

use super::super::features::session::controller;
use super::super::state::WgApp;

impl WgApp {
    /// 处理启动/停止按钮点击
    pub(crate) fn handle_start_stop(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        controller::handle_start_stop(self, _window, cx);
    }

    /// 核心启动/停止逻辑
    pub(crate) fn handle_start_stop_core(&mut self, cx: &mut Context<Self>) {
        controller::handle_start_stop_core(self, cx);
    }

    /// 处理从托盘启动
    pub(crate) fn handle_start_from_tray(&mut self, cx: &mut Context<Self>) {
        controller::handle_start_from_tray(self, cx);
    }

    /// 处理从托盘停止
    pub(crate) fn handle_stop_from_tray(&mut self, cx: &mut Context<Self>) {
        controller::handle_stop_from_tray(self, cx);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;
    use std::time::Instant;

    use gpui_component::theme::ThemeMode;
    use r_wg::application::TunnelSessionService;

    use super::*;
    use crate::ui::features::themes::AppearancePolicy;
    use crate::ui::state::{ConfigSource, EndpointFamily, TunnelConfig, RESTART_COOLDOWN};

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

    fn make_config(id: u64, name: &str) -> TunnelConfig {
        TunnelConfig {
            id,
            name: name.to_string(),
            name_lower: name.to_ascii_lowercase(),
            text: None,
            source: ConfigSource::Paste,
            storage_path: PathBuf::from(format!("/tmp/{id}.conf")),
            endpoint_family: EndpointFamily::Unknown,
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
        app.selection.selected_id = Some(22);
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
