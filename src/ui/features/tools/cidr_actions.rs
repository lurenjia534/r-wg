use gpui::{AppContext as _, Context, SharedString};
use r_wg::backend::wg::tools::{
    compute_cidr_exclusion, normalize_cidr_set, parse_tool_prefixes, CidrExclusionResult,
    CidrNormalizationResult,
};

use super::state::{
    ActiveConfigParseState, AsyncJobState, CidrRequestSnapshot, CidrViewModel, ToolsTab,
    ToolsWorkspace,
};

const CIDR_RESULT_LIMIT: usize = 1024;

impl ToolsWorkspace {
    pub(crate) fn current_cidr_request(&self, cx: &gpui::App) -> CidrRequestSnapshot {
        CidrRequestSnapshot {
            include_text: self
                .cidr
                .include_input
                .as_ref()
                .map(|input| input.read(cx).value().to_string())
                .unwrap_or_default(),
            exclude_text: self
                .cidr
                .exclude_input
                .as_ref()
                .map(|input| input.read(cx).value().to_string())
                .unwrap_or_default(),
        }
    }

    pub(crate) fn compute_cidr(&mut self, cx: &mut Context<Self>) {
        if self.cidr.job.is_running() {
            return;
        }

        let request = self.current_cidr_request(cx);
        let includes = request.include_text.clone();
        let excludes = request.exclude_text.clone();

        self.cidr.generation = self.cidr.generation.wrapping_add(1);
        let generation = self.cidr.generation;
        let cancel = self.cidr.job.set_running(generation);
        cx.notify();

        cx.spawn(async move |view, cx| {
            let task = cx.background_spawn(async move {
                if cancel.is_cancelled() {
                    return Err("cancelled".to_string());
                }
                let includes = parse_tool_prefixes(&includes).map_err(|err| err.to_string())?;
                let excludes = parse_tool_prefixes(&excludes).map_err(|err| err.to_string())?;
                let result = compute_cidr_exclusion(&includes, &excludes, CIDR_RESULT_LIMIT)
                    .map_err(|err| err.to_string())?;
                if cancel.is_cancelled() {
                    return Err("cancelled".to_string());
                }
                Ok::<_, String>(CidrViewModel::from_result(result, request))
            });
            let result = task.await;
            let _ = view.update(cx, |this, cx| {
                if this.cidr.job.generation() != Some(generation) {
                    return;
                }
                match result {
                    Ok(view_model) => this.cidr.job = AsyncJobState::Ready(view_model),
                    Err(message) if message == "cancelled" => this.cidr.job = AsyncJobState::Idle,
                    Err(message) => this.cidr.job = AsyncJobState::Failed(message.into()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn load_cidr_prefill_peer(
        &mut self,
        peer_index: usize,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let Some(parsed) = self.active_config.parsed_config() else {
            self.cidr.job = AsyncJobState::Failed(active_config_unavailable_message(self).into());
            cx.notify();
            return;
        };

        let prefixes = parsed
            .peers
            .get(peer_index)
            .map(|peer| peer.allowed_ips.clone())
            .unwrap_or_default();
        self.load_cidr_prefill_prefixes(prefixes, window, cx);
    }

    pub(crate) fn load_cidr_prefill_union(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let Some(parsed) = self.active_config.parsed_config() else {
            self.cidr.job = AsyncJobState::Failed(active_config_unavailable_message(self).into());
            cx.notify();
            return;
        };

        let prefixes = parsed
            .peers
            .iter()
            .flat_map(|peer| peer.allowed_ips.clone())
            .collect::<Vec<_>>();
        self.load_cidr_prefill_prefixes(prefixes, window, cx);
    }

    fn load_cidr_prefill_prefixes(
        &mut self,
        prefixes: Vec<r_wg::core::config::AllowedIp>,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        match CidrViewModel::normalize_only(prefixes) {
            Ok(view_model) => {
                if let Some(input) = self.cidr.include_input.as_ref() {
                    input.update(cx, |input, cx| {
                        input.set_value(view_model.remaining_text.clone(), window, cx);
                    });
                }
                if let Some(input) = self.cidr.exclude_input.as_ref() {
                    input.update(cx, |input, cx| {
                        input.set_value("", window, cx);
                    });
                }
                let request = self.current_cidr_request(cx);
                self.cidr.job = AsyncJobState::Ready(view_model.with_request(request));
                self.active_tab = ToolsTab::Cidr;
                let _ = self.app.update(cx, |app, cx| {
                    app.push_success_toast(
                        "Loaded AllowedIPs into Include; Exclude cleared",
                        window,
                        cx,
                    );
                });
                cx.notify();
            }
            Err(message) => {
                self.cidr.job = AsyncJobState::Failed(message.into());
                cx.notify();
            }
        }
    }
}

impl CidrViewModel {
    pub(crate) fn from_result(result: CidrExclusionResult, request: CidrRequestSnapshot) -> Self {
        let normalized_include_text = format_prefix_list(&result.normalized_includes).into();
        let normalized_exclude_text = format_prefix_list(&result.normalized_excludes).into();
        let remaining_text = format_prefix_list(&result.remaining).into();
        let allowed_ips_assignment = format_allowed_ips_assignment(&result.remaining).into();
        let summary_rows = cidr_summary_rows(&result)
            .into_iter()
            .map(|(label, value)| (label.into(), value.into()))
            .collect();

        Self {
            request,
            normalized_include_text,
            normalized_exclude_text,
            remaining_text,
            allowed_ips_assignment,
            summary_rows,
        }
    }

    pub(crate) fn normalize_only(
        prefixes: Vec<r_wg::core::config::AllowedIp>,
    ) -> Result<Self, String> {
        let result =
            normalize_cidr_set(&prefixes, CIDR_RESULT_LIMIT).map_err(|err| err.to_string())?;
        Ok(Self::from_normalized_result(result))
    }

    fn from_normalized_result(result: CidrNormalizationResult) -> Self {
        let normalized_text: SharedString = format_prefix_list(&result.normalized).into();
        let allowed_ips_assignment: SharedString =
            format_allowed_ips_assignment(&result.normalized).into();
        let summary_rows = vec![
            (
                "Input includes".into(),
                result.stats.input_count.to_string().into(),
            ),
            ("Input excludes".into(), "0".into()),
            (
                "Normalized includes".into(),
                result.stats.normalized_count.to_string().into(),
            ),
            ("Normalized excludes".into(), "0".into()),
            (
                "Remaining CIDRs".into(),
                result.stats.normalized_count.to_string().into(),
            ),
            (
                "Host bits normalized".into(),
                yes_no(result.stats.host_bits_normalized).into(),
            ),
            (
                "Merged prefixes".into(),
                yes_no(result.stats.merged_prefixes).into(),
            ),
        ];

        Self {
            request: CidrRequestSnapshot {
                include_text: String::new(),
                exclude_text: String::new(),
            },
            normalized_include_text: normalized_text.clone(),
            normalized_exclude_text: SharedString::from(""),
            remaining_text: normalized_text,
            allowed_ips_assignment,
            summary_rows,
        }
    }

    pub(crate) fn with_request(mut self, request: CidrRequestSnapshot) -> Self {
        self.request = request;
        self
    }

    pub(crate) fn has_remaining_prefixes(&self) -> bool {
        !self.remaining_text.as_ref().trim().is_empty()
    }
}

fn format_prefix_list(prefixes: &[r_wg::core::config::AllowedIp]) -> String {
    prefixes
        .iter()
        .map(|prefix| format!("{}/{}", prefix.addr, prefix.cidr))
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_allowed_ips_assignment(prefixes: &[r_wg::core::config::AllowedIp]) -> String {
    format!(
        "AllowedIPs = {}",
        prefixes
            .iter()
            .map(|prefix| format!("{}/{}", prefix.addr, prefix.cidr))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn cidr_summary_rows(result: &CidrExclusionResult) -> Vec<(String, String)> {
    vec![
        (
            "Input includes".to_string(),
            result.stats.input_include_count.to_string(),
        ),
        (
            "Input excludes".to_string(),
            result.stats.input_exclude_count.to_string(),
        ),
        (
            "Normalized includes".to_string(),
            result.stats.normalized_include_count.to_string(),
        ),
        (
            "Normalized excludes".to_string(),
            result.stats.normalized_exclude_count.to_string(),
        ),
        (
            "Remaining CIDRs".to_string(),
            result.stats.output_count.to_string(),
        ),
        (
            "Host bits normalized".to_string(),
            yes_no(result.stats.host_bits_normalized),
        ),
        (
            "Merged prefixes".to_string(),
            yes_no(result.stats.merged_prefixes),
        ),
    ]
}

fn yes_no(value: bool) -> String {
    if value {
        "Yes".to_string()
    } else {
        "No".to_string()
    }
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
