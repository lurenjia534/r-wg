use std::time::Duration;

use crate::backend::wg::{Engine, EngineError, EngineStats, EngineStatus, StartRequest};
use crate::core::dns::DnsSelection;
use crate::core::route_plan::RouteApplyReport;

#[derive(Clone)]
pub struct TunnelSessionService {
    engine: Engine,
}

impl TunnelSessionService {
    pub fn new(engine: Engine) -> Self {
        Self { engine }
    }

    pub fn start(&self, request: StartTunnelRequest) -> StartTunnelOutcome {
        let result = self.engine.start(StartRequest::new(
            request.tun_name,
            request.config_text,
            request.dns_selection,
        ));
        let apply_report = self.engine.apply_report().ok().flatten();
        StartTunnelOutcome {
            result,
            apply_report,
        }
    }

    pub fn stop(&self) -> Result<(), EngineError> {
        self.engine.stop()
    }

    pub fn status(&self) -> Result<EngineStatus, EngineError> {
        self.engine.status()
    }

    pub fn stats(&self) -> Result<EngineStats, EngineError> {
        self.engine.stats()
    }

    pub fn apply_report(&self) -> Result<Option<RouteApplyReport>, EngineError> {
        self.engine.apply_report()
    }

    pub fn runtime_snapshot(&self) -> Result<TunnelRuntimeSnapshot, EngineError> {
        Ok(TunnelRuntimeSnapshot {
            status: self.engine.status()?,
            apply_report: self.engine.apply_report().ok().flatten(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct StartTunnelRequest {
    pub tun_name: String,
    pub config_text: String,
    pub dns_selection: DnsSelection,
}

impl StartTunnelRequest {
    pub fn new(
        tun_name: impl Into<String>,
        config_text: impl Into<String>,
        dns_selection: DnsSelection,
    ) -> Self {
        Self {
            tun_name: tun_name.into(),
            config_text: config_text.into(),
            dns_selection,
        }
    }
}

pub struct StartTunnelOutcome {
    pub result: Result<(), EngineError>,
    pub apply_report: Option<RouteApplyReport>,
}

pub struct TunnelRuntimeSnapshot {
    pub status: EngineStatus,
    pub apply_report: Option<RouteApplyReport>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToggleTunnelInput {
    pub busy: bool,
    pub running: bool,
    pub selected_config_id: Option<u64>,
    pub running_config_id: Option<u64>,
    pub draft_has_saved_source: bool,
    pub draft_is_dirty: bool,
    pub restart_delay: Option<Duration>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartBlockedReason {
    SaveDraftFirst,
    SaveChangesFirst,
    SelectTunnelFirst,
}

impl StartBlockedReason {
    pub fn message(self) -> &'static str {
        match self {
            Self::SaveDraftFirst => "Save this draft before starting",
            Self::SaveChangesFirst => "Save changes before starting",
            Self::SelectTunnelFirst => "Select a tunnel first",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToggleTunnelDecision {
    Noop,
    QueuePendingStart { config_id: u64 },
    StopRunning,
    StartSelected {
        config_id: u64,
        restart_delay: Option<Duration>,
    },
    Blocked(StartBlockedReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopSuccessDecision {
    Idle,
    RestartPending { config_id: u64 },
}

pub fn decide_toggle(input: ToggleTunnelInput) -> ToggleTunnelDecision {
    if input.busy {
        return match pending_start_target(input.selected_config_id, input.running_config_id) {
            Some(config_id) if input.running => ToggleTunnelDecision::QueuePendingStart { config_id },
            _ => ToggleTunnelDecision::Noop,
        };
    }

    if input.running {
        return ToggleTunnelDecision::StopRunning;
    }

    if !input.draft_has_saved_source {
        return ToggleTunnelDecision::Blocked(StartBlockedReason::SaveDraftFirst);
    }
    if input.draft_is_dirty {
        return ToggleTunnelDecision::Blocked(StartBlockedReason::SaveChangesFirst);
    }

    match input.selected_config_id {
        Some(config_id) => ToggleTunnelDecision::StartSelected {
            config_id,
            restart_delay: input.restart_delay,
        },
        None => ToggleTunnelDecision::Blocked(StartBlockedReason::SelectTunnelFirst),
    }
}

pub fn decide_after_stop_success(pending_config_id: Option<u64>) -> StopSuccessDecision {
    match pending_config_id {
        Some(config_id) => StopSuccessDecision::RestartPending { config_id },
        None => StopSuccessDecision::Idle,
    }
}

pub fn pending_start_target(selected_config_id: Option<u64>, running_config_id: Option<u64>) -> Option<u64> {
    selected_config_id.or(running_config_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_running_queues_selected_config() {
        let decision = decide_toggle(ToggleTunnelInput {
            busy: true,
            running: true,
            selected_config_id: Some(22),
            running_config_id: Some(11),
            draft_has_saved_source: true,
            draft_is_dirty: false,
            restart_delay: None,
        });

        assert_eq!(
            decision,
            ToggleTunnelDecision::QueuePendingStart { config_id: 22 }
        );
    }

    #[test]
    fn idle_running_requests_stop() {
        let decision = decide_toggle(ToggleTunnelInput {
            busy: false,
            running: true,
            selected_config_id: Some(22),
            running_config_id: Some(11),
            draft_has_saved_source: true,
            draft_is_dirty: false,
            restart_delay: None,
        });

        assert_eq!(decision, ToggleTunnelDecision::StopRunning);
    }

    #[test]
    fn idle_start_requires_saved_draft() {
        let decision = decide_toggle(ToggleTunnelInput {
            busy: false,
            running: false,
            selected_config_id: Some(22),
            running_config_id: None,
            draft_has_saved_source: false,
            draft_is_dirty: false,
            restart_delay: None,
        });

        assert_eq!(
            decision,
            ToggleTunnelDecision::Blocked(StartBlockedReason::SaveDraftFirst)
        );
    }

    #[test]
    fn idle_start_requires_clean_draft() {
        let decision = decide_toggle(ToggleTunnelInput {
            busy: false,
            running: false,
            selected_config_id: Some(22),
            running_config_id: None,
            draft_has_saved_source: true,
            draft_is_dirty: true,
            restart_delay: None,
        });

        assert_eq!(
            decision,
            ToggleTunnelDecision::Blocked(StartBlockedReason::SaveChangesFirst)
        );
    }

    #[test]
    fn idle_start_uses_selected_config_and_delay() {
        let delay = Some(Duration::from_millis(250));
        let decision = decide_toggle(ToggleTunnelInput {
            busy: false,
            running: false,
            selected_config_id: Some(22),
            running_config_id: None,
            draft_has_saved_source: true,
            draft_is_dirty: false,
            restart_delay: delay,
        });

        assert_eq!(
            decision,
            ToggleTunnelDecision::StartSelected {
                config_id: 22,
                restart_delay: delay,
            }
        );
    }

    #[test]
    fn stop_success_restarts_pending_config() {
        assert_eq!(
            decide_after_stop_success(Some(7)),
            StopSuccessDecision::RestartPending { config_id: 7 }
        );
        assert_eq!(decide_after_stop_success(None), StopSuccessDecision::Idle);
    }
}
