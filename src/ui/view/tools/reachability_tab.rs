use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonGroup, ButtonVariants},
    group_box::{GroupBox, GroupBoxVariants},
    h_flex,
    input::Input,
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
    scroll::ScrollableElement as _,
    tab::{Tab, TabBar},
    tag::Tag,
    v_flex, ActiveTheme as _, Disableable as _, Selectable, Sizable as _, StyledExt as _,
};

use r_wg::backend::wg::tools::{
    AddressFamilyPreference, ReachabilityAttemptResult, ReachabilityMode, ReachabilityVerdict,
};

use crate::ui::state::{
    AsyncJobState, ReachabilityAuditFilter, ReachabilityAuditPhase, ReachabilityAuditProgress,
    ReachabilityBatchStatus, ReachabilitySingleViewModel, ReachabilityTab, ReachabilityToolState,
    ToolsWorkspace,
};

use super::components::{empty_result_state, error_banner};

pub(super) fn render_reachability_tab(
    workspace: &ToolsWorkspace,
    stack: bool,
    window: &mut Window,
    cx: &mut Context<ToolsWorkspace>,
) -> Div {
    let target_input = workspace
        .reachability
        .target_input
        .clone()
        .expect("reach target input should exist");
    let port_input = workspace
        .reachability
        .port_input
        .clone()
        .expect("reach port input should exist");
    let timeout_input = workspace
        .reachability
        .timeout_input
        .clone()
        .expect("reach timeout input should exist");

    let workspace_handle = cx.entity();
    let sub_tabs = TabBar::new("tools-reach-subtabs")
        .underline()
        .small()
        .selected_index(match workspace.reachability.active_tab {
            ReachabilityTab::Single => 0,
            ReachabilityTab::Audit => 1,
        })
        .on_click(move |index, _window, app| {
            let next = match *index {
                0 => ReachabilityTab::Single,
                1 => ReachabilityTab::Audit,
                _ => return,
            };
            app.update_entity(&workspace_handle, |this, cx| {
                if this.set_reachability_tab(next) {
                    cx.notify();
                }
            });
        })
        .child(Tab::new().label(ReachabilityTab::Single.label()).small())
        .child(Tab::new().label(ReachabilityTab::Audit.label()).small());

    let form = v_flex()
        .gap_3()
        .child(sub_tabs)
        .child(render_single_input_box(
            "Target",
            "Hostname, IP literal, or host:port.",
            &target_input,
            render_reachability_prefill_action(workspace, window, cx),
            cx,
        ))
        .child(render_single_input_box(
            "Port Override",
            "Optional when the target already includes :port.",
            &port_input,
            None,
            cx,
        ))
        .child(render_single_input_box(
            "Timeout (ms)",
            "Per resolve or connect attempt timeout.",
            &timeout_input,
            None,
            cx,
        ))
        .child(render_reachability_toggles(&workspace.reachability, cx))
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("Resolve checks the current host resolver. TCP Connect checks host-side TCP reachability only. It does not prove WireGuard UDP liveness."),
        )
        .child(match workspace.reachability.active_tab {
            ReachabilityTab::Single => h_flex()
                .justify_end()
                .child(
                    Button::new("tools-reach-run")
                        .label(if workspace.reachability.single.is_running() {
                            "Testing..."
                        } else {
                            "Test"
                        })
                        .primary()
                        .small()
                        .disabled(
                            workspace.reachability.single.is_running()
                                || workspace.reachability.audit.is_running(),
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.run_reachability(cx);
                        })),
                )
                .into_any_element(),
            ReachabilityTab::Audit => h_flex()
                .justify_between()
                .gap_2()
                .flex_wrap()
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("Audit runs against every saved config and checks each peer endpoint it can parse."),
                )
                .child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(
                            Button::new("tools-reach-audit-run")
                                .label(if workspace.reachability.audit.is_running() {
                                    "Auditing..."
                                } else {
                                    "Run Audit"
                                })
                                .primary()
                                .small()
                                .disabled(
                                    workspace.reachability.single.is_running()
                                        || workspace.reachability.audit.is_running(),
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.run_reachability_audit(cx);
                                })),
                        )
                        .child(
                            Button::new("tools-reach-audit-cancel")
                                .label("Cancel")
                                .outline()
                                .small()
                                .disabled(!workspace.reachability.audit.is_running())
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.cancel_reachability_audit(cx);
                                })),
                        ),
                )
                .into_any_element(),
        });

    let result = render_reachability_result_panel(workspace, cx);

    if stack {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .flex_1()
            .min_h(px(0.0))
            .child(form)
            .child(result)
    } else {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .flex_1()
            .min_h(px(0.0))
            .child(form)
            .child(result)
    }
}

