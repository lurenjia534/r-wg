use crate::{log_info, log_warn};

pub fn connect_button_clicked() {
    log_info!("ui", "connect button clicked");
}

pub fn tunnel_start_requested(name: &str) {
    log_info!("ui", "tunnel start requested: {name}");
}

pub fn tunnel_started(name: &str) {
    log_info!("ui", "tunnel started: {name}");
}

pub fn tunnel_start_failed(message: &str) {
    log_warn!("ui", "tunnel start failed: {message}");
}

pub fn tunnel_stop_requested() {
    log_info!("ui", "tunnel stop requested");
}

pub fn tunnel_stopped() {
    log_info!("ui", "tunnel stopped");
}

pub fn tunnel_stop_failed(message: &str) {
    log_warn!("ui", "tunnel stop failed: {message}");
}

pub fn tunnel_start_blocked(message: &str) {
    log_warn!("ui", "tunnel start blocked: {message}");
}

pub fn logs_opened() {
    log_info!("ui", "logs page opened");
}

pub fn logs_cleared() {
    log_info!("ui", "logs cleared");
}

pub fn logs_copied(line_count: usize) {
    log_info!("ui", "logs copied: lines={line_count}");
}
