// Shared UI-state constants.

/// 速度曲线采样点数量（固定窗口）。
pub(crate) const SPARKLINE_SAMPLES: usize = 24;
/// 7 日流量趋势展示天数。
pub(crate) const TRAFFIC_TREND_DAYS: usize = 7;
/// Traffic Summary 的滚动天数（过去 30 天 + 前 30 天）。
pub(crate) const TRAFFIC_ROLLING_DAYS: usize = 60;
/// Traffic Summary 的滚动小时数（过去 24 小时，预留 48 小时）。
pub(crate) const TRAFFIC_HOURLY_HISTORY: usize = 48;
/// stop -> start 的最短冷却时间。
pub(crate) const RESTART_COOLDOWN: Duration = Duration::from_millis(300);
pub(crate) const DEFAULT_CONFIGS_LIBRARY_WIDTH: f32 = 300.0;
pub(crate) const DEFAULT_CONFIGS_INSPECTOR_WIDTH: f32 = 332.0;
pub(crate) const DEFAULT_ROUTE_MAP_INVENTORY_WIDTH: f32 = 280.0;
pub(crate) const DEFAULT_ROUTE_MAP_INSPECTOR_WIDTH: f32 = 340.0;
