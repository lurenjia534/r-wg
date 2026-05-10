use std::sync::{Mutex, Once, OnceLock};

use r_wg::log::{self, LogLevel};

// 与日志模块容量保持一致，便于断言溢出行为。
const MAX_LOG_LINES: usize = 2000;

static INIT: Once = Once::new();
static LOCK: OnceLock<Mutex<()>> = OnceLock::new();

// 只初始化一次全局日志（避免多次设置全局 subscriber 造成 panic）。
fn test_init() {
    INIT.call_once(|| {
        // 先关闭缓冲，验证运行时开启是否生效。
        let config = log::LogConfig::builder()
            .level(LogLevel::Info)
            .stderr_enabled(true)
            .buffer_enabled(false)
            .scopes(["app", "net", "engine", "ui", "ipc", "service"])
            .build();
        let _ = log::init_with(config);
    });
}

// 串行化测试，避免并发读写缓冲导致断言不稳定。
fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}

#[test]
fn enabled_for_respects_level_and_scopes() {
    let _guard = test_lock();
    test_init();
    assert!(log::enabled_for(LogLevel::Info, "net"));
    assert!(!log::enabled_for(LogLevel::Debug, "net"));
    assert!(!log::enabled_for(LogLevel::Info, "dns"));
}

#[test]
fn buffer_overflow_keeps_capacity() {
    let _guard = test_lock();
    test_init();
    log::set_buffer_enabled(true);
    log::clear();
    for idx in 0..(MAX_LOG_LINES + 32) {
        log::event(LogLevel::Info, "net", format_args!("line-{idx}"));
    }
    let lines = log::snapshot();
    assert!(lines.len() <= MAX_LOG_LINES);
}

#[test]
fn formatting_contains_scope_and_message() {
    let _guard = test_lock();
    test_init();
    log::set_buffer_enabled(true);
    log::clear();
    log::event(LogLevel::Info, "net", format_args!("hello"));
    let lines = log::snapshot();
    let last = lines.last().expect("log line captured");
    assert!(last.ends_with("[r-wg][net] hello"));
}

#[test]
fn tracing_targets_under_app_namespace_are_captured_and_scoped() {
    let _guard = test_lock();
    test_init();
    log::set_buffer_enabled(true);
    log::clear();

    tracing::warn!(target: "r_wg", scope = "net", "root target");
    tracing::warn!(target: "r_wg::ui::startup", "ui target");
    tracing::warn!(target: "r_wg::backend::wg::engine", "backend target");
    tracing::warn!(target: "r_wg::backend::wg::ipc_server", "ipc target");
    tracing::warn!(target: "r_wg::backend::wg::linux_service::server", "service target");
    tracing::warn!(target: "some_external_crate", "external target");

    let lines = log::snapshot();
    assert!(lines
        .iter()
        .any(|line| line.ends_with("[r-wg][net] root target")));
    assert!(lines
        .iter()
        .any(|line| line.ends_with("[r-wg][ui] ui target")));
    assert!(lines
        .iter()
        .any(|line| line.ends_with("[r-wg][engine] backend target")));
    assert!(lines
        .iter()
        .any(|line| line.ends_with("[r-wg][ipc] ipc target")));
    assert!(lines
        .iter()
        .any(|line| line.ends_with("[r-wg][service] service target")));
    assert!(!lines.iter().any(|line| line.contains("external target")));
}
