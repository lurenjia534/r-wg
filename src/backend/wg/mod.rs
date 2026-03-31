/// WireGuard backend export layer.
///
/// - Linux: use the privileged service backend.
/// - Windows: use the SCM service backend.
/// - Other platforms: fall back to the local engine.
pub mod config;
mod engine;
mod ipc;
mod ipc_client;
mod ipc_server;
#[cfg(target_os = "linux")]
mod linux_service;
pub mod route_plan;
pub mod tools;
#[cfg(target_os = "windows")]
mod windows_pipe;
#[cfg(target_os = "windows")]
mod windows_service;
#[cfg(target_os = "windows")]
mod windows_service_host;
#[cfg(target_os = "windows")]
mod windows_service_manager;

#[cfg(all(not(target_os = "windows"), not(target_os = "linux")))]
pub use engine::Engine;
pub use engine::{EngineError, EngineStats, EngineStatus, PeerStats, StartRequest};
pub use route_plan::{
    collect_allowed_routes, detect_full_tunnel, linux_default_policy_table_id,
    linux_policy_table_id, linux_route_table_for, windows_planned_bypass_count, FullTunnelStatus,
    OperationalRoutePlan, RouteApplyAttemptState, RouteApplyEntry, RouteApplyFailureKind,
    RouteApplyKind, RouteApplyPhase, RouteApplyReport, RouteApplyReportSource, RouteApplyStatus,
    RoutePlan, RoutePlanPlatform, LINUX_DEFAULT_POLICY_TABLE_ID,
};

#[cfg(target_os = "linux")]
pub use linux_service::{
    manage_privileged_service, maybe_run_service_mode, probe_privileged_service, Engine,
    PrivilegedServiceAction, PrivilegedServiceStatus,
};

#[cfg(target_os = "windows")]
pub use windows_service::{
    manage_privileged_service, maybe_run_service_mode, probe_privileged_service, Engine,
    PrivilegedServiceAction, PrivilegedServiceStatus,
};

#[cfg(all(not(target_os = "windows"), not(target_os = "linux")))]
pub fn maybe_run_privileged_backend() -> bool {
    false
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
pub fn maybe_run_privileged_backend() -> bool {
    maybe_run_service_mode()
}

#[cfg(all(not(target_os = "windows"), not(target_os = "linux")))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrivilegedServiceStatus {
    Unsupported,
}

#[cfg(all(not(target_os = "windows"), not(target_os = "linux")))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivilegedServiceAction {
    Install,
    Repair,
    Remove,
}

#[cfg(all(not(target_os = "windows"), not(target_os = "linux")))]
pub fn probe_privileged_service() -> PrivilegedServiceStatus {
    PrivilegedServiceStatus::Unsupported
}

#[cfg(all(not(target_os = "windows"), not(target_os = "linux")))]
pub fn manage_privileged_service(_action: PrivilegedServiceAction) -> Result<(), EngineError> {
    Err(EngineError::Remote(
        "privileged backend management is not supported on this platform".to_string(),
    ))
}
