use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonGroup},
    group_box::{GroupBox, GroupBoxVariants},
    h_flex,
    scroll::{ScrollableElement as _, Scrollbar},
    tag::Tag,
    v_flex, ActiveTheme as _, Selectable, Sizable as _, StyledExt as _,
};

use r_wg::backend::wg::tools::{ReachabilityAttemptResult, ReachabilityMode, ReachabilityVerdict};

use super::components::{empty_result_state, error_banner, warning_banner};
use super::state::{
    AsyncJobState, ReachabilityAuditFilter, ReachabilityAuditPhase, ReachabilityAuditProgress,
    ReachabilityBatchRow, ReachabilityBatchStatus, ReachabilitySingleViewModel, ReachabilityTab,
    ToolsWorkspace,
};

const AUDIT_LIST_SCROLL_STATE_ID: &str = "tools-reach-audit-scroll";

pub(super) fn render_reachability_result_panel(
    workspace: &ToolsWorkspace,
    window: &mut Window,
    cx: &mut Context<ToolsWorkspace>,
) -> GroupBox {
    let active_error = match workspace.reachability.active_tab {
        ReachabilityTab::Single => workspace.reachability.single_error.clone(),
        ReachabilityTab::Audit => workspace.reachability.audit_error.clone(),
    };
    let active_notice = match workspace.reachability.active_tab {
        ReachabilityTab::Single => None,
        ReachabilityTab::Audit => workspace.reachability.audit_notice.clone(),
    };
    let stale_message = match workspace.reachability.active_tab {
        ReachabilityTab::Single => match &workspace.reachability.single {
            AsyncJobState::Ready(view_model) => workspace
                .current_reachability_request_snapshot(cx)
                .ok()
                .filter(|request| request != &view_model.request)
                .map(|_| "Inputs changed since this result was produced. Re-run Test to refresh."),
            _ => None,
        },
        ReachabilityTab::Audit => match &workspace.reachability.audit {
            AsyncJobState::Ready(view_model) => workspace
                .current_reachability_audit_request(cx)
                .ok()
                .filter(|request| request != &view_model.request)
                .map(|_| "Audit settings changed since this result was produced. Re-run Audit to refresh."),
            _ => None,
        },
    };
    GroupBox::new().fill().title("Result".to_string()).child(
        v_flex()
            .gap_3()
            .when_some(stale_message, |this, message| {
                this.child(warning_banner(message, cx))
            })
            .when_some(active_notice, |this, message| {
                this.child(warning_banner(message, cx))
            })
            .when_some(active_error, |this, message| {
                this.child(error_banner(message, cx))
            })
            .child(match workspace.reachability.active_tab {
                ReachabilityTab::Single => render_single_result(workspace, cx).into_any_element(),
                ReachabilityTab::Audit => {
                    render_audit_result(workspace, window, cx).into_any_element()
                }
            }),
    )
}

