use std::sync::Arc;

use gpui::{App, AppContext as _, Context, Entity, Window};
use gpui_component::input::{InputEvent, InputState};
use r_wg::application::ConfigLibraryService;
use r_wg::core::config;

use crate::ui::state::{ConfigDraftState, DraftValidationState, WgApp};

use super::state::{
    ActiveConfigIdentity, ActiveConfigParseState, ActiveConfigSnapshot, ActiveConfigSource,
    ActiveConfigTextRequest, JobCancelHandle, ResolvedActiveConfigText, ToolsWorkspace,
};

struct ActiveConfigBridge {
    snapshot: ActiveConfigSnapshot,
    request: Option<ActiveConfigTextRequest>,
}

impl WgApp {
    pub(crate) fn ensure_tools_workspace(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ToolsWorkspace> {
        let workspace = if let Some(workspace) = self.ui.tools_workspace.clone() {
            workspace
        } else {
            let app = cx.entity();
            let workspace = cx.new(|_| ToolsWorkspace::new(app));
            self.ui.tools_workspace = Some(workspace.clone());
            workspace
        };

        workspace.update(cx, |workspace, cx| {
            workspace.ensure_inputs(window, cx);
        });
        workspace
    }

    pub(crate) fn sync_tools_active_config_snapshot(&mut self, cx: &mut Context<Self>) {
        self.update_tools_active_config(false, cx);
    }

    pub(crate) fn refresh_tools_active_config_for_display(&mut self, cx: &mut Context<Self>) {
        self.update_tools_active_config(true, cx);
    }

    fn update_tools_active_config(&mut self, allow_parse: bool, cx: &mut Context<Self>) {
        let Some(workspace) = self.ui.tools_workspace.clone() else {
            return;
        };
        let bridge = self.build_active_config_bridge(cx);
        workspace.update(cx, |workspace, cx| {
            workspace.sync_active_config_bridge(bridge, allow_parse, cx);
        });
    }

    fn build_active_config_bridge(&self, cx: &App) -> ActiveConfigBridge {
        if let Some(workspace) = self.ui.configs_workspace.as_ref() {
            let workspace = workspace.read(cx);
            if let Some(bridge) = self.build_draft_active_config_bridge(&workspace.draft) {
                return bridge;
            }
        }

        self.build_selected_active_config_bridge()
    }

    fn build_draft_active_config_bridge(
        &self,
        draft: &ConfigDraftState,
    ) -> Option<ActiveConfigBridge> {
        let text = draft.text.clone();
        if text.as_ref().trim().is_empty() {
            return None;
        }

        let display_name = if draft.name.as_ref().trim().is_empty() {
            self.selected_config()
                .map(|selected| selected.name.clone().into())
                .unwrap_or_else(|| "Current Draft".into())
        } else {
            draft.name.clone()
        };

        let identity = ActiveConfigIdentity {
            source: ActiveConfigSource::Draft,
            config_id: draft.source_id,
            text_revision: draft.revision,
        };

        let (parse_state, request) = match &draft.validation {
            DraftValidationState::Valid { parsed, .. } => (
                ActiveConfigParseState::Ready(Arc::new(parsed.clone())),
                None,
            ),
            DraftValidationState::Invalid { line, message, .. } => {
                let detail = match line {
                    Some(line) => format!("line {line}: {message}"),
                    None => message.to_string(),
                };
                (ActiveConfigParseState::Invalid(detail.into()), None)
            }
            DraftValidationState::Idle => (
                ActiveConfigParseState::Loading,
                Some(ActiveConfigTextRequest {
                    identity,
                    display_name: display_name.clone(),
                    inline_text: Some(text),
                    storage_path: draft.source_id.and_then(|id| {
                        self.configs
                            .get_by_id(id)
                            .map(|config| config.storage_path.clone())
                    }),
                    source: ActiveConfigSource::Draft,
                }),
            ),
        };

        Some(ActiveConfigBridge {
            snapshot: ActiveConfigSnapshot {
                source: ActiveConfigSource::Draft,
                source_label: display_name,
                identity: Some(identity),
                parse_state,
                ..ActiveConfigSnapshot::default()
            },
            request,
        })
    }

    fn build_selected_active_config_bridge(&self) -> ActiveConfigBridge {
        let Some(selected) = self.selected_config() else {
            return ActiveConfigBridge {
                snapshot: ActiveConfigSnapshot {
                    source: ActiveConfigSource::None,
                    source_label: "No active config".into(),
                    ..ActiveConfigSnapshot::default()
                },
                request: None,
            };
        };

        let identity = ActiveConfigIdentity {
            source: ActiveConfigSource::SavedSelection,
            config_id: Some(selected.id),
            text_revision: self.selection.selection_revision,
        };
        let inline_text = selected
            .text
            .clone()
            .or_else(|| self.peek_cached_config_text(&selected.storage_path));

        let request = Some(ActiveConfigTextRequest {
            identity,
            display_name: selected.name.clone().into(),
            inline_text,
            storage_path: Some(selected.storage_path.clone()),
            source: ActiveConfigSource::SavedSelection,
        });

        ActiveConfigBridge {
            snapshot: ActiveConfigSnapshot {
                source: ActiveConfigSource::SavedSelection,
                source_label: selected.name.clone().into(),
                identity: Some(identity),
                parse_state: ActiveConfigParseState::Loading,
                ..ActiveConfigSnapshot::default()
            },
            request,
        }
    }

    pub(crate) fn saved_config_text_requests(&self) -> Vec<ActiveConfigTextRequest> {
        self.configs
            .iter()
            .map(|config| ActiveConfigTextRequest {
                identity: ActiveConfigIdentity {
                    source: ActiveConfigSource::SavedSelection,
                    config_id: Some(config.id),
                    text_revision: self.selection.selection_revision,
                },
                display_name: config.name.clone().into(),
                inline_text: config
                    .text
                    .clone()
                    .or_else(|| self.peek_cached_config_text(&config.storage_path)),
                storage_path: Some(config.storage_path.clone()),
                source: ActiveConfigSource::SavedSelection,
            })
            .collect()
    }
}

impl ToolsWorkspace {
    pub(crate) fn ensure_inputs(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) {
        if self.cidr.include_input.is_none() {
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .multi_line(true)
                    .soft_wrap(true)
                    .searchable(false)
                    .rows(8)
                    // Keep placeholders single-line; multiline placeholders can panic on Windows
                    // in gpui-component 0.5.1 during text shaping.
                    .placeholder("One CIDR per line, e.g. 10.0.0.0/8")
            });
            let subscription = cx.subscribe(
                &input,
                |this, _, event: &InputEvent, cx: &mut gpui::Context<Self>| {
                    if matches!(event, InputEvent::Change)
                        && matches!(this.cidr.job, super::state::AsyncJobState::Failed(_))
                    {
                        this.cidr.job = super::state::AsyncJobState::Idle;
                        cx.notify();
                    }
                },
            );
            self.cidr.include_input = Some(input);
            self.cidr.include_subscription = Some(subscription);
        }

