use crate::{log_error, log_info};

pub fn startup() {
    log_info!("app", "app startup");
}

pub fn single_instance_failed(err: &impl std::fmt::Display) {
    log_error!("app", "ui single-instance startup failed: {err}");
}