fn render_single_input_box(
    title: &str,
    description: &str,
    input: &Entity<gpui_component::input::InputState>,
    action: Option<AnyElement>,
    cx: &mut Context<ToolsWorkspace>,
) -> GroupBox {
    GroupBox::new().fill().title(title.to_string()).child(
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .flex_wrap()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(description.to_string()),
                    )
                    .when_some(action, |this, action| this.child(action)),
            )
            .child(
                div()
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border.alpha(0.7))
                    .bg(cx.theme().group_box)
                    .px_2()
                    .py_1()
                    .child(Input::new(input).appearance(false).bordered(false)),
            ),
    )
}

fn render_reachability_toggles(
    state: &ReachabilityToolState,
    cx: &mut Context<ToolsWorkspace>,
) -> GroupBox {
    let mode_group = ButtonGroup::new("tools-reach-mode")
        .outline()
        .compact()
        .small()
        .child(
            Button::new("tools-reach-mode-resolve")
                .label("Resolve")
                .selected(state.form.mode == ReachabilityMode::ResolveOnly)
                .on_click(cx.listener(|this, _, _, cx| {
                    if this.set_reachability_mode(ReachabilityMode::ResolveOnly) {
                        cx.notify();
                    }
                })),
        )
        .child(
            Button::new("tools-reach-mode-tcp")
                .label("TCP Connect")
                .selected(state.form.mode == ReachabilityMode::TcpConnect)
                .on_click(cx.listener(|this, _, _, cx| {
                    if this.set_reachability_mode(ReachabilityMode::TcpConnect) {
                        cx.notify();
                    }
                })),
        );

    let family_group = ButtonGroup::new("tools-reach-family")
        .outline()
        .compact()
        .small()
        .child(family_button(AddressFamilyPreference::System, state, cx))
        .child(family_button(
            AddressFamilyPreference::PreferIpv4,
            state,
            cx,
        ))
        .child(family_button(
            AddressFamilyPreference::PreferIpv6,
            state,
            cx,
        ));

    let stop_group = ButtonGroup::new("tools-reach-stop")
        .outline()
        .compact()
        .small()
        .child(
            Button::new("tools-reach-stop-first")
                .label("Stop on First Success")
                .selected(state.form.stop_on_first_success)
                .on_click(cx.listener(|this, _, _, cx| {
                    if this.set_stop_on_first_success(true) {
                        cx.notify();
                    }
                })),
        )
        .child(
            Button::new("tools-reach-stop-all")
                .label("Try All Addresses")
                .selected(!state.form.stop_on_first_success)
                .on_click(cx.listener(|this, _, _, cx| {
                    if this.set_stop_on_first_success(false) {
                        cx.notify();
                    }
                })),
        );

    GroupBox::new().fill().title("Mode".to_string()).child(
        v_flex()
            .gap_3()
            .child(mode_group)
            .child(
                v_flex()
                    .gap_1()
                    .child(div().text_xs().font_semibold().child("Address Family"))
                    .child(family_group),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(div().text_xs().font_semibold().child("Attempt Strategy"))
                    .child(stop_group),
            ),
    )
}

