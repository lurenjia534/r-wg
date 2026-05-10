use gpui::prelude::FluentBuilder as _;
use gpui::{Context, Stateful, *};
use gpui_component::{
    h_flex, scroll::ScrollableElement, tag::Tag, v_flex, ActiveTheme as _, Sizable as _,
    StyledExt as _,
};

use crate::ui::features::configs::state::{ConfigsWorkspace, DraftValidationState};
use crate::ui::format::{format_addresses, format_allowed_ips, format_dns, format_route_table};
use crate::ui::state::ConfigInspectorTab;

use super::{ConfigsLayoutMode, ConfigsRuntimeView, ConfigsViewData};

// Inspector panes and compact inspector tab controls.

pub(super) fn render_inspector_panel(
    runtime: &ConfigsRuntimeView,
    workspace: &Entity<ConfigsWorkspace>,
    inspector_tab: ConfigInspectorTab,
    mode: ConfigsLayoutMode,
    data: &ConfigsViewData,
    cx: &mut Context<ConfigsWorkspace>,
) -> Div {
    let compact = matches!(mode, ConfigsLayoutMode::Compact);
    let framed = compact;
    let validation_badge = match &data.draft.validation {
        DraftValidationState::Idle => Tag::secondary().small().child("Idle"),
        DraftValidationState::Valid { .. } => Tag::success().small().child("Valid"),
        DraftValidationState::Invalid { .. } => Tag::danger().small().child("Invalid"),
    };
    let save_badge = if data.shared.draft_dirty {
        Tag::warning().small().child("Dirty")
    } else {
        Tag::secondary().small().child("Saved")
    };
    let runtime_badge = if data.shared.needs_restart {
        Tag::warning().small().child("Restart")
    } else if data.is_running_draft {
        Tag::success().small().child("Running")
    } else {
        Tag::secondary().small().child("Stored")
    };
    let preview_card = {
        let addresses = data
            .shared
            .parsed_config
            .as_ref()
            .map(|cfg| format_addresses(&cfg.interface))
            .unwrap_or_else(|| "-".to_string());
        let dns = data
            .shared
            .parsed_config
            .as_ref()
            .map(|cfg| format_dns(&cfg.interface))
            .unwrap_or_else(|| "-".to_string());
        let route_table = data
            .shared
            .parsed_config
            .as_ref()
            .map(|cfg| format_route_table(cfg.interface.table))
            .unwrap_or_else(|| "-".to_string());
        let routes = data
            .shared
            .parsed_config
            .as_ref()
            .map(|cfg| format_allowed_ips(&cfg.peers))
            .unwrap_or_else(|| "-".to_string());
        let peers = data
            .shared
            .parsed_config
            .as_ref()
            .map(|cfg| cfg.peers.len().to_string())
            .unwrap_or_else(|| "0".to_string());
        let source = data.source_summary.clone();

        inspector_card(
            "Preview",
            "Interface and routing summary",
            v_flex()
                .gap_1()
                .child(inspector_summary_row("Source", source, cx))
                .child(inspector_summary_row("Local", addresses, cx))
                .child(inspector_summary_row("DNS", dns, cx))
                .child(inspector_summary_row("Route Table", route_table, cx))
                .child(inspector_summary_row("Allowed IPs", routes, cx))
                .child(inspector_summary_row("Peers", peers, cx)),
            compact,
            cx,
        )
    };

    let diagnostics_card = {
        let (state, line, message) = match &data.draft.validation {
            DraftValidationState::Idle => (
                "Idle".to_string(),
                None,
                "Start editing to validate this config.".to_string(),
            ),
            DraftValidationState::Valid { .. } => (
                "Valid".to_string(),
                None,
                "Config parses successfully.".to_string(),
            ),
            DraftValidationState::Invalid { line, message, .. } => {
                ("Invalid".to_string(), *line, message.to_string())
            }
        };
        let save_state = if data.shared.draft_dirty {
            "Unsaved changes".to_string()
        } else {
            "Saved".to_string()
        };
        let runtime_state = if data.shared.needs_restart {
            "Restart required".to_string()
        } else if data.is_running_draft {
            "Running tunnel".to_string()
        } else {
            "Stored config".to_string()
        };
        let details = v_flex()
            .gap_1()
            .child(inspector_summary_row("Validation", state.clone(), cx))
            .child(inspector_summary_row("Save", save_state, cx))
            .child(inspector_summary_row("Runtime", runtime_state, cx))
            .when_some(line, |this, line| {
                this.child(inspector_summary_row("Line", line.to_string(), cx))
            });

        inspector_card(
            "Diagnostics",
            "Validation detail and save state",
            v_flex()
                .gap_2()
                .child(details)
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(message),
                )
                .when(data.shared.needs_restart, |this| {
                    this.child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("Restart the running tunnel after saving to apply this draft."),
                    )
                }),
            compact,
            cx,
        )
    };

    let activity_card = inspector_card(
        "Activity",
        "Recent runtime notes",
        v_flex()
            .gap_1()
            .child(inspector_activity_row(
                "Latest Status",
                activity_value_or_fallback(&runtime.latest_status, "No recent status update."),
                cx,
            ))
            .child(inspector_activity_row(
                "Last Error",
                activity_value_or_fallback(&runtime.last_error, "No recent error recorded."),
                cx,
            ))
            .child(inspector_activity_row(
                "Running Tunnel",
                activity_value_or_fallback(&runtime.running_name, "No tunnel is running."),
                cx,
            ))
            .child(inspector_activity_row(
                "Handshake",
                activity_value_or_fallback(
                    &data.shared.last_handshake,
                    "No handshake recorded yet.",
                ),
                cx,
            ))
            .child(inspector_activity_row(
                "DAITA",
                activity_value_or_fallback(&data.shared.daita_overhead, "DAITA inactive."),
                cx,
            )),
        compact,
        cx,
    );

    let inspector_tabs = inspector_tab_row(workspace, inspector_tab, cx);
    let inspector_body = if compact {
        match inspector_tab {
            ConfigInspectorTab::Preview => preview_card.into_any_element(),
            ConfigInspectorTab::Diagnostics => diagnostics_card.into_any_element(),
            ConfigInspectorTab::Activity => activity_card.into_any_element(),
        }
    } else {
        v_flex()
            .gap_0()
            .child(preview_card)
            .child(diagnostics_card)
            .child(activity_card)
            .into_any_element()
    };

    div()
        .flex()
        .flex_col()
        .flex_1()
        .w_full()
        .min_w(px(0.0))
        .h_full()
        .min_h(px(0.0))
        .bg(cx
            .theme()
            .background
            .alpha(if compact { 0.0 } else { 0.68 }))
        .when(framed, |this| {
            this.rounded_xl()
                .border_1()
                .border_color(cx.theme().border.alpha(0.9))
                .bg(cx.theme().background)
        })
        .when(compact, |this| {
            this.child(
                div()
                    .px(px(16.0))
                    .py(px(16.0))
                    .border_b_1()
                    .border_color(cx.theme().border.alpha(0.62))
                    .child(
                        v_flex()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_xs()
                                    .font_semibold()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("INSPECTOR"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .font_semibold()
                                    .child("Config summary and runtime notes."),
                            ),
                    ),
            )
        })
        .when(!compact, |this| {
            this.child(
                div()
                    .px(px(14.0))
                    .py(px(13.0))
                    .border_b_1()
                    .border_color(cx.theme().border.alpha(0.74))
                    .bg(cx.theme().background.alpha(0.9))
                    .child(
                        v_flex()
                            .gap_2()
                            .child(
                                div()
                                    .text_xs()
                                    .font_semibold()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("INSPECTOR"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .font_semibold()
                                    .truncate()
                                    .child(data.title.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .truncate()
                                    .child(data.source_summary.clone()),
                            )
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_2()
                                    .flex_wrap()
                                    .child(validation_badge)
                                    .child(save_badge)
                                    .child(runtime_badge),
                            ),
                    ),
            )
        })
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h(px(0.0))
                .overflow_y_scrollbar()
                .px(px(if compact { 12.0 } else { 16.0 }))
                .py(px(if compact { 12.0 } else { 14.0 }))
                .when(compact, |this| this.child(inspector_tabs))
                .child(inspector_body),
        )
}

