use std::any::Any;
use std::panic::AssertUnwindSafe;

use futures_util::FutureExt;
use tokio::sync::mpsc;

use crate::log::events::engine as log_engine;
use crate::platform;

use super::super::relay_inventory;
use super::command::Command;
use super::snapshot::map_relay_inventory_status_snapshot;
use super::state::EngineState;
use super::{EngineError, StartRequest};

/// 后台线程的主事件循环：
/// 接收命令并顺序执行，通道关闭后安全收尾。
///
/// 该循环是引擎的“串行化核心”，避免并发修改内部状态。
pub(super) async fn run(mut rx: mpsc::Receiver<Command>) {
    let mut state = EngineState {
        route_apply_report: platform::load_persisted_apply_report(),
        ..Default::default()
    };

    while let Some(command) = rx.recv().await {
        match command {
            Command::Start(request, reply) => {
                let result = catch_start_panic(&mut state, request).await;
                match &result {
                    Ok(()) => log_engine::tunnel_started(),
                    Err(err) => log_engine::tunnel_start_failed(err),
                }
                let _ = reply.send(result);
            }
            Command::Stop(reply) => {
                let result = state.stop().await;
                if let Err(err) = &result {
                    log_engine::stop_failed(err);
                }
                let _ = reply.send(result);
            }
            Command::Status(reply) => {
                let _ = reply.send(state.status());
            }
            Command::Stats(reply) => {
                let result = state.stats().await;
                let _ = reply.send(result);
            }
            Command::ApplyReport(reply) => {
                let _ = reply.send(state.apply_report());
            }
            Command::RuntimeSnapshot(reply) => {
                let _ = reply.send(state.runtime_snapshot());
            }
            Command::RelayInventoryStatus(reply) => {
                let _ = reply.send(state.relay_inventory_status());
            }
            Command::RefreshRelayInventory(reply) => {
                tokio::spawn(async move {
                    let result = relay_inventory::refresh_cache()
                        .await
                        .map(map_relay_inventory_status_snapshot)
                        .map_err(|error| EngineError::Remote(error.to_string()));
                    let _ = reply.send(result);
                });
            }
        }
    }

    let _ = state.stop().await;
    state.shutdown_active_backend().await;
    state.shutdown_cached_userspace_device().await;
}

async fn catch_start_panic(
    state: &mut EngineState,
    request: StartRequest,
) -> Result<(), EngineError> {
    match AssertUnwindSafe(state.start(request)).catch_unwind().await {
        Ok(result) => result,
        Err(payload) => {
            let message = format!(
                "backend worker panicked while starting tunnel: {}",
                panic_payload_message(payload)
            );
            log_engine::worker_panic(&message);
            recover_after_worker_panic(state).await;
            Err(EngineError::Remote(message))
        }
    }
}

async fn recover_after_worker_panic(state: &mut EngineState) {
    let apply_report = state.apply_report();
    if let Err(err) = state.cleanup_active_network_state().await {
        log_engine::panic_cleanup_failed(&err);
    }
    state.shutdown_active_backend().await;
    state.shutdown_cached_userspace_device().await;
    state.route_apply_report = apply_report;
}

fn panic_payload_message(payload: Box<dyn Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_string()
    }
}