fn render_reachability_prefill_action(
    workspace: &ToolsWorkspace,
    _window: &mut Window,
    cx: &mut Context<ToolsWorkspace>,
) -> Option<AnyElement> {
    let Some(parsed) = workspace.active_config.parsed_config() else {
        return Some(
            Button::new("tools-reach-prefill-disabled")
                .label(if workspace.active_config.is_loading() {
                    "Parsing active config"
                } else {
                    "Use current endpoint"
                })
                .outline()
                .xsmall()
                .disabled(true)
                .into_any_element(),
        );
    };

    let options = parsed
        .peers
        .iter()
        .enumerate()
        .filter(|(_, peer)| peer.endpoint.is_some())
        .map(|(index, _)| (format!("Peer {} endpoint", index + 1), index))
        .collect::<Vec<_>>();
    if options.is_empty() {
        return None;
    }
    if options.len() == 1 {
        let peer_index = options[0].1;
        return Some(
            Button::new("tools-reach-prefill-single")
                .label("Use current endpoint")
                .outline()
                .xsmall()
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.load_reachability_prefill_peer(peer_index, window, cx);
                }))
                .into_any_element(),
        );
    }

    Some(
        Button::new("tools-reach-prefill-menu")
            .label("Use current endpoint")
            .outline()
            .xsmall()
            .dropdown_caret(true)
            .dropdown_menu_with_anchor(Corner::TopRight, {
                let workspace = cx.entity();
                move |menu: PopupMenu, _, _| {
                    let mut menu = menu;
                    for (label, peer_index) in options.clone() {
                        let workspace = workspace.clone();
                        menu =
                            menu.item(PopupMenuItem::new(label).on_click(move |_, window, cx| {
                                workspace.update(cx, |this, cx| {
                                    this.load_reachability_prefill_peer(peer_index, window, cx);
                                });
                            }));
                    }
                    menu
                }
            })
            .into_any_element(),
    )
}

fn render_reachability_result_panel(
    workspace: &ToolsWorkspace,
    cx: &mut Context<ToolsWorkspace>,
) -> GroupBox {
    GroupBox::new().fill().title("Result".to_string()).child(
        v_flex()
            .gap_3()
            .when_some(
                workspace.reachability.form_error.clone(),
                |this, message| this.child(error_banner(message, cx)),
            )
            .child(match workspace.reachability.active_tab {
                ReachabilityTab::Single => render_single_result(workspace, cx).into_any_element(),
                ReachabilityTab::Audit => render_audit_result(workspace, cx).into_any_element(),
            }),
    )
}

fn render_single_result(workspace: &ToolsWorkspace, cx: &mut Context<ToolsWorkspace>) -> Div {
    match &workspace.reachability.single {
        AsyncJobState::Idle => empty_result_state(
            "Run a reachability test to inspect resolved addresses and per-address attempts.",
            cx,
        ),
        AsyncJobState::Running { .. } => empty_result_state("Reachability test is running…", cx),
        AsyncJobState::Failed(message) => error_banner(message.clone(), cx),
        AsyncJobState::Ready(view_model) => render_reachability_result(view_model, cx),
    }
}

fn render_reachability_result(
    view_model: &ReachabilitySingleViewModel,
    cx: &mut Context<ToolsWorkspace>,
) -> Div {
    let result = &view_model.result;
    let verdict = match result.verdict {
        ReachabilityVerdict::Resolved => Tag::info().small().rounded_full().child("Resolved"),
        ReachabilityVerdict::Reachable => Tag::success().small().rounded_full().child("Reachable"),
        ReachabilityVerdict::PartiallyReachable => Tag::warning()
            .small()
            .rounded_full()
            .child("Partially Reachable"),
        ReachabilityVerdict::Failed => Tag::danger().small().rounded_full().child("Failed"),
    };

    v_flex()
        .gap_3()
        .child(
            h_flex()
                .items_center()
                .gap_2()
                .flex_wrap()
                .child(verdict)
                .child(
                    Tag::secondary()
                        .small()
                        .rounded_full()
                        .child(result.normalized_target.clone()),
                ),
        )
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(result.summary.clone()),
        )
        .child(render_socket_list(
            "Resolved Addresses",
            result
                .resolved
                .iter()
                .map(|addr| addr.to_string())
                .collect(),
            cx,
        ))
        .when(!result.attempts.is_empty(), |this| {
            this.child(
                GroupBox::new().fill().title("Attempts".to_string()).child(
                    div()
                        .max_h(px(280.0))
                        .overflow_y_scrollbar()
                        .child(
                            v_flex()
                                .gap_2()
                                .children(result.attempts.iter().map(|attempt| {
                                    div()
                                        .rounded_lg()
                                        .border_1()
                                        .border_color(cx.theme().border.alpha(0.6))
                                        .bg(cx.theme().group_box)
                                        .px_3()
                                        .py_2()
                                        .child(
                                            v_flex()
                                                .gap_1()
                                                .child(
                                                    h_flex()
                                                        .items_center()
                                                        .justify_between()
                                                        .gap_3()
                                                        .flex_wrap()
                                                        .child(
                                                            div()
                                                                .text_sm()
                                                                .font_semibold()
                                                                .child(attempt.address.to_string()),
                                                        )
                                                        .child(attempt_result_tag(attempt.result)),
                                                )
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(cx.theme().muted_foreground)
                                                        .child(format!(
                                                            "{} | {} ms | {}",
                                                            if attempt.address.is_ipv4() {
                                                                "IPv4"
                                                            } else {
                                                                "IPv6"
                                                            },
                                                            attempt.elapsed_ms,
                                                            attempt.message
                                                        )),
                                                ),
                                        )
                                })),
                        ),
                ),
            )
        })
}