fn inspector_tab_row(
    workspace: &Entity<ConfigsWorkspace>,
    inspector_tab: ConfigInspectorTab,
    cx: &mut Context<ConfigsWorkspace>,
) -> Div {
    h_flex()
        .items_end()
        .gap_4()
        .w_full()
        .border_b_1()
        .border_color(cx.theme().border.alpha(0.6))
        .child(inspector_tab_button(
            "inspector-preview",
            "Preview",
            ConfigInspectorTab::Preview,
            inspector_tab,
            workspace,
            cx,
        ))
        .child(inspector_tab_button(
            "inspector-diagnostics",
            "Diagnostics",
            ConfigInspectorTab::Diagnostics,
            inspector_tab,
            workspace,
            cx,
        ))
        .child(inspector_tab_button(
            "inspector-activity",
            "Activity",
            ConfigInspectorTab::Activity,
            inspector_tab,
            workspace,
            cx,
        ))
}

fn inspector_tab_button(
    id: &'static str,
    label: &'static str,
    value: ConfigInspectorTab,
    current: ConfigInspectorTab,
    workspace: &Entity<ConfigsWorkspace>,
    cx: &mut Context<ConfigsWorkspace>,
) -> Stateful<Div> {
    let selected = current == value;
    let text_color = if selected {
        cx.theme().foreground
    } else {
        cx.theme().muted_foreground
    };
    let underline = if selected {
        cx.theme().accent
    } else {
        cx.theme().background.alpha(0.0)
    };

    div()
        .id(id)
        .flex()
        .flex_col()
        .items_center()
        .gap_2()
        .pt_1()
        .pb_0p5()
        .cursor_pointer()
        .child(
            div()
                .text_sm()
                .font_weight(if selected {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::MEDIUM
                })
                .text_color(text_color)
                .child(label),
        )
        .child(div().w_full().h(px(2.0)).rounded_full().bg(underline))
        .on_click({
            let workspace = workspace.clone();
            move |_, _, cx| {
                workspace.update(cx, |workspace, cx| {
                    let changed = workspace.set_inspector_tab(value);
                    workspace.app.update(cx, |app, cx| {
                        app.persist_preferred_inspector_tab(value, cx);
                    });
                    if changed {
                        cx.notify();
                    }
                });
            }
        })
}

