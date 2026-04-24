use std::io;

use super::ipc::{
    map_backend_error, protocol_mismatch, unexpected_reply, BackendCommand, BackendReply,
    IPC_PROTOCOL_VERSION,
};
use super::{
    EngineError, EngineRuntimeSnapshot, EngineStats, EngineStatus, RelayInventoryStatusSnapshot,
    StartRequest,
};
use crate::core::route_plan::RouteApplyReport;

pub(crate) trait BackendTransport {
    fn send_command_raw(&self, command: BackendCommand) -> Result<BackendReply, io::Error>;
    fn connect_error(&self, err: io::Error) -> EngineError;
    fn is_missing_backend_error(&self, err: &io::Error) -> bool;
    fn is_access_denied_error(&self, err: &io::Error) -> bool;
    fn is_timeout_error(&self, err: &io::Error) -> bool;
}

pub(crate) fn info<T: BackendTransport>(transport: &T) -> Result<u32, EngineError> {
    match transport.send_command_raw(BackendCommand::Info) {
        Ok(BackendReply::Info { protocol_version }) => Ok(protocol_version),
        Ok(BackendReply::Error { kind, message }) => Err(map_backend_error(kind, message)),
        Ok(other) => Err(unexpected_reply(other)),
        Err(err) => Err(map_transport_error(
            transport,
            err,
            Some(EngineError::ChannelClosed),
        )),
    }
}

pub(crate) fn start<T: BackendTransport>(
    transport: &T,
    request: StartRequest,
) -> Result<(), EngineError> {
    check_protocol(transport)?;
    let reply = match transport.send_command_raw(BackendCommand::Start { request }) {
        Ok(reply) => reply,
        Err(err) => {
            if transport.is_timeout_error(&err) {
                attempt_start_cleanup(transport);
            }
            return Err(map_transport_error(transport, err, None));
        }
    };
    expect_unit(reply)
}

pub(crate) fn stop<T: BackendTransport>(
    transport: &T,
    missing_error: EngineError,
) -> Result<(), EngineError> {
    match transport.send_command_raw(BackendCommand::Stop) {
        Ok(reply) => expect_unit(reply),
        Err(err) => Err(map_transport_error(transport, err, Some(missing_error))),
    }
}

pub(crate) fn status<T: BackendTransport>(transport: &T) -> Result<EngineStatus, EngineError> {
    match transport.send_command_raw(BackendCommand::Status) {
        Ok(BackendReply::Status { status }) => Ok(status),
        Ok(BackendReply::Error { kind, message }) => Err(map_backend_error(kind, message)),
        Ok(other) => Err(unexpected_reply(other)),
        Err(err) => {
            if transport.is_missing_backend_error(&err) {
                Ok(EngineStatus::Stopped)
            } else {
                Err(map_transport_error(transport, err, None))
            }
        }
    }
}

pub(crate) fn stats<T: BackendTransport>(
    transport: &T,
    missing_error: EngineError,
) -> Result<EngineStats, EngineError> {
    match transport.send_command_raw(BackendCommand::Stats) {
        Ok(BackendReply::Stats { stats }) => Ok(stats),
        Ok(BackendReply::Error { kind, message }) => Err(map_backend_error(kind, message)),
        Ok(other) => Err(unexpected_reply(other)),
        Err(err) => Err(map_transport_error(transport, err, Some(missing_error))),
    }
}

pub(crate) fn apply_report<T: BackendTransport>(
    transport: &T,
    missing_error: EngineError,
) -> Result<Option<RouteApplyReport>, EngineError> {
    check_protocol(transport)?;
    match transport.send_command_raw(BackendCommand::ApplyReport) {
        Ok(BackendReply::ApplyReport { report }) => Ok(report),
        Ok(BackendReply::Error { kind, message }) => Err(map_backend_error(kind, message)),
        Ok(other) => Err(unexpected_reply(other)),
        Err(err) => Err(map_transport_error(transport, err, Some(missing_error))),
    }
}

pub(crate) fn runtime_snapshot<T: BackendTransport>(
    transport: &T,
    missing_error: EngineError,
) -> Result<EngineRuntimeSnapshot, EngineError> {
    check_protocol(transport)?;
    match transport.send_command_raw(BackendCommand::RuntimeSnapshot) {
        Ok(BackendReply::RuntimeSnapshot { snapshot }) => Ok(snapshot),
        Ok(BackendReply::Error { kind, message }) => Err(map_backend_error(kind, message)),
        Ok(other) => Err(unexpected_reply(other)),
        Err(err) => Err(map_transport_error(transport, err, Some(missing_error))),
    }
}

