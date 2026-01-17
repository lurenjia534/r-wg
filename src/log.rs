use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

use chrono::Local;

const MAX_LOG_LINES: usize = 2000;

#[derive(Default)]
struct LogBuffer {
    lines: VecDeque<String>,
}

fn buffer() -> &'static Mutex<LogBuffer> {
    static LOG_BUFFER: OnceLock<Mutex<LogBuffer>> = OnceLock::new();
    LOG_BUFFER.get_or_init(|| {
        Mutex::new(LogBuffer {
            lines: VecDeque::with_capacity(MAX_LOG_LINES),
        })
    })
}

pub fn enabled() -> bool {
    std::env::var_os("RWG_LOG").is_some()
}

pub fn log(scope: &str, message: String) {
    let timestamp = format_timestamp();
    let line = format!("[{timestamp}][r-wg][{scope}] {message}");
    if let Ok(mut buffer) = buffer().lock() {
        if buffer.lines.len() >= MAX_LOG_LINES {
            buffer.lines.pop_front();
        }
        buffer.lines.push_back(line.clone());
    }
    if enabled() {
        eprintln!("{line}");
    }
}

pub fn snapshot() -> Vec<String> {
    buffer()
        .lock()
        .map(|buffer| buffer.lines.iter().cloned().collect())
        .unwrap_or_default()
}

pub fn clear() {
    if let Ok(mut buffer) = buffer().lock() {
        buffer.lines.clear();
    }
}

fn format_timestamp() -> String {
    // Local timestamp for human-readable logs.
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}