fn render_single_result(workspace: &ToolsWorkspace, cx: &mut Context<ToolsWorkspace>) -> Div {
    match &workspace.reachability.single {
        AsyncJobState::Idle => empty_result_state(
            "Run a reachability test to inspect resolved addresses and per-address attempts.",
            cx,
        ),
        AsyncJobState::Running { .. } => empty_result_state("Reachability test is running...", cx),
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
                .map(|addr| format_resolved_address(result.mode, *addr))
                .collect(),
            cx,
        ))
        .when(
            result.mode != ReachabilityMode::ResolveOnly && !result.attempts.is_empty(),
            |this| {
                this.child(
                    GroupBox::new().fill().title("Attempts".to_string()).child(
                        div().max_h(px(280.0)).overflow_y_scrollbar().child(
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
            },
        )
}

fn render_audit_result(
    workspace: &ToolsWorkspace,
    window: &mut Window,
    cx: &mut Context<ToolsWorkspace>,
) -> Div {
    match &workspace.reachability.audit {
        AsyncJobState::Idle => empty_result_state(
            "Run an audit to scan all saved configs and summarize endpoint reachability.",
            cx,
        ),
        AsyncJobState::Running { .. } => render_audit_progress_state(workspace, cx),
        AsyncJobState::Failed(message) => error_banner(message.clone(), cx),
        AsyncJobState::Ready(view_model) => {
            let result = &view_model.result;
            let audit_mode = view_model.request.mode;
            let filtered_indices = result
                .rows
                .iter()
                .enumerate()
                .filter(|(_, row)| workspace.reachability.audit_filter.matches(row.status))
                .map(|(ix, _)| ix)
                .collect::<Vec<_>>();
            let visible_rows = filtered_indices.len();
            let rows = result.rows.clone();
            let visible_indices = filtered_indices.clone();
            let scroll_handle = window
                .use_keyed_state(AUDIT_LIST_SCROLL_STATE_ID, cx, |_, _| {
                    UniformListScrollHandle::new()
                })
                .read(cx)
                .clone();
            let list = uniform_list(
                "tools-reach-audit-list",
                visible_indices.len(),
                move |visible_range, _window, cx| {
                    visible_range
                        .map(|ix| render_audit_row(&rows[visible_indices[ix]], cx))
                        .collect::<Vec<_>>()
                },
            )
            .track_scroll(scroll_handle.clone())
            .w_full()
            .flex_1();
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
                                    .child(if audit_mode == ReachabilityMode::ResolveOnly {
                                        Tag::info()
                                            .small()
                                            .rounded_full()
                                            .child(format!("{} resolved", result.resolved_rows))
                                    } else {
                                        Tag::success()
                                            .small()
                                            .rounded_full()
                                            .child(format!("{} reachable", result.reachable_rows))
                                    })
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
                            .child(render_audit_filter_bar(workspace, visible_rows, cx))
                            .child(
                                div()
                                    .relative()
                                    .max_h(px(520.0))
                                    .min_h(px(220.0))
                                    .overflow_hidden()
                                    .child(if filtered_indices.is_empty() {
                                        empty_result_state("No rows match the current filter.", cx)
                                            .into_any_element()
                                    } else {
                                        div()
                                            .flex()
                                            .flex_col()
                                            .flex_1()
                                            .min_h(px(0.0))
                                            .child(list)
                                            .child(Scrollbar::vertical(&scroll_handle))
                                            .into_any_element()
                                    }),
                            ),
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
                    .child(if workspace.reachability.audit_cancelling {
                        "Stopping queued and in-flight probes..."
                    } else {
                        "Saved config audit is running..."
                    }),
            ),
        None => empty_result_state(
            if workspace.reachability.audit_cancelling {
                "Stopping queued and in-flight probes..."
            } else {
                "Saved config audit is running..."
            },
            cx,
        ),
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
            h_flex().items_center().gap_2().flex_wrap().child(
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
            ),
        )
        .child(
            v_flex().gap_1().items_end().child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{visible_rows} visible rows")),
            ),
        )
}

fn render_audit_row(row: &ReachabilityBatchRow, cx: &mut App) -> Div {
    div()
        .rounded_lg()
        .border_1()
        .border_color(cx.theme().border.alpha(0.6))
        .bg(cx.theme().group_box)
        .px_3()
        .py_2()
        .h(px(82.0))
        .child(
            v_flex()
                .gap_1()
                .child(
                    h_flex()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .child(
                            h_flex()
                                .items_center()
                                .gap_2()
                                .flex_1()
                                .min_w(px(0.0))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.0))
                                        .text_sm()
                                        .font_semibold()
                                        .truncate()
                                        .child(row.config_name.clone()),
                                )
                                .child(
                                    Tag::secondary()
                                        .small()
                                        .rounded_full()
                                        .child(row.peer_label.clone()),
                                ),
                        )
                        .child(batch_status_tag(row.status)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .truncate()
                        .child(row.target.clone()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .truncate()
                        .child(row.summary.clone()),
                ),
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

fn format_resolved_address(mode: ReachabilityMode, address: std::net::SocketAddr) -> String {
    if mode == ReachabilityMode::ResolveOnly {
        address.ip().to_string()
    } else {
        address.to_string()
    }
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
        | ReachabilityBatchStatus::ReadError => {
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