pub(crate) fn relay_inventory_status<T: BackendTransport>(
    transport: &T,
    missing_error: EngineError,
) -> Result<RelayInventoryStatusSnapshot, EngineError> {
    check_protocol(transport)?;
    match transport.send_command_raw(BackendCommand::RelayInventoryStatus) {
        Ok(BackendReply::RelayInventoryStatus { snapshot }) => Ok(snapshot),
        Ok(BackendReply::Error { kind, message }) => Err(map_backend_error(kind, message)),
        Ok(other) => Err(unexpected_reply(other)),
        Err(err) => Err(map_transport_error(transport, err, Some(missing_error))),
    }
}

pub(crate) fn refresh_relay_inventory<T: BackendTransport>(
    transport: &T,
    missing_error: EngineError,
) -> Result<RelayInventoryStatusSnapshot, EngineError> {
    check_protocol(transport)?;
    match transport.send_command_raw(BackendCommand::RefreshRelayInventory) {
        Ok(BackendReply::RelayInventoryStatus { snapshot }) => Ok(snapshot),
        Ok(BackendReply::Error { kind, message }) => Err(map_backend_error(kind, message)),
        Ok(other) => Err(unexpected_reply(other)),
        Err(err) => Err(map_transport_error(transport, err, Some(missing_error))),
    }
}

fn check_protocol<T: BackendTransport>(transport: &T) -> Result<(), EngineError> {
    let protocol_version = info(transport)?;
    if protocol_version == IPC_PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(protocol_mismatch(IPC_PROTOCOL_VERSION, protocol_version))
    }
}

fn expect_unit(reply: BackendReply) -> Result<(), EngineError> {
    match reply {
        BackendReply::Ok => Ok(()),
        BackendReply::Error { kind, message } => Err(map_backend_error(kind, message)),
        other => Err(unexpected_reply(other)),
    }
}

fn map_transport_error<T: BackendTransport>(
    transport: &T,
    err: io::Error,
    missing_error: Option<EngineError>,
) -> EngineError {
    if transport.is_access_denied_error(&err) {
        return EngineError::AccessDenied;
    }
    if transport.is_missing_backend_error(&err) {
        if let Some(missing_error) = missing_error {
            return missing_error;
        }
    }
    transport.connect_error(err)
}

fn attempt_start_cleanup<T: BackendTransport>(transport: &T) {
    match transport.send_command_raw(BackendCommand::Stop) {
        Ok(_) => {}
        Err(err) if transport.is_missing_backend_error(&err) => {}
        Err(err) if transport.is_timeout_error(&err) => {}
        Err(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::io;

    use super::*;
    use crate::backend::wg::{DaitaMode, QuantumMode};
    use crate::core::dns::{DnsMode, DnsPreset, DnsSelection};

    #[derive(Default)]
    struct MockTransport {
        commands: RefCell<Vec<BackendCommand>>,
    }

    impl BackendTransport for MockTransport {
        fn send_command_raw(&self, command: BackendCommand) -> Result<BackendReply, io::Error> {
            self.commands.borrow_mut().push(command.clone());
            match command {
                BackendCommand::Info => Ok(BackendReply::Info {
                    protocol_version: IPC_PROTOCOL_VERSION,
                }),
                BackendCommand::Start { .. } => Err(io::Error::from(io::ErrorKind::TimedOut)),
                BackendCommand::Stop => Ok(BackendReply::Ok),
                other => panic!("unexpected command: {other:?}"),
            }
        }

        fn connect_error(&self, err: io::Error) -> EngineError {
            EngineError::Remote(err.to_string())
        }

        fn is_missing_backend_error(&self, _err: &io::Error) -> bool {
            false
        }

        fn is_access_denied_error(&self, _err: &io::Error) -> bool {
            false
        }

        fn is_timeout_error(&self, err: &io::Error) -> bool {
            matches!(
                err.kind(),
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
            )
        }
    }

    #[test]
    fn start_timeout_queues_cleanup_stop() {
        let transport = MockTransport::default();

        let result = start(
            &transport,
            StartRequest::new(
                "wg0",
                "[Interface]\n",
                DnsSelection::new(DnsMode::FollowConfig, DnsPreset::CloudflareStandard),
                QuantumMode::Off,
                DaitaMode::Off,
                true,
            ),
        );

        assert!(result.is_err());
        let commands = transport.commands.borrow();
        assert!(matches!(
            commands.as_slice(),
            [
                BackendCommand::Info,
                BackendCommand::Start { .. },
                BackendCommand::Stop
            ]
        ));
    }
}
