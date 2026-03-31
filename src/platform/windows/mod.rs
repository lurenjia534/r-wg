//! Windows network configuration entry points.
//!
//! The top-level module stays as a facade. Apply/cleanup orchestration lives in
//! dedicated pipeline modules, while adapter/routes/DNS helpers remain in their
//! existing leaf modules.

mod adapter;
mod addresses;
mod apply_pipeline;
mod cleanup_pipeline;
mod dns;
mod error;
mod firewall;
mod metrics;
mod nrpt;
mod recovery;
mod report;
mod routes;
mod sockaddr;

use windows::core::PWSTR;
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, ERROR_OBJECT_ALREADY_EXISTS, WIN32_ERROR};

use crate::core::config::WireGuardConfig;
use crate::core::route_plan::{RouteApplyReport, RoutePlan};

pub use error::NetworkError;
pub use report::AppliedNetworkState;

use recovery::load_persisted_apply_report as load_persisted_apply_report_from_disk;

/// Tunnel interface metric. Lower values win.
pub(super) const TUNNEL_METRIC: u32 = 0;

pub async fn apply_network_config(
    tun_name: &str,
    config: &WireGuardConfig,
    route_plan: &RoutePlan,
) -> Result<crate::platform::NetworkApplyResult, crate::platform::NetworkApplyError> {
    apply_pipeline::apply_network_config(tun_name, config, route_plan).await
}

pub async fn cleanup_network_config(state: AppliedNetworkState) -> Result<(), NetworkError> {
    cleanup_pipeline::cleanup_network_config(state).await
}

/// Windows service startup repair placeholder.
pub fn attempt_startup_repair() -> Result<(), NetworkError> {
    recovery::attempt_startup_repair()
}

pub fn load_persisted_apply_report() -> Option<RouteApplyReport> {
    load_persisted_apply_report_from_disk()
        .ok()
        .flatten()
        .map(|mut report| {
            report.mark_persisted();
            report
        })
}

pub(super) fn pwstr_to_string(ptr: PWSTR) -> String {
    if ptr.0.is_null() {
        return String::new();
    }
    unsafe {
        let mut len = 0usize;
        let mut cursor = ptr.0;
        while *cursor != 0 {
            len += 1;
            cursor = cursor.add(1);
        }
        let slice = std::slice::from_raw_parts(ptr.0, len);
        String::from_utf16_lossy(slice)
    }
}

pub(super) fn is_already_exists(code: WIN32_ERROR) -> bool {
    code == ERROR_OBJECT_ALREADY_EXISTS || code == ERROR_ALREADY_EXISTS
}