fn inspector_card<T: IntoElement>(
    title: &'static str,
    subtitle: &'static str,
    body: T,
    compact: bool,
    cx: &mut Context<ConfigsWorkspace>,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .when(compact, |this| {
            this.rounded_lg()
                .border_1()
                .border_color(cx.theme().border.alpha(0.48))
                .bg(cx.theme().group_box)
                .p_3()
        })
        .when(!compact, |this| {
            this.py_3()
                .border_b_1()
                .border_color(cx.theme().border.alpha(0.58))
        })
        .child(
            v_flex()
                .gap_0p5()
                .child(
                    div()
                        .text_xs()
                        .font_semibold()
                        .text_color(cx.theme().muted_foreground)
                        .child(title.to_uppercase()),
                )
                .when(compact, |this| {
                    this.child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(subtitle),
                    )
                }),
        )
        .child(body)
}

fn inspector_summary_row(
    label: &'static str,
    value: String,
    cx: &mut Context<ConfigsWorkspace>,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_0p5()
        .py(px(5.0))
        .border_b_1()
        .border_color(cx.theme().border.alpha(0.36))
        .child(
            div()
                .text_xs()
                .font_semibold()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(div().text_xs().child(value))
}

fn inspector_activity_row(
    label: &'static str,
    value: String,
    cx: &mut Context<ConfigsWorkspace>,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_0p5()
        .py(px(5.0))
        .border_b_1()
        .border_color(cx.theme().border.alpha(0.36))
        .child(
            div()
                .text_xs()
                .font_semibold()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(div().text_xs().child(value))
}

fn activity_value_or_fallback(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() || value == "-" || value.eq_ignore_ascii_case("none") || value == "never" {
        fallback.to_string()
    } else {
        value.to_string()
    }
}
