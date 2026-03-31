use gpui::Context;

use super::super::features::session::polling;
use super::super::state::WgApp;

impl WgApp {
    pub(crate) fn start_stats_polling(&mut self, cx: &mut Context<Self>) {
        polling::start_stats_polling(self, cx);
    }
}

#[cfg(test)]
mod tests {
    use gpui_component::theme::ThemeMode;

    use crate::ui::features::session::polling::{
        read_process_rss_bytes, record_traffic, SampledRuntimeMetrics,
    };
    use crate::ui::features::themes::AppearancePolicy;
    use crate::ui::state::{SidebarItem, WgApp};

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

        let persist_due = record_traffic(&mut app, 512, 256);

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
