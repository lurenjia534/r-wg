use crate::{log_error, log_info, log_warn};

pub fn linux_service_starting() {
    log_info!("service", "linux privileged backend service starting");
}

pub fn linux_service_listening() {
    log_info!("service", "linux privileged backend service listening");
}

pub fn linux_client_denied() {
    log_warn!("service", "linux privileged backend client denied");
}

pub fn linux_client_failed(err: &impl std::fmt::Display) {
    log_warn!("service", "linux service client handling failed: {err}");
}

pub fn linux_accept_failed(err: &impl std::fmt::Display) {
    log_warn!("service", "linux service accept failed: {err}");
}

pub fn linux_spawn_failed(err: &impl std::fmt::Display) {
    log_warn!(
        "service",
        "failed to spawn linux service client worker: {err}"
    );
}

pub fn shutdown_stop_failed(err: &impl std::fmt::Display) {
    log_warn!(
        "service",
        "failed to stop engine during service shutdown: {err}"
    );
}

pub fn windows_service_starting() {
    log_info!("service", "windows privileged backend service starting");
}

pub fn windows_service_running() {
    log_info!("service", "windows privileged backend service running");
}

pub fn windows_service_failed(err: &impl std::fmt::Display) {
    log_error!("service", "windows service failed: {err}");
}

pub fn windows_client_failed(err: &impl std::fmt::Display) {
    log_warn!("service", "windows service client handling failed: {err}");
}

pub fn windows_spawn_failed(err: &impl std::fmt::Display) {
    log_warn!(
        "service",
        "failed to spawn windows service client worker: {err}"
    );
}
