use std::time::{Duration, Instant};

use r_wg::backend::wg::{ActiveBackendStatus, EphemeralFailureKind};
use r_wg::core::route_plan::RouteApplyReport;

use super::{PendingStart, TunnelConfig, RESTART_COOLDOWN};

pub(crate) struct RuntimeState {
    /// 是否处于运行中。
    pub(crate) running: bool,
    /// 是否有异步流程正在执行。
    pub(crate) busy: bool,
    /// 停止过程中记录的“待启动”请求。
    pub(crate) pending_start: Option<PendingStart>,
    /// 最近一次停止完成的时间（用于冷却启动）。
    pub(crate) last_stop_at: Option<Instant>,
    pub(crate) running_name: Option<String>,
    pub(crate) running_id: Option<u64>,
    pub(crate) active_backend: Option<ActiveBackendStatus>,
    pub(crate) quantum_protected: bool,
    pub(crate) last_quantum_failure: Option<EphemeralFailureKind>,
    pub(crate) daita_active: bool,
    pub(crate) last_daita_failure: Option<EphemeralFailureKind>,
    pub(crate) last_apply_report: Option<RouteApplyReport>,
    pub(crate) runtime_revision: u64,
}

impl RuntimeState {
    pub(super) fn new() -> Self {
        Self {
            running: false,
            busy: false,
            pending_start: None,
            last_stop_at: None,
            running_name: None,
            running_id: None,
            active_backend: None,
            quantum_protected: false,
            last_quantum_failure: None,
            daita_active: false,
            last_daita_failure: None,
            last_apply_report: None,
            runtime_revision: 0,
        }
    }

    pub(crate) fn restart_delay(&self) -> Option<Duration> {
        let last_stop = self.last_stop_at?;
        let elapsed = last_stop.elapsed();
        if elapsed >= RESTART_COOLDOWN {
            None
        } else {
            Some(RESTART_COOLDOWN - elapsed)
        }
    }

    pub(crate) fn queue_pending_start(&mut self, pending: Option<PendingStart>) -> bool {
        let Some(pending) = pending else {
            return false;
        };
        self.pending_start = Some(pending);
        true
    }

    pub(crate) fn begin_stop(&mut self) {
        self.busy = true;
        self.runtime_revision = self.runtime_revision.wrapping_add(1);
    }

    pub(crate) fn finish_stop_success(&mut self) {
        self.busy = false;
        self.running = false;
        self.running_name = None;
        self.running_id = None;
        self.active_backend = None;
        self.quantum_protected = false;
        self.last_quantum_failure = None;
        self.daita_active = false;
        self.last_daita_failure = None;
        self.clear_last_apply_report();
        self.last_stop_at = Some(Instant::now());
        self.runtime_revision = self.runtime_revision.wrapping_add(1);
    }

    pub(crate) fn finish_stop_failure(&mut self) {
        self.busy = false;
        self.pending_start = None;
        self.runtime_revision = self.runtime_revision.wrapping_add(1);
    }

    pub(crate) fn begin_start(&mut self) {
        self.busy = true;
        self.quantum_protected = false;
        self.active_backend = None;
        self.last_quantum_failure = None;
        self.daita_active = false;
        self.last_daita_failure = None;
        self.runtime_revision = self.runtime_revision.wrapping_add(1);
    }

    pub(crate) fn finish_start_attempt(&mut self) {
        self.busy = false;
        self.runtime_revision = self.runtime_revision.wrapping_add(1);
    }

    pub(crate) fn mark_started(&mut self, selected: &TunnelConfig) {
        self.running = true;
        self.running_name = Some(selected.name.clone());
        self.running_id = Some(selected.id);
        self.runtime_revision = self.runtime_revision.wrapping_add(1);
    }

    pub(crate) fn set_active_backend(&mut self, backend: Option<ActiveBackendStatus>) {
        self.active_backend = backend;
        self.runtime_revision = self.runtime_revision.wrapping_add(1);
    }

    pub(crate) fn set_quantum_status(
        &mut self,
        protected: bool,
        failure: Option<EphemeralFailureKind>,
    ) {
        self.quantum_protected = protected;
        self.last_quantum_failure = failure;
        self.runtime_revision = self.runtime_revision.wrapping_add(1);
    }

    pub(crate) fn set_daita_status(&mut self, active: bool, failure: Option<EphemeralFailureKind>) {
        self.daita_active = active;
        self.last_daita_failure = failure;
        self.runtime_revision = self.runtime_revision.wrapping_add(1);
    }

    pub(crate) fn set_last_apply_report(&mut self, report: Option<RouteApplyReport>) {
        self.last_apply_report = report;
        self.runtime_revision = self.runtime_revision.wrapping_add(1);
    }

    pub(crate) fn clear_last_apply_report(&mut self) {
        self.last_apply_report = None;
        self.runtime_revision = self.runtime_revision.wrapping_add(1);
    }
}
