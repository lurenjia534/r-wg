use gpui::{AppContext as _, Context, Window};
use r_wg::backend::wg::tools::{
    format_endpoint_display, probe_reachability_blocking_until_cancel,
    probe_reachability_until_cancel, AddressFamilyPreference, ReachabilityMode,
    ReachabilityRequest,
};

use crate::ui::state::{
    ActiveConfigParseState, AsyncJobState, JobCancelHandle, ReachabilityAuditFilter,
    ReachabilityAuditRequest, ReachabilitySingleViewModel, ReachabilityTab, ToolsTab,
    ToolsWorkspace,
};

const REACHABILITY_DEFAULT_TIMEOUT_MS: &str = "1500";

impl ToolsWorkspace {
    pub(crate) fn current_reachability_request_snapshot(
        &self,
        cx: &gpui::App,
    ) -> Result<ReachabilityRequest, String> {
        let target = self
            .reachability
            .target_input
            .as_ref()
            .map(|input| input.read(cx).value().to_string())
            .unwrap_or_default();
        let port_override = parse_optional_u16(
            self.reachability
                .port_input
                .as_ref()
                .map(|input| input.read(cx).value().to_string())
                .unwrap_or_default()
                .trim(),
        )?;
        let timeout_ms = parse_timeout_ms(
            self.reachability
                .single_timeout_input
                .as_ref()
                .map(|input| input.read(cx).value().to_string())
                .unwrap_or_else(|| REACHABILITY_DEFAULT_TIMEOUT_MS.to_string())
                .trim(),
        )?;

        Ok(ReachabilityRequest {
            target,
            mode: self.reachability.single_form.mode,
            port_override,
            family_preference: self.reachability.single_form.family_preference,
            timeout_ms,
            max_addresses: 8,
            stop_on_first_success: self.reachability.single_form.stop_on_first_success,
        })
    }

    pub(crate) fn current_reachability_audit_request(
        &self,
        cx: &gpui::App,
    ) -> Result<ReachabilityAuditRequest, String> {
        Ok(ReachabilityAuditRequest {
            mode: self.reachability.audit_form.mode,
            family_preference: self.reachability.audit_form.family_preference,
            stop_on_first_success: self.reachability.audit_form.stop_on_first_success,
            timeout_ms: parse_timeout_ms(
                self.reachability
                    .audit_timeout_input
                    .as_ref()
                    .map(|input| input.read(cx).value().to_string())
                    .unwrap_or_else(|| REACHABILITY_DEFAULT_TIMEOUT_MS.to_string())
                    .trim(),
            )?,
        })
    }

    pub(crate) fn set_single_reachability_mode(&mut self, value: ReachabilityMode) -> bool {
        if self.reachability.single_form.mode == value {
            return false;
        }
        self.reachability.single_form.mode = value;
        true
    }

    pub(crate) fn set_single_family_preference(&mut self, value: AddressFamilyPreference) -> bool {
        if self.reachability.single_form.family_preference == value {
            return false;
        }
        self.reachability.single_form.family_preference = value;
        true
    }

    pub(crate) fn set_single_stop_on_first_success(&mut self, value: bool) -> bool {
        if self.reachability.single_form.stop_on_first_success == value {
            return false;
        }
        self.reachability.single_form.stop_on_first_success = value;
        true
    }

    pub(crate) fn set_audit_reachability_mode(&mut self, value: ReachabilityMode) -> bool {
        if self.reachability.audit_form.mode == value {
            return false;
        }
        self.reachability.audit_form.mode = value;
        true
    }

    pub(crate) fn set_audit_family_preference(&mut self, value: AddressFamilyPreference) -> bool {
        if self.reachability.audit_form.family_preference == value {
            return false;
        }
        self.reachability.audit_form.family_preference = value;
        true
    }

    pub(crate) fn set_audit_stop_on_first_success(&mut self, value: bool) -> bool {
        if self.reachability.audit_form.stop_on_first_success == value {
            return false;
        }
        self.reachability.audit_form.stop_on_first_success = value;
        true
    }

    pub(crate) fn set_reachability_tab(&mut self, value: ReachabilityTab) -> bool {
        if self.reachability.active_tab == value {
            return false;
        }
        self.reachability.active_tab = value;
        true
    }

    pub(crate) fn set_reachability_audit_filter(&mut self, value: ReachabilityAuditFilter) -> bool {
        if self.reachability.audit_filter == value {
            return false;
        }
        self.reachability.audit_filter = value;
        true
    }

