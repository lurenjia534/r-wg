use std::cmp::Reverse;
use std::collections::HashSet;

use chrono::{Duration as ChronoDuration, NaiveDate};

use super::{
    day_key_from_date, ConfigsState, StatsState, TrafficDayBucket, TrafficHourBucket,
    TrafficPeriod, TRAFFIC_TREND_DAYS,
};

/// 趋势图上的单个点。
#[derive(Clone)]
pub(crate) struct TrafficTrendPoint {
    pub(crate) label: String,
    pub(crate) bytes: u64,
    pub(crate) is_today: bool,
}

/// Overview 页 7 日趋势图的完整输入。
pub(crate) struct TrafficTrendData {
    pub(crate) points: Vec<TrafficTrendPoint>,
    pub(crate) average_bytes: f64,
    pub(crate) total_bytes: u64,
    pub(crate) max_bytes: u64,
    pub(crate) peak_label: String,
    pub(crate) peak_bytes: u64,
    pub(crate) non_zero_days: usize,
}

/// Traffic Summary 区块的完整输入。
#[derive(Clone)]
pub(crate) struct TrafficSummaryData {
    pub(crate) total_rx: u64,
    pub(crate) total_tx: u64,
    pub(crate) ranked: Vec<TrafficRankItem>,
    pub(crate) active_configs: usize,
    pub(crate) others_total: u64,
    pub(crate) top_config_name: Option<String>,
    pub(crate) top_config_total: u64,
}

/// Traffic Summary 中单个配置的流量排行项。
#[derive(Clone)]
pub(crate) struct TrafficRankItem {
    pub(crate) name: String,
    pub(crate) rx_bytes: u64,
    pub(crate) tx_bytes: u64,
}

impl TrafficRankItem {
    pub(crate) fn total_bytes(&self) -> u64 {
        self.rx_bytes.saturating_add(self.tx_bytes)
    }
}

impl StatsState {
    pub(crate) fn overview_traffic_summary(
        &self,
        configs: &ConfigsState,
        period: TrafficPeriod,
        today: NaiveDate,
        current_hour: i64,
    ) -> TrafficSummaryData {
        const MAX_RANK_ITEMS: usize = 7;
        self.overview_traffic_summary_at(configs, period, today, current_hour, MAX_RANK_ITEMS)
    }

    fn overview_traffic_summary_at(
        &self,
        configs: &ConfigsState,
        period: TrafficPeriod,
        today: NaiveDate,
        current_hour: i64,
        max_rank_items: usize,
    ) -> TrafficSummaryData {
        let (total_rx, total_tx, ranked) = match period {
            TrafficPeriod::Today => {
                let min_hour = current_hour.saturating_sub(23);
                let (total_rx, total_tx) =
                    sum_hours(&self.traffic.global_hours, min_hour, current_hour);
                let ranked = configs
                    .iter()
                    .filter_map(|cfg| {
                        let hours = self.traffic.config_hours.get(&cfg.id)?;
                        let (rx, tx) = sum_hours(hours, min_hour, current_hour);
                        let total = rx.saturating_add(tx);
                        if total == 0 {
                            None
                        } else {
                            Some(TrafficRankItem {
                                name: cfg.name.clone(),
                                rx_bytes: rx,
                                tx_bytes: tx,
                            })
                        }
                    })
                    .collect::<Vec<_>>();
                (total_rx, total_tx, ranked)
            }
            TrafficPeriod::ThisMonth => {
                let dates = build_day_key_set(today, 0, 30);
                let (total_rx, total_tx) = sum_days(&self.traffic.global_days, &dates);
                let ranked = configs
                    .iter()
                    .filter_map(|cfg| {
                        let days = self.traffic.config_days.get(&cfg.id)?;
                        let (rx, tx) = sum_days(days, &dates);
                        let total = rx.saturating_add(tx);
                        if total == 0 {
                            None
                        } else {
                            Some(TrafficRankItem {
                                name: cfg.name.clone(),
                                rx_bytes: rx,
                                tx_bytes: tx,
                            })
                        }
                    })
                    .collect::<Vec<_>>();
                (total_rx, total_tx, ranked)
            }
            TrafficPeriod::LastMonth => {
                let dates = build_day_key_set(today, 30, 30);
                let (total_rx, total_tx) = sum_days(&self.traffic.global_days, &dates);
                let ranked = configs
                    .iter()
                    .filter_map(|cfg| {
                        let days = self.traffic.config_days.get(&cfg.id)?;
                        let (rx, tx) = sum_days(days, &dates);
                        let total = rx.saturating_add(tx);
                        if total == 0 {
                            None
                        } else {
                            Some(TrafficRankItem {
                                name: cfg.name.clone(),
                                rx_bytes: rx,
                                tx_bytes: tx,
                            })
                        }
                    })
                    .collect::<Vec<_>>();
                (total_rx, total_tx, ranked)
            }
        };

        let mut ranked = ranked;
        ranked.sort_by_key(|item| Reverse(item.total_bytes()));
        let active_configs = ranked.len();
        let top_config_name = ranked.first().map(|item| item.name.clone());
        let top_config_total = ranked
            .first()
            .map(TrafficRankItem::total_bytes)
            .unwrap_or(0);
        let others_total = ranked
            .iter()
            .skip(max_rank_items)
            .fold(0_u64, |acc, item| acc.saturating_add(item.total_bytes()));
        ranked.truncate(max_rank_items);

        TrafficSummaryData {
            total_rx,
            total_tx,
            ranked,
            active_configs,
            others_total,
            top_config_name,
            top_config_total,
        }
    }