fn render_audit_result(workspace: &ToolsWorkspace, cx: &mut Context<ToolsWorkspace>) -> Div {
    match &workspace.reachability.audit {
        AsyncJobState::Idle => empty_result_state(
            "Run an audit to scan all saved configs and summarize endpoint reachability.",
            cx,
        ),
        AsyncJobState::Running { .. } => render_audit_progress_state(workspace, cx),
        AsyncJobState::Failed(message) => error_banner(message.clone(), cx),
        AsyncJobState::Ready(view_model) => {
            let result = &view_model.result;
            let filtered_rows = result
                .rows
                .iter()
                .filter(|row| workspace.reachability.audit_filter.matches(row.status))
                .collect::<Vec<_>>();
            div().child(
                GroupBox::new()
                    .fill()
                    .title("Saved Config Audit".to_string())
                    .child(
                        v_flex()
                            .gap_3()
                            .when_some(
                                workspace.reachability.audit_progress.as_ref(),
                                |this, progress| {
                                    this.child(render_audit_progress_summary(progress, cx))
                                },
                            )
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_2()
                                    .flex_wrap()
                                    .child(
                                        Tag::secondary()
                                            .small()
                                            .rounded_full()
                                            .child(format!("{} configs", result.total_configs)),
                                    )
                                    .child(
                                        Tag::info()
                                            .small()
                                            .rounded_full()
                                            .child(format!("{} endpoints", result.endpoint_rows)),
                                    )
                                    .child(
                                        Tag::success()
                                            .small()
                                            .rounded_full()
                                            .child(format!("{} reachable", result.reachable_rows)),
                                    )
                                    .when(result.partial_rows > 0, |this| {
                                        this.child(
                                            Tag::warning()
                                                .small()
                                                .rounded_full()
                                                .child(format!("{} partial", result.partial_rows)),
                                        )
                                    })
                                    .when(result.failed_rows > 0, |this| {
                                        this.child(
                                            Tag::danger()
                                                .small()
                                                .rounded_full()
                                                .child(format!("{} failed", result.failed_rows)),
                                        )
                                    })
                                    .when(result.issue_rows > 0, |this| {
                                        this.child(
                                            Tag::warning()
                                                .small()
                                                .rounded_full()
                                                .child(format!("{} issues", result.issue_rows)),
                                        )
                                    }),
                            )
                            .child(render_audit_filter_bar(workspace, filtered_rows.len(), cx))
                            .child(div().max_h(px(360.0)).overflow_y_scrollbar().child(
                                if filtered_rows.is_empty() {
                                    empty_result_state("No rows match the current filter.", cx)
                                        .into_any_element()
                                } else {
                                    v_flex()
                                        .gap_2()
                                        .children(filtered_rows.into_iter().map(|row| {
                                            div()
                                                .rounded_lg()
                                                .border_1()
                                                .border_color(cx.theme().border.alpha(0.6))
                                                .bg(cx.theme().group_box)
                                                .px_3()
                                                .py_2()
                                                .child(
                                                    v_flex()
                                                        .gap_1()
                                                        .child(
                                                            h_flex()
                                                                .items_center()
                                                                .justify_between()
                                                                .gap_3()
                                                                .flex_wrap()
                                                                .child(
                                                                    h_flex()
                                                                        .items_center()
                                                                        .gap_2()
                                                                        .flex_wrap()
                                                                        .child(
                                                                            div()
                                                                                .text_sm()
                                                                                .font_semibold()
                                                                                .child(
                                                                                    row.config_name
                                                                                        .clone(),
                                                                                ),
                                                                        )
                                                                        .child(
                                                                            Tag::secondary()
                                                                                .small()
                                                                                .rounded_full()
                                                                                .child(
                                                                                    row.peer_label
                                                                                        .clone(),
                                                                                ),
                                                                        ),
                                                                )
                                                                .child(batch_status_tag(
                                                                    row.status,
                                                                )),
                                                        )
                                                        .child(
                                                            div()
                                                                .text_xs()
                                                                .text_color(
                                                                    cx.theme().muted_foreground,
                                                                )
                                                                .child(row.target.clone()),
                                                        )
                                                        .child(
                                                            div()
                                                                .text_xs()
                                                                .text_color(
                                                                    cx.theme().muted_foreground,
                                                                )
                                                                .child(row.summary.clone()),
                                                        ),
                                                )
                                        }))
                                        .into_any_element()
                                },
                            )),
                    ),
            )
        }
    }
}

