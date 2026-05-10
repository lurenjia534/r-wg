use crate::{log_info, log_warn};

pub fn request_sent(request_id: u64, command: &str) {
    log_info!(
        "ipc",
        "backend request sent: id={request_id} command={command}"
    );
}

pub fn request_received(request_id: u64, command: &str) {
    log_info!(
        "ipc",
        "backend request received: id={request_id} command={command}"
    );
}

pub fn request_completed(request_id: u64, command: &str) {
    log_info!(
        "ipc",
        "backend request completed: id={request_id} command={command}"
    );
}

pub fn request_failed(request_id: u64, command: &str, err: &impl std::fmt::Display) {
    log_warn!(
        "ipc",
        "backend request failed: id={request_id} command={command}: {err}"
    );
}

pub fn backend_log_snapshot_requested() {
    log_info!("ipc", "backend log snapshot requested");
}

pub fn backend_log_snapshot_received(line_count: usize) {
    log_info!("ipc", "backend log snapshot received: lines={line_count}");
}

pub fn backend_log_snapshot_failed(err: &impl std::fmt::Display) {
    log_warn!("ipc", "backend log snapshot failed: {err}");
}

pub fn backend_log_clear_requested() {
    log_info!("ipc", "backend log clear requested");
}

pub fn backend_log_clear_failed(err: &impl std::fmt::Display) {
    log_warn!("ipc", "backend log clear failed: {err}");
}
