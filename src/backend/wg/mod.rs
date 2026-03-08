/// WireGuard 后端统一导出层。
///
/// - Linux / 其它非 Windows 平台：直接导出原本的本地引擎；
/// - Windows：导出带按需提权能力的门面引擎；
/// - `maybe_run_elevated_helper` 只在 Windows 有实际行为，其它平台恒为 no-op。
pub mod config;
mod engine;
#[cfg(target_os = "windows")]
mod windows_elevated;

#[cfg(not(target_os = "windows"))]
pub use engine::Engine;
pub use engine::{EngineError, EngineStats, EngineStatus, PeerStats, StartRequest};
#[cfg(target_os = "windows")]
pub use windows_elevated::Engine;

#[cfg(target_os = "windows")]
pub use windows_elevated::maybe_run_elevated_helper;

#[cfg(not(target_os = "windows"))]
pub fn maybe_run_elevated_helper() -> bool {
    false
}
