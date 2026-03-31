use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use futures_util::stream::{self, StreamExt as _, TryStreamExt as _};
use gpui::{AppContext as _, Context, Timer};
use r_wg::backend::wg::tools::format_endpoint_display;
use r_wg::core::config::{self, PeerConfig};
use tokio::runtime::Builder;

use super::active_config::resolve_active_config_text_request;
use super::reachability::run_reachability_probe_with_cancel_async;
use crate::ui::state::{
    AsyncJobState, JobCancelHandle, ReachabilityAuditPhase, ReachabilityAuditProgress,
    ReachabilityAuditRequest, ReachabilityAuditViewModel, ReachabilityBatchResult,
    ReachabilityBatchRow, ReachabilityBatchStatus, ToolsWorkspace,
};

const REACHABILITY_BATCH_CONCURRENCY: usize = 24;
const AUDIT_PROGRESS_POLL_INTERVAL: Duration = Duration::from_millis(100);

type SharedAuditProgress = Arc<Mutex<ReachabilityAuditProgress>>;

struct BatchEndpointJob {
    config_name: String,
    peer_label: String,
    target: String,
}

impl ToolsWorkspace {
    pub(crate) fn run_reachability_audit(&mut self, cx: &mut Context<Self>) {
        if self.reachability.single.is_running() || self.reachability.audit.is_running() {
            return;
        }

        let audit_request = match self.current_reachability_audit_request(cx) {
            Ok(request) => request,
            Err(message) => {
                self.reachability.audit_error = Some(message.into());
                cx.notify();
                return;
            }
        };

        let requests = self.app.read(cx).saved_config_text_requests();
        if requests.is_empty() {
            self.reachability.audit_error = Some("No saved configs are available.".into());
            cx.notify();
            return;
        }

        self.reachability.audit_generation = self.reachability.audit_generation.wrapping_add(1);
        let generation = self.reachability.audit_generation;
        let cancel = self.reachability.audit.set_running(generation);
        let progress = Arc::new(Mutex::new(ReachabilityAuditProgress {
            phase: ReachabilityAuditPhase::LoadingConfigs,
            total_configs: requests.len(),
            processed_configs: 0,
            total_endpoints: 0,
            completed_endpoints: 0,
        }));
        self.reachability.audit_progress = Some(progress_snapshot(&progress));
        self.reachability.audit_error = None;
        self.reachability.audit_notice = None;
        self.reachability.audit_cancelling = false;
        cx.notify();

        self.spawn_audit_progress_poller(generation, progress.clone(), cx);

        cx.spawn(async move |view, cx| {
            let progress_for_task = progress.clone();
            let task = cx.background_spawn(async move {
                build_batch_reachability_result_blocking(
                    requests,
                    audit_request,
                    cancel,
                    progress_for_task,
                )
            });
            let result = task.await;
            let final_progress = progress_snapshot(&progress);
            let _ = view.update(cx, |this, cx| {
                if this.reachability.audit.generation() != Some(generation) {
                    return;
                }
                match result {
                    Ok(view_model) => {
                        this.reachability.audit = AsyncJobState::Ready(view_model);
                        this.reachability.audit_progress = Some(final_progress);
                        this.reachability.audit_cancelling = false;
                        this.reachability.audit_notice = None;
                    }
                    Err(message) if message == "cancelled" => {
                        this.reachability.audit = AsyncJobState::Idle;
                        this.reachability.audit_progress = None;
                        this.reachability.audit_cancelling = false;
                        this.reachability.audit_notice = Some("Audit cancelled.".into());
                    }
                    Err(message) => {
                        this.reachability.audit = AsyncJobState::Failed(message.into());
                        this.reachability.audit_progress = Some(final_progress);
                        this.reachability.audit_cancelling = false;
                        this.reachability.audit_notice = None;
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn spawn_audit_progress_poller(
        &self,
        generation: u64,
        progress: SharedAuditProgress,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |view, cx| loop {
            Timer::after(AUDIT_PROGRESS_POLL_INTERVAL).await;
            let keep_running = view
                .update(cx, |this, cx| {
                    if this.reachability.audit.generation() != Some(generation) {
                        return false;
                    }
                    let snapshot = progress_snapshot(&progress);
                    if this.reachability.audit_progress.as_ref() != Some(&snapshot) {
                        this.reachability.audit_progress = Some(snapshot);
                        cx.notify();
                    }
                    this.reachability.audit.is_running()
                })
                .unwrap_or(false);
            if !keep_running {
                break;
            }
        })
        .detach();
    }
}

fn build_batch_reachability_result_blocking(
    requests: Vec<crate::ui::state::ActiveConfigTextRequest>,
    request: ReachabilityAuditRequest,
    cancel: JobCancelHandle,
    progress: SharedAuditProgress,
) -> Result<ReachabilityAuditViewModel, String> {
    let runtime = Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|err| format!("Failed to initialize batch reachability runtime: {err}"))?;
    runtime.block_on(async move {
        let result = build_batch_reachability_result(requests, request, cancel, progress).await?;
        Ok(ReachabilityAuditViewModel { request, result })
    })
}

async fn build_batch_reachability_result(
    requests: Vec<crate::ui::state::ActiveConfigTextRequest>,
    request: ReachabilityAuditRequest,
    cancel: JobCancelHandle,
    progress: SharedAuditProgress,
) -> Result<ReachabilityBatchResult, String> {
    let total_configs = requests.len();
    let mut rows = Vec::new();
    let mut jobs = Vec::new();

    for request in requests {
        if cancel.is_cancelled() {
            return Err("cancelled".to_string());
        }

        let request_label = request.display_name.clone();
        let resolved = match resolve_active_config_text_request(request).await {
            Ok(resolved) => resolved,
            Err(message) => {
                rows.push(ReachabilityBatchRow {
                    config_name: request_label,
                    peer_label: "-".into(),
                    target: "-".into(),
                    status: ReachabilityBatchStatus::ReadError,
                    summary: message.into(),
                });
                increment_processed_configs(&progress);
                continue;
            }
        };

        let config_name = resolved.display_name.to_string();
        let parsed = match config::parse_config(resolved.text.as_ref()) {
            Ok(parsed) => parsed,
            Err(err) => {
                rows.push(ReachabilityBatchRow {
                    config_name: config_name.clone().into(),
                    peer_label: "-".into(),
                    target: "-".into(),
                    status: ReachabilityBatchStatus::ParseError,
                    summary: err.to_string().into(),
                });
                increment_processed_configs(&progress);
                continue;
            }
        };

        let mut endpoint_count = 0usize;
        for (index, peer) in parsed.peers.iter().enumerate() {
            let Some(endpoint) = peer.endpoint.as_ref() else {
                continue;
            };
            endpoint_count += 1;
            jobs.push(BatchEndpointJob {
                config_name: config_name.clone(),
                peer_label: peer_label(index, peer),
                target: format_endpoint_display(endpoint),
            });
        }

        increment_processed_configs(&progress);
        if endpoint_count == 0 {
            rows.push(ReachabilityBatchRow {
                config_name: config_name.into(),
                peer_label: "-".into(),
                target: "-".into(),
                status: ReachabilityBatchStatus::NoEndpoint,
                summary: "No peer endpoint in this config.".into(),
            });
            continue;
        }

        increment_total_endpoints(&progress, endpoint_count);
    }

    update_progress(&progress, |state| {
        state.phase = ReachabilityAuditPhase::ProbingEndpoints;
        state.total_endpoints = jobs.len();
    });
    let endpoint_rows = jobs.len();

    let endpoint_results = stream::iter(jobs.into_iter().map(|job| {
        let cancel = cancel.clone();
        let progress = progress.clone();
        async move {
            if cancel.is_cancelled() {
                increment_completed_endpoints(&progress);
                return Err("cancelled".to_string());
            }

            let probe_request = r_wg::backend::wg::tools::ReachabilityRequest {
                target: job.target.clone(),
                mode: request.mode,
                port_override: None,
                family_preference: request.family_preference,
                timeout_ms: request.timeout_ms,
                max_addresses: 8,
                stop_on_first_success: request.stop_on_first_success,
            };
            let probe_result =
                run_reachability_probe_with_cancel_async(probe_request, cancel.clone()).await;

            let row = match probe_result {
                Ok(result) => ReachabilityBatchRow {
                    config_name: job.config_name.into(),
                    peer_label: job.peer_label.into(),
                    target: job.target.into(),
                    status: ReachabilityBatchStatus::from_verdict(result.verdict),
                    summary: result.summary.into(),
                },
                Err(message) if message == "cancelled" => {
                    increment_completed_endpoints(&progress);
                    return Err(message);
                }
                Err(err) => ReachabilityBatchRow {
                    config_name: job.config_name.into(),
                    peer_label: job.peer_label.into(),
                    target: job.target.into(),
                    status: ReachabilityBatchStatus::Failed,
                    summary: err.into(),
                },
            };
            increment_completed_endpoints(&progress);
            Ok::<_, String>(row)
        }
    }))
    .buffer_unordered(REACHABILITY_BATCH_CONCURRENCY)
    .try_collect::<Vec<_>>()
    .await?;

    update_progress(&progress, |state| {
        state.phase = ReachabilityAuditPhase::Finalizing;
    });

    rows.extend(endpoint_results);
    rows.sort_by(|left, right| {
        left.config_name
            .as_ref()
            .cmp(right.config_name.as_ref())
            .then(left.peer_label.as_ref().cmp(right.peer_label.as_ref()))
    });

    let resolved_rows = rows
        .iter()
        .filter(|row| row.status == ReachabilityBatchStatus::Resolved)
        .count();
    let reachable_rows = rows
        .iter()
        .filter(|row| row.status == ReachabilityBatchStatus::Reachable)
        .count();
    let partial_rows = rows
        .iter()
        .filter(|row| row.status == ReachabilityBatchStatus::PartiallyReachable)
        .count();
    let failed_rows = rows
        .iter()
        .filter(|row| row.status == ReachabilityBatchStatus::Failed)
        .count();
    let issue_rows = rows
        .iter()
        .filter(|row| {
            matches!(
                row.status,
                ReachabilityBatchStatus::ParseError
                    | ReachabilityBatchStatus::ReadError
                    | ReachabilityBatchStatus::NoEndpoint
            )
        })
        .count();

    update_progress(&progress, |state| {
        state.phase = ReachabilityAuditPhase::Completed;
        state.completed_endpoints = state.total_endpoints;
    });

    Ok(ReachabilityBatchResult {
        total_configs,
        endpoint_rows,
        resolved_rows,
        reachable_rows,
        partial_rows,
        failed_rows,
        issue_rows,
        rows: rows.into(),
    })
}

fn update_progress(
    progress: &SharedAuditProgress,
    update: impl FnOnce(&mut ReachabilityAuditProgress),
) {
    let mut guard = progress.lock().expect("audit progress lock poisoned");
    update(&mut guard);
}

fn increment_processed_configs(progress: &SharedAuditProgress) {
    update_progress(progress, |state| {
        state.processed_configs += 1;
    });
}

fn increment_total_endpoints(progress: &SharedAuditProgress, count: usize) {
    update_progress(progress, |state| {
        state.total_endpoints += count;
    });
}

fn increment_completed_endpoints(progress: &SharedAuditProgress) {
    update_progress(progress, |state| {
        state.completed_endpoints += 1;
    });
}

fn progress_snapshot(progress: &SharedAuditProgress) -> ReachabilityAuditProgress {
    progress
        .lock()
        .expect("audit progress lock poisoned")
        .clone()
}

fn peer_label(index: usize, peer: &PeerConfig) -> String {
    match peer.endpoint.as_ref() {
        Some(endpoint) => format!("Peer {} ({})", index + 1, endpoint.host),
        None => format!("Peer {}", index + 1),
    }
}