    pub(crate) fn run_reachability(&mut self, cx: &mut Context<Self>) {
        if self.reachability.single.is_running() || self.reachability.audit.is_running() {
            return;
        }

        let Some(request) = self.build_reachability_request(cx) else {
            return;
        };

        self.reachability.single_generation = self.reachability.single_generation.wrapping_add(1);
        let generation = self.reachability.single_generation;
        let cancel = self.reachability.single.set_running(generation);
        self.reachability.single_error = None;
        cx.notify();

        cx.spawn(async move |view, cx| {
            let task = cx.background_spawn(async move {
                if cancel.is_cancelled() {
                    return Err("cancelled".to_string());
                }
                let result = run_reachability_probe_with_cancel(request.clone(), cancel.clone())?;
                Ok::<_, String>(ReachabilitySingleViewModel { request, result })
            });
            let result = task.await;
            let _ = view.update(cx, |this, cx| {
                if this.reachability.single.generation() != Some(generation) {
                    return;
                }
                match result {
                    Ok(view_model) => this.reachability.single = AsyncJobState::Ready(view_model),
                    Err(message) if message == "cancelled" => {
                        this.reachability.single = AsyncJobState::Idle
                    }
                    Err(message) => {
                        this.reachability.single = AsyncJobState::Failed(message.into())
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn load_reachability_prefill_peer(
        &mut self,
        peer_index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(parsed) = self.active_config.parsed_config() else {
            self.reachability.single_error = Some(active_config_unavailable_message(self).into());
            cx.notify();
            return;
        };
        let endpoint = parsed
            .peers
            .get(peer_index)
            .and_then(|peer| peer.endpoint.as_ref())
            .cloned();

        let Some(endpoint) = endpoint else {
            self.reachability.single_error = Some("The selected peer has no endpoint.".into());
            cx.notify();
            return;
        };

        if let Some(input) = self.reachability.target_input.as_ref() {
            input.update(cx, |input, cx| {
                input.set_value(format_endpoint_display(&endpoint), window, cx);
            });
        }
        if let Some(input) = self.reachability.port_input.as_ref() {
            input.update(cx, |input, cx| {
                input.set_value("", window, cx);
            });
        }
        self.reachability.single_form.mode = ReachabilityMode::TcpConnect;
        self.reachability.single_error = None;
        self.active_tab = ToolsTab::Reachability;
        self.reachability.active_tab = ReachabilityTab::Single;
        cx.notify();
    }

    pub(crate) fn cancel_reachability_audit(&mut self, cx: &mut Context<Self>) {
        if !self.reachability.audit.is_running() || self.reachability.audit_cancelling {
            return;
        }
        self.reachability.audit.cancel();
        self.reachability.audit_cancelling = true;
        self.reachability.audit_notice = None;
        cx.notify();
    }

    fn build_reachability_request(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<ReachabilityRequest> {
        match self.current_reachability_request_snapshot(cx) {
            Ok(request) => Some(request),
            Err(message) => {
                self.reachability.single_error = Some(message.into());
                cx.notify();
                None
            }
        }
    }
}

pub(super) fn run_reachability_probe_with_cancel(
    request: ReachabilityRequest,
    cancel: JobCancelHandle,
) -> Result<r_wg::backend::wg::tools::ReachabilityResult, String> {
    probe_reachability_blocking_until_cancel(request, move || cancel.is_cancelled())
}

pub(super) async fn run_reachability_probe_with_cancel_async(
    request: ReachabilityRequest,
    cancel: JobCancelHandle,
) -> Result<r_wg::backend::wg::tools::ReachabilityResult, String> {
    probe_reachability_until_cancel(request, move || cancel.is_cancelled()).await
}

pub(super) fn parse_optional_u16(value: &str) -> Result<Option<u16>, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    let parsed = value
        .parse::<u16>()
        .map_err(|_| "Port override must be a non-zero integer.".to_string())?;
    if parsed == 0 {
        return Err("Port override must be a non-zero integer.".to_string());
    }
    Ok(Some(parsed))
}

pub(super) fn parse_timeout_ms(value: &str) -> Result<u64, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(REACHABILITY_DEFAULT_TIMEOUT_MS.parse().unwrap());
    }
    value
        .parse::<u64>()
        .map_err(|_| "Timeout must be an integer in milliseconds.".to_string())
        .and_then(|value| {
            if value == 0 {
                Err("Timeout must be greater than zero.".to_string())
            } else {
                Ok(value)
            }
        })
}

fn active_config_unavailable_message(workspace: &ToolsWorkspace) -> String {
    match &workspace.active_config.parse_state {
        ActiveConfigParseState::Loading => "Active config is still being parsed.".to_string(),
        ActiveConfigParseState::Invalid(message) => {
            format!("Active config is not usable: {message}")
        }
        _ => "No active config is available for prefill.".to_string(),
    }
}