fn render_audit_progress_state(
    workspace: &ToolsWorkspace,
    cx: &mut Context<ToolsWorkspace>,
) -> Div {
    match workspace.reachability.audit_progress.as_ref() {
        Some(progress) => v_flex()
            .gap_3()
            .child(render_audit_progress_summary(progress, cx))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("Saved config audit is running…"),
            ),
        None => empty_result_state("Saved config audit is running…", cx),
    }
}

fn render_audit_progress_summary(
    progress: &ReachabilityAuditProgress,
    cx: &mut Context<ToolsWorkspace>,
) -> Div {
    v_flex()
        .gap_2()
        .child(
            h_flex()
                .items_center()
                .gap_2()
                .flex_wrap()
                .child(
                    Tag::info()
                        .small()
                        .rounded_full()
                        .child(progress.phase.label()),
                )
                .child(Tag::secondary().small().rounded_full().child(format!(
                    "{} / {} configs",
                    progress.processed_configs, progress.total_configs
                )))
                .when(
                    progress.phase != ReachabilityAuditPhase::LoadingConfigs,
                    |this| {
                        this.child(Tag::secondary().small().rounded_full().child(format!(
                            "{} / {} endpoints",
                            progress.completed_endpoints, progress.total_endpoints
                        )))
                    },
                ),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(progress_summary_text(progress)),
        )
}

fn render_audit_filter_bar(
    workspace: &ToolsWorkspace,
    visible_rows: usize,
    cx: &mut Context<ToolsWorkspace>,
) -> Div {
    h_flex()
        .items_center()
        .justify_between()
        .gap_3()
        .flex_wrap()
        .child(
            ButtonGroup::new("tools-reach-audit-filter")
                .outline()
                .compact()
                .small()
                .child(audit_filter_button(
                    ReachabilityAuditFilter::All,
                    workspace.reachability.audit_filter,
                    cx,
                ))
                .child(audit_filter_button(
                    ReachabilityAuditFilter::Failures,
                    workspace.reachability.audit_filter,
                    cx,
                ))
                .child(audit_filter_button(
                    ReachabilityAuditFilter::Issues,
                    workspace.reachability.audit_filter,
                    cx,
                )),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format!("{visible_rows} visible rows")),
        )
}

fn render_socket_list(
    title: &str,
    rows: Vec<String>,
    cx: &mut Context<ToolsWorkspace>,
) -> GroupBox {
    GroupBox::new()
        .fill()
        .title(title.to_string())
        .child(if rows.is_empty() {
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("None")
                .into_any_element()
        } else {
            div()
                .max_h(px(180.0))
                .overflow_y_scrollbar()
                .child(v_flex().gap_2().children(rows.into_iter().map(|row| {
                    div()
                        .rounded_md()
                        .border_1()
                        .border_color(cx.theme().border.alpha(0.55))
                        .bg(cx.theme().secondary.alpha(0.55))
                        .px_3()
                        .py_2()
                        .text_sm()
                        .child(row)
                })))
                .into_any_element()
        })
}

