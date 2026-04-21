use super::engine::Engine as LocalEngine;
use super::ipc::{
    error_reply, option_reply, relay_inventory_status_reply, runtime_snapshot_reply, unit_reply,
    BackendCommand, BackendReply, IPC_PROTOCOL_VERSION,
};

pub(crate) fn dispatch_command(engine: &LocalEngine, command: BackendCommand) -> BackendReply {
    match command {
        BackendCommand::Ping => BackendReply::Ok,
        BackendCommand::Info => BackendReply::Info {
            protocol_version: IPC_PROTOCOL_VERSION,
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
    }
}