        if self.cidr.exclude_input.is_none() {
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .multi_line(true)
                    .soft_wrap(true)
                    .searchable(false)
                    .rows(8)
                    .placeholder("One CIDR per line, e.g. 10.0.0.0/24")
            });
            let subscription = cx.subscribe(
                &input,
                |this, _, event: &InputEvent, cx: &mut gpui::Context<Self>| {
                    if matches!(event, InputEvent::Change)
                        && matches!(this.cidr.job, super::state::AsyncJobState::Failed(_))
                    {
                        this.cidr.job = super::state::AsyncJobState::Idle;
                        cx.notify();
                    }
                },
            );
            self.cidr.exclude_input = Some(input);
            self.cidr.exclude_subscription = Some(subscription);
        }

        if self.reachability.target_input.is_none() {
            let input = cx.new(|cx| {
                InputState::new(window, cx).placeholder("example.com or 203.0.113.10:443")
            });
            let subscription = cx.subscribe(
                &input,
                |this, _, event: &InputEvent, cx: &mut gpui::Context<Self>| {
                    if matches!(event, InputEvent::Change)
                        && this.reachability.single_error.take().is_some()
                    {
                        cx.notify();
                    }
                },
            );
            self.reachability.target_input = Some(input);
            self.reachability.target_subscription = Some(subscription);
        }

        if self.reachability.port_input.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("51820"));
            let subscription = cx.subscribe(
                &input,
                |this, _, event: &InputEvent, cx: &mut gpui::Context<Self>| {
                    if matches!(event, InputEvent::Change)
                        && this.reachability.single_error.take().is_some()
                    {
                        cx.notify();
                    }
                },
            );
            self.reachability.port_input = Some(input);
            self.reachability.port_subscription = Some(subscription);
        }

        if self.reachability.single_timeout_input.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("1500"));
            input.update(cx, |input, cx| {
                input.set_value("1500", window, cx);
            });
            let subscription = cx.subscribe(
                &input,
                |this, _, event: &InputEvent, cx: &mut gpui::Context<Self>| {
                    if matches!(event, InputEvent::Change)
                        && this.reachability.single_error.take().is_some()
                    {
                        cx.notify();
                    }
                },
            );
            self.reachability.single_timeout_input = Some(input);
            self.reachability.single_timeout_subscription = Some(subscription);
        }

        if self.reachability.audit_timeout_input.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("1500"));
            input.update(cx, |input, cx| {
                input.set_value("1500", window, cx);
            });
            let subscription = cx.subscribe(
                &input,
                |this, _, event: &InputEvent, cx: &mut gpui::Context<Self>| {
                    if matches!(event, InputEvent::Change)
                        && this.reachability.audit_error.take().is_some()
                    {
                        cx.notify();
                    }
                },
            );
            self.reachability.audit_timeout_input = Some(input);
            self.reachability.audit_timeout_subscription = Some(subscription);
        }
    }

    fn sync_active_config_bridge(
        &mut self,
        bridge: ActiveConfigBridge,
        allow_parse: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        let same_identity = self.active_config.identity == bridge.snapshot.identity;
        let can_keep_parse_state = same_identity
            && matches!(
                self.active_config.parse_state,
                ActiveConfigParseState::Ready(_) | ActiveConfigParseState::Invalid(_)
            )
            && matches!(bridge.snapshot.parse_state, ActiveConfigParseState::Loading);
        let should_spawn_parse = allow_parse && bridge.request.is_some() && !can_keep_parse_state;
        let parse_state = if bridge.request.is_none() {
            bridge.snapshot.parse_state.clone()
        } else if can_keep_parse_state {
            self.active_config.parse_state.clone()
        } else if should_spawn_parse {
            ActiveConfigParseState::Loading
        } else {
            ActiveConfigParseState::None
        };
        let refresh_pending =
            bridge.request.is_some() && !can_keep_parse_state && !should_spawn_parse;

        if self.active_config.identity == bridge.snapshot.identity
            && self.active_config.source == bridge.snapshot.source
            && self.active_config.source_label == bridge.snapshot.source_label
            && active_config_parse_state_matches(&self.active_config.parse_state, &parse_state)
            && self.active_config_refresh_pending == refresh_pending
        {
            return;
        }

        if !should_spawn_parse {
            let parse_state = if can_keep_parse_state {
                self.active_config.parse_state.clone()
            } else {
                parse_state
            };
            self.active_config_cancel
                .take()
                .map(|cancel| cancel.cancel());
            self.active_config_refresh_pending = refresh_pending;
            self.active_config = ActiveConfigSnapshot {
                revision: self.active_config.revision.wrapping_add(1),
                identity: bridge.snapshot.identity,
                source: bridge.snapshot.source,
                source_label: bridge.snapshot.source_label,
                parse_state,
            };
            cx.notify();
            return;
        }

        let request = bridge.request.expect("request should exist");
        self.active_config_generation = self.active_config_generation.wrapping_add(1);
        let generation = self.active_config_generation;
        let cancel = JobCancelHandle::new();
        self.active_config_cancel
            .take()
            .map(|existing| existing.cancel());
        self.active_config_cancel = Some(cancel.clone());
        self.active_config_refresh_pending = false;
        let source_label = bridge.snapshot.source_label.clone();
        self.active_config = ActiveConfigSnapshot {
            revision: self.active_config.revision.wrapping_add(1),
            identity: bridge.snapshot.identity,
            source: bridge.snapshot.source,
            source_label,
            parse_state: ActiveConfigParseState::Loading,
        };
        cx.notify();

        cx.spawn(async move |view, cx| {
            let task = cx.background_spawn(async move {
                if cancel.is_cancelled() {
                    return Err("cancelled".to_string());
                }
                let resolved = resolve_active_config_text_request(request).await?;
                if cancel.is_cancelled() {
                    return Err("cancelled".to_string());
                }
                let parsed =
                    config::parse_config(resolved.text.as_ref()).map_err(|err| err.to_string())?;
                Ok::<_, String>((resolved, parsed))
            });
            let result = task.await;
            let _ = view.update(cx, |this, cx| {
                if this.active_config_generation != generation {
                    return;
                }
                match result {
                    Ok((resolved, parsed)) => {
                        this.active_config = ActiveConfigSnapshot {
                            revision: this.active_config.revision.wrapping_add(1),
                            identity: Some(resolved.identity),
                            source: resolved.source,
                            source_label: resolved.display_name,
                            parse_state: ActiveConfigParseState::Ready(Arc::new(parsed)),
                        };
                        this.active_config_cancel = None;
                        this.active_config_refresh_pending = false;
                    }
                    Err(message) if message == "cancelled" => {}
                    Err(message) => {
                        this.active_config = ActiveConfigSnapshot {
                            revision: this.active_config.revision.wrapping_add(1),
                            identity: bridge.snapshot.identity,
                            source: bridge.snapshot.source,
                            source_label: bridge.snapshot.source_label.clone(),
                            parse_state: ActiveConfigParseState::Invalid(message.into()),
                        };
                        this.active_config_cancel = None;
                        this.active_config_refresh_pending = false;
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

fn active_config_parse_state_matches(
    left: &ActiveConfigParseState,
    right: &ActiveConfigParseState,
) -> bool {
    match (left, right) {
        (ActiveConfigParseState::None, ActiveConfigParseState::None)
        | (ActiveConfigParseState::Loading, ActiveConfigParseState::Loading)
        | (ActiveConfigParseState::Ready(_), ActiveConfigParseState::Ready(_)) => true,
        (ActiveConfigParseState::Invalid(left), ActiveConfigParseState::Invalid(right)) => {
            left == right
        }
        _ => false,
    }
}

pub(crate) async fn resolve_active_config_text_request(
    request: ActiveConfigTextRequest,
) -> Result<ResolvedActiveConfigText, String> {
    if let Some(text) = request.inline_text.clone() {
        return Ok(ResolvedActiveConfigText {
            identity: request.identity,
            display_name: request.display_name,
            text,
            source: request.source,
        });
    }

    let Some(path) = request.storage_path.clone() else {
        return Err("No active config text is available.".to_string());
    };
    let text = ConfigLibraryService::new().read_config_text(&path)?;

    Ok(ResolvedActiveConfigText {
        identity: request.identity,
        display_name: request.display_name,
        text: text.into(),
        source: request.source,
    })
}
