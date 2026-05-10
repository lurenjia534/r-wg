use super::engine::Engine as LocalEngine;
use super::ipc::{
    backend_capabilities, backend_platform, backend_service_version, error_reply, option_reply,
    relay_inventory_status_reply, runtime_snapshot_reply, unit_reply, BackendCommand, BackendReply,
    BackendRequest, IPC_PROTOCOL_VERSION,
};
use crate::log::events::ipc as log_ipc;

pub(crate) fn dispatch_request(engine: &LocalEngine, request: BackendRequest) -> BackendReply {
    let request_id = request.request_id;
    let command_name = request.command.name();
    log_ipc::request_received(request_id, command_name);
    let reply = match request.command {
        BackendCommand::Ping => BackendReply::Ok,
        BackendCommand::Info => BackendReply::Info {
            protocol_version: IPC_PROTOCOL_VERSION,
            service_version: backend_service_version(),
            platform: backend_platform(),
            capabilities: backend_capabilities(),
        },
        BackendCommand::Start { request } => unit_reply(engine.start(request)),
        BackendCommand::Stop => unit_reply(engine.stop()),
        BackendCommand::Status => match engine.status() {
            Ok(status) => BackendReply::Status { status },
            Err(err) => error_reply(err),
        },
        BackendCommand::Stats => match engine.stats() {
            Ok(stats) => BackendReply::Stats { stats },
            Err(err) => error_reply(err),
        },
        BackendCommand::ApplyReport => option_reply(engine.apply_report()),
        BackendCommand::RuntimeSnapshot => runtime_snapshot_reply(engine.runtime_snapshot()),
        BackendCommand::RelayInventoryStatus => {
            relay_inventory_status_reply(engine.relay_inventory_status())
        }
        BackendCommand::RefreshRelayInventory => {
            relay_inventory_status_reply(engine.refresh_relay_inventory())
        }
        BackendCommand::LogSnapshot => BackendReply::LogSnapshot {
            lines: {
                log_ipc::backend_log_snapshot_requested();
                crate::log::snapshot_for_ipc()
            },
        },
        BackendCommand::LogClear => {
            log_ipc::backend_log_clear_requested();
            crate::log::clear();
            BackendReply::Ok
        }
    };
    log_ipc::request_completed(request_id, command_name);
    reply
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, OnceLock};

    use super::*;

    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    fn dispatch_test_command(engine: &LocalEngine, command: BackendCommand) -> BackendReply {
        dispatch_request(
            engine,
            BackendRequest {
                request_id: 1,
                command,
            },
        )
    }

    #[test]
    fn dispatch_log_snapshot_returns_backend_buffer() {
        let _guard = test_lock();
        let engine = LocalEngine::new();
        crate::log::clear();
        crate::log::event(
            crate::log::LogLevel::Info,
            "service",
            format_args!("server-line"),
        );

        let reply = dispatch_test_command(&engine, BackendCommand::LogSnapshot);

        match reply {
            BackendReply::LogSnapshot { lines } => {
                assert!(lines
                    .iter()
                    .any(|line| line.ends_with("[r-wg][service] server-line")));
            }
            other => panic!("unexpected reply: {other:?}"),
        }
    }

    #[test]
    fn dispatch_log_snapshot_redacts_sensitive_values() {
        let _guard = test_lock();
        let engine = LocalEngine::new();
        crate::log::clear();
        crate::log::event(
            crate::log::LogLevel::Info,
            "service",
            format_args!("PrivateKey=server-secret token:ipc-secret visible"),
        );

        let reply = dispatch_test_command(&engine, BackendCommand::LogSnapshot);

        match reply {
            BackendReply::LogSnapshot { lines } => {
                let line = lines
                    .iter()
                    .find(|line| line.contains("PrivateKey="))
                    .expect("backend log line");
                assert!(line.contains("PrivateKey=<redacted>"));
                assert!(line.contains("token:<redacted>"));
                assert!(line.contains("visible"));
                assert!(!line.contains("server-secret"));
                assert!(!line.contains("ipc-secret"));
            }
            other => panic!("unexpected reply: {other:?}"),
        }
    }

    #[test]
    fn dispatch_log_clear_clears_backend_buffer() {
        let _guard = test_lock();
        let engine = LocalEngine::new();
        crate::log::event(
            crate::log::LogLevel::Info,
            "service",
            format_args!("clear-me"),
        );

        let reply = dispatch_test_command(&engine, BackendCommand::LogClear);

        assert!(matches!(reply, BackendReply::Ok));
        assert!(!crate::log::snapshot()
            .iter()
            .any(|line| line.contains("clear-me")));
    }
}