    pub(crate) fn overview_traffic_trend(&self, today: NaiveDate) -> TrafficTrendData {
        let mut points = Vec::with_capacity(TRAFFIC_TREND_DAYS);
        for offset in (0..TRAFFIC_TREND_DAYS).rev() {
            let date = today - ChronoDuration::days(offset as i64);
            let day_key = day_key_from_date(date);
            let bytes = self
                .traffic
                .global_days
                .iter()
                .find(|day| day.day_key == day_key)
                .map(|day| day.rx_bytes.saturating_add(day.tx_bytes))
                .unwrap_or(0);
            let label = date.format("%a").to_string();
            points.push(TrafficTrendPoint {
                label,
                bytes,
                is_today: offset == 0,
            });
        }

        let total: u64 = points.iter().map(|point| point.bytes).sum();
        let average_bytes = total as f64 / TRAFFIC_TREND_DAYS as f64;
        let peak = points
            .iter()
            .max_by_key(|point| point.bytes)
            .map(|point| (point.label.clone(), point.bytes));
        let (peak_label, peak_bytes) = peak
            .filter(|(_, bytes)| *bytes > 0)
            .unwrap_or_else(|| ("—".to_string(), 0));
        let max_bytes = peak_bytes;
        let non_zero_days = points.iter().filter(|point| point.bytes > 0).count();

        TrafficTrendData {
            points,
            average_bytes,
            total_bytes: total,
            max_bytes,
            peak_label,
            peak_bytes,
            non_zero_days,
        }
    }
}

fn build_day_key_set(today: NaiveDate, start_offset: i64, days: i64) -> HashSet<i32> {
    let mut set = HashSet::with_capacity(days as usize);
    for offset in start_offset..start_offset + days {
        let date = today - ChronoDuration::days(offset);
        set.insert(day_key_from_date(date));
    }
    set
}

fn sum_days(days: &[TrafficDayBucket], dates: &HashSet<i32>) -> (u64, u64) {
    let mut rx = 0u64;
    let mut tx = 0u64;
    for day in days {
        if dates.contains(&day.day_key) {
            rx = rx.saturating_add(day.rx_bytes);
            tx = tx.saturating_add(day.tx_bytes);
        }
    }
    (rx, tx)
}