fn family_button(
    family: AddressFamilyPreference,
    state: &ReachabilityToolState,
    cx: &mut Context<ToolsWorkspace>,
) -> Button {
    let (id, label) = match family {
        AddressFamilyPreference::System => ("tools-reach-family-system", "System"),
        AddressFamilyPreference::PreferIpv4 => ("tools-reach-family-v4", "Prefer IPv4"),
        AddressFamilyPreference::PreferIpv6 => ("tools-reach-family-v6", "Prefer IPv6"),
    };
    Button::new(id)
        .label(label)
        .selected(state.form.family_preference == family)
        .on_click(cx.listener(move |this, _, _, cx| {
            if this.set_family_preference(family) {
                cx.notify();
            }
        }))
}

fn attempt_result_tag(result: ReachabilityAttemptResult) -> Tag {
    match result {
        ReachabilityAttemptResult::Resolved => Tag::info().small().rounded_full().child("Resolved"),
        ReachabilityAttemptResult::Connected => {
            Tag::success().small().rounded_full().child("Connected")
        }
        ReachabilityAttemptResult::Refused => {
            Tag::warning().small().rounded_full().child("Refused")
        }
        ReachabilityAttemptResult::TimedOut => {
            Tag::danger().small().rounded_full().child("Timed Out")
        }
        ReachabilityAttemptResult::Failed => Tag::danger().small().rounded_full().child("Failed"),
    }
}

fn batch_status_tag(status: ReachabilityBatchStatus) -> Tag {
    match status {
        ReachabilityBatchStatus::Resolved => {
            Tag::info().small().rounded_full().child(status.label())
        }
        ReachabilityBatchStatus::Reachable => {
            Tag::success().small().rounded_full().child(status.label())
        }
        ReachabilityBatchStatus::PartiallyReachable => {
            Tag::warning().small().rounded_full().child(status.label())
        }
        ReachabilityBatchStatus::Failed
        | ReachabilityBatchStatus::ParseError
        | ReachabilityBatchStatus::ReadError
        | ReachabilityBatchStatus::Cancelled => {
            Tag::danger().small().rounded_full().child(status.label())
        }
        ReachabilityBatchStatus::NoEndpoint => Tag::secondary()
            .small()
            .rounded_full()
            .child(status.label()),
    }
}

fn audit_filter_button(
    filter: ReachabilityAuditFilter,
    active: ReachabilityAuditFilter,
    cx: &mut Context<ToolsWorkspace>,
) -> Button {
    let id = match filter {
        ReachabilityAuditFilter::All => "tools-reach-audit-filter-all",
        ReachabilityAuditFilter::Failures => "tools-reach-audit-filter-failures",
        ReachabilityAuditFilter::Issues => "tools-reach-audit-filter-issues",
    };
    Button::new(id)
        .label(filter.label())
        .selected(active == filter)
        .on_click(cx.listener(move |this, _, _, cx| {
            if this.set_reachability_audit_filter(filter) {
                cx.notify();
            }
        }))
}

fn progress_summary_text(progress: &ReachabilityAuditProgress) -> String {
    match progress.phase {
        ReachabilityAuditPhase::LoadingConfigs => format!(
            "Loaded {} of {} saved configs and discovered {} endpoints so far.",
            progress.processed_configs, progress.total_configs, progress.total_endpoints
        ),
        ReachabilityAuditPhase::ProbingEndpoints => format!(
            "Resolved {} configs and checked {} of {} endpoints.",
            progress.processed_configs, progress.completed_endpoints, progress.total_endpoints
        ),
        ReachabilityAuditPhase::Finalizing => format!(
            "Checked {} endpoints. Finalizing the aggregated result.",
            progress.completed_endpoints
        ),
        ReachabilityAuditPhase::Completed => format!(
            "Scanned {} configs and checked {} endpoints.",
            progress.total_configs, progress.total_endpoints
        ),
    }
}
