use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use chrono::Local;
use crossbeam_queue::ArrayQueue;

const MAX_LOG_LINES: usize = 2000;

// 使用无锁环形队列保存最近日志，供 UI 快速读取。
static LOG_BUFFER: OnceLock<ArrayQueue<String>> = OnceLock::new();
// 日志缓冲开关默认开启，与 stderr 输出开关独立。
static LOG_BUFFER_ENABLED: AtomicBool = AtomicBool::new(true);

fn buffer() -> &'static ArrayQueue<String> {
    LOG_BUFFER.get_or_init(|| ArrayQueue::new(MAX_LOG_LINES))
}

pub fn buffer_enabled() -> bool {
    LOG_BUFFER_ENABLED.load(Ordering::Relaxed)
}

pub fn set_buffer_enabled(enabled: bool) {
    LOG_BUFFER_ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    // stderr 输出开关；缓冲是否记录由 buffer_enabled 控制。
    std::env::var_os("RWG_LOG").is_some()
}

pub fn log(scope: &str, message: String) {
    let capture = buffer_enabled();
    let print = enabled();
    if !capture && !print {
        return;
    }
    let timestamp = format_timestamp();
    let line = format!("[{timestamp}][r-wg][{scope}] {message}");
    if capture {
        let _ = buffer().force_push(line.clone());
    }
    if print {
        eprintln!("{line}");
    }
}

pub fn snapshot() -> Vec<String> {
    let Some(buffer) = LOG_BUFFER.get() else {
        return Vec::new();
    };
    let mut lines = Vec::with_capacity(MAX_LOG_LINES);
    // 无锁快照：有限次出队再回填；如并发写入导致队列变满则提前停止。
    let count = buffer.len().min(MAX_LOG_LINES);
    for _ in 0..count {
        match buffer.pop() {
            Some(line) => lines.push(line),
            None => break,
        }
    }
    if !lines.is_empty() {
        for line in &lines {
            if buffer.push(line.clone()).is_err() {
                break;
            }
        }
    }
    lines
}

pub fn clear() {
    let Some(buffer) = LOG_BUFFER.get() else {
        return;
    };
    for _ in 0..MAX_LOG_LINES {
        if buffer.pop().is_none() {
            break;
        }
    }
}

fn format_timestamp() -> String {
    // 本地时间戳，便于人工阅读。
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}