fn sum_hours(hours: &[TrafficHourBucket], min_hour: i64, max_hour: i64) -> (u64, u64) {
    let mut rx = 0u64;
    let mut tx = 0u64;
    for hour in hours {
        if hour.hour_key >= min_hour && hour.hour_key <= max_hour {
            rx = rx.saturating_add(hour.rx_bytes);
            tx = tx.saturating_add(hour.tx_bytes);
        }
    }
    (rx, tx)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use chrono::NaiveDate;
    use gpui_component::theme::ThemeMode;

    use super::*;
    use crate::ui::features::themes::AppearancePolicy;
    use crate::ui::state::{ConfigSource, EndpointFamily, TunnelConfig, WgApp};

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
    fn traffic_summary_today_uses_last_24_hours_and_sorts_rankings() {
        let mut app = make_app();
        let current_hour = 1_000;
        app.ui_session.traffic_period = TrafficPeriod::Today;
        app.configs.configs = vec![make_config(1, "alpha"), make_config(2, "beta")];
        app.stats.traffic.global_hours = vec![
            TrafficHourBucket {
                hour_key: current_hour,
                rx_bytes: 50,
                tx_bytes: 5,
            },
            TrafficHourBucket {
                hour_key: current_hour - 23,
                rx_bytes: 10,
                tx_bytes: 1,
            },
            TrafficHourBucket {
                hour_key: current_hour - 24,
                rx_bytes: 999,
                tx_bytes: 999,
            },
        ];
        app.stats.traffic.config_hours.insert(
            1,
            vec![
                TrafficHourBucket {
                    hour_key: current_hour,
                    rx_bytes: 20,
                    tx_bytes: 10,
                },
                TrafficHourBucket {
                    hour_key: current_hour - 24,
                    rx_bytes: 500,
                    tx_bytes: 500,
                },
            ],
        );
        app.stats.traffic.config_hours.insert(
            2,
            vec![TrafficHourBucket {
                hour_key: current_hour - 1,
                rx_bytes: 40,
                tx_bytes: 20,
            }],
        );

        let summary = app.stats.overview_traffic_summary_at(
            &app.configs,
            app.ui_session.traffic_period,
            NaiveDate::from_ymd_opt(2026, 3, 6).expect("valid test date"),
            current_hour,
            7,
        );

        assert_eq!(summary.total_rx, 60);
        assert_eq!(summary.total_tx, 6);
        assert_eq!(summary.active_configs, 2);
        assert_eq!(summary.others_total, 0);
        assert_eq!(summary.top_config_name.as_deref(), Some("beta"));
        assert_eq!(summary.top_config_total, 60);
        assert_eq!(summary.ranked.len(), 2);
        assert_eq!(summary.ranked[0].name, "beta");
        assert_eq!(summary.ranked[0].total_bytes(), 60);
        assert_eq!(summary.ranked[1].name, "alpha");
        assert_eq!(summary.ranked[1].total_bytes(), 30);
    }

    #[test]
    fn traffic_summary_month_windows_split_current_and_previous_periods() {
        let today = NaiveDate::from_ymd_opt(2026, 3, 6).expect("valid test date");
        let mut app = make_app();
        app.configs.configs = vec![make_config(7, "alpha")];
        app.stats.traffic.global_days = vec![
            TrafficDayBucket {
                day_key: day_key_from_date(
                    NaiveDate::from_ymd_opt(2026, 3, 6).expect("valid test date"),
                ),
                rx_bytes: 10,
                tx_bytes: 20,
            },
            TrafficDayBucket {
                day_key: day_key_from_date(
                    NaiveDate::from_ymd_opt(2026, 2, 10).expect("valid test date"),
                ),
                rx_bytes: 30,
                tx_bytes: 40,
            },
            TrafficDayBucket {
                day_key: day_key_from_date(
                    NaiveDate::from_ymd_opt(2026, 1, 20).expect("valid test date"),
                ),
                rx_bytes: 500,
                tx_bytes: 600,
            },
        ];
        app.stats.traffic.config_days.insert(
            7,
            vec![
                TrafficDayBucket {
                    day_key: day_key_from_date(
                        NaiveDate::from_ymd_opt(2026, 3, 6).expect("valid test date"),
                    ),
                    rx_bytes: 10,
                    tx_bytes: 20,
                },
                TrafficDayBucket {
                    day_key: day_key_from_date(
                        NaiveDate::from_ymd_opt(2026, 2, 10).expect("valid test date"),
                    ),
                    rx_bytes: 30,
                    tx_bytes: 40,
                },
                TrafficDayBucket {
                    day_key: day_key_from_date(
                        NaiveDate::from_ymd_opt(2026, 1, 20).expect("valid test date"),
                    ),
                    rx_bytes: 500,
                    tx_bytes: 600,
                },
            ],
        );

        app.ui_session.traffic_period = TrafficPeriod::ThisMonth;
        let this_month = app.stats.overview_traffic_summary_at(
            &app.configs,
            app.ui_session.traffic_period,
            today,
            0,
            7,
        );
        assert_eq!(this_month.total_rx, 40);
        assert_eq!(this_month.total_tx, 60);
        assert_eq!(this_month.active_configs, 1);
        assert_eq!(this_month.others_total, 0);
        assert_eq!(this_month.top_config_name.as_deref(), Some("alpha"));
        assert_eq!(this_month.top_config_total, 100);
        assert_eq!(this_month.ranked[0].total_bytes(), 100);

        app.ui_session.traffic_period = TrafficPeriod::LastMonth;
        let last_month = app.stats.overview_traffic_summary_at(
            &app.configs,
            app.ui_session.traffic_period,
            today,
            0,
            7,
        );
        assert_eq!(last_month.total_rx, 500);
        assert_eq!(last_month.total_tx, 600);
        assert_eq!(last_month.active_configs, 1);
        assert_eq!(last_month.others_total, 0);
        assert_eq!(last_month.top_config_name.as_deref(), Some("alpha"));
        assert_eq!(last_month.top_config_total, 1100);
        assert_eq!(last_month.ranked[0].total_bytes(), 1100);
    }

    #[test]
    fn traffic_summary_keeps_tail_total_when_truncated() {
        let current_hour = 300_i64;
        let mut app = make_app();
        app.configs.configs = vec![
            make_config(1, "alpha"),
            make_config(2, "beta"),
            make_config(3, "gamma"),
        ];
        app.stats.traffic.global_hours = vec![
            TrafficHourBucket {
                hour_key: current_hour,
                rx_bytes: 60,
                tx_bytes: 10,
            },
            TrafficHourBucket {
                hour_key: current_hour - 1,
                rx_bytes: 15,
                tx_bytes: 15,
            },
        ];
        app.stats.traffic.config_hours.insert(
            1,
            vec![TrafficHourBucket {
                hour_key: current_hour,
                rx_bytes: 40,
                tx_bytes: 10,
            }],
        );
        app.stats.traffic.config_hours.insert(
            2,
            vec![TrafficHourBucket {
                hour_key: current_hour,
                rx_bytes: 20,
                tx_bytes: 0,
            }],
        );
        app.stats.traffic.config_hours.insert(
            3,
            vec![TrafficHourBucket {
                hour_key: current_hour - 1,
                rx_bytes: 15,
                tx_bytes: 15,
            }],
        );

        let summary = app.stats.overview_traffic_summary_at(
            &app.configs,
            TrafficPeriod::Today,
            NaiveDate::from_ymd_opt(2026, 3, 6).expect("valid test date"),
            current_hour,
            2,
        );

        assert_eq!(summary.total_rx, 75);
        assert_eq!(summary.total_tx, 25);
        assert_eq!(summary.active_configs, 3);
        assert_eq!(summary.others_total, 20);
        assert_eq!(summary.top_config_name.as_deref(), Some("alpha"));
        assert_eq!(summary.top_config_total, 50);
        assert_eq!(summary.ranked.len(), 2);
        assert_eq!(summary.ranked[0].name, "alpha");
        assert_eq!(summary.ranked[0].total_bytes(), 50);
        assert_eq!(summary.ranked[1].name, "gamma");
        assert_eq!(summary.ranked[1].total_bytes(), 30);
    }

    #[test]
    fn traffic_trend_aggregates_same_day_entries_and_marks_today() {
        let today = NaiveDate::from_ymd_opt(2026, 3, 6).expect("valid test date");
        let mut app = make_app();
        app.stats.traffic.global_days = vec![
            TrafficDayBucket {
                day_key: day_key_from_date(
                    NaiveDate::from_ymd_opt(2026, 3, 6).expect("valid test date"),
                ),
                rx_bytes: 100,
                tx_bytes: 50,
            },
            TrafficDayBucket {
                day_key: day_key_from_date(
                    NaiveDate::from_ymd_opt(2026, 3, 4).expect("valid test date"),
                ),
                rx_bytes: 20,
                tx_bytes: 10,
            },
            TrafficDayBucket {
                day_key: day_key_from_date(
                    NaiveDate::from_ymd_opt(2026, 2, 20).expect("valid test date"),
                ),
                rx_bytes: 999,
                tx_bytes: 0,
            },
        ];

        let trend = app.stats.overview_traffic_trend(today);

        assert_eq!(trend.points.len(), TRAFFIC_TREND_DAYS);
        let today_point = trend.points.last().expect("today should be present");
        assert!(today_point.is_today);
        assert_eq!(today_point.bytes, 150);
        assert_eq!(trend.total_bytes, 180);
        assert_eq!(trend.average_bytes, 180.0 / TRAFFIC_TREND_DAYS as f64);
        assert_eq!(trend.max_bytes, 150);
        assert_eq!(trend.peak_bytes, 150);
        assert_eq!(trend.peak_label, "Fri");
        assert_eq!(trend.non_zero_days, 2);
    }

    #[test]
    fn traffic_trend_reports_sparse_zero_window() {
        let today = NaiveDate::from_ymd_opt(2026, 3, 6).expect("valid test date");
        let app = make_app();

        let trend = app.stats.overview_traffic_trend(today);

        assert_eq!(trend.total_bytes, 0);
        assert_eq!(trend.max_bytes, 0);
        assert_eq!(trend.peak_bytes, 0);
        assert_eq!(trend.peak_label, "—");
        assert_eq!(trend.non_zero_days, 0);
    }
}
