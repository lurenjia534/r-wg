use gpui::prelude::FluentBuilder as _;
use gpui::{Context, Stateful, *};
use gpui_component::{
    button::{Button, ButtonVariants},
    description_list::DescriptionList,
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
    scroll::ScrollableElement,
    tag::Tag,
    h_flex, v_flex, ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _,
    StyledExt as _,
};

use crate::ui::format::{format_addresses, format_allowed_ips, format_dns, format_route_table};
use crate::ui::state::{
    ConfigInspectorTab, ConfigSource, ConfigsPrimaryPane, ConfigsWorkspace, DraftValidationState,
    EndpointFamily, WgApp,
};
use crate::ui::view::configs::ConfigsViewData;

use super::{ConfigsLayoutMode, ConfigsRuntimeView};

// Inspector panes, diagnostics, activity cards, and helper tags.

pub(super) fn render_inspector_panel(
    runtime: &ConfigsRuntimeView,
    workspace: &Entity<ConfigsWorkspace>,
    inspector_tab: ConfigInspectorTab,
    mode: ConfigsLayoutMode,
    data: &ConfigsViewData,
    cx: &mut Context<ConfigsWorkspace>,
) -> Div {
    let compact = matches!(mode, ConfigsLayoutMode::Compact);
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
            DescriptionList::new()
                .small()
                .columns(1)
                .label_width(px(92.0))
                .bordered(false)
                .item("Source", source, 1)
                .item("Local Address", addresses, 1)
                .item("DNS", dns, 1)
                .item("Route Table", route_table, 1)
                .item("Allowed IPs", routes, 1)
                .item("Peers", peers, 1),
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
        let mut details = DescriptionList::new()
            .small()
            .columns(1)
            .label_width(px(88.0))
            .bordered(false)
            .item("Validation", state.clone(), 1)
            .item("Save", save_state, 1)
            .item("Runtime", runtime_state, 1);
        if let Some(line) = line {
            details = details.item("Line", line.to_string(), 1);
        }

        inspector_card(
            "Diagnostics",
            "Validation detail and save state",
            v_flex()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(message),
                )
                .child(details),
            compact,
            cx,
        )
    };

    let activity_card = inspector_card(
        "Activity",
        "Recent runtime notes",
        v_flex()
            .gap_2()
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
            .gap_3()
            .child(preview_card)
            .child(diagnostics_card)
            .child(activity_card)
            .into_any_element()
    };

    div()
        .flex()
        .flex_col()
        .gap_3()
        .h_full()
        .min_h(px(0.0))
        .rounded_xl()
        .border_1()
        .border_color(cx.theme().border.alpha(0.9))
        .bg(if compact {
            cx.theme().background
        } else {
            cx.theme().background.alpha(0.84)
        })
        .child(
            div()
                .px_4()
                .py_4()
                .border_b_1()
                .border_color(cx.theme().border.alpha(0.62))
                .bg(if compact {
                    linear_gradient(
                        180.0,
                        linear_color_stop(cx.theme().background, 0.0),
                        linear_color_stop(cx.theme().group_box, 1.0),
                    )
                } else {
                    linear_gradient(
                        180.0,
                        linear_color_stop(cx.theme().background.alpha(0.96), 0.0),
                        linear_color_stop(cx.theme().group_box.alpha(0.84), 1.0),
                    )
                })
                .child(
                    v_flex()
                        .gap_1()
                        .child(div().text_lg().font_semibold().child("Inspector"))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child("Preview parsed config, validation, and runtime notes."),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_3()
                .flex_1()
                .min_h(px(0.0))
                .overflow_y_scrollbar()
                .p_3()
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

pub(super) fn render_configs_primary_pane_tabs(
    workspace: &Entity<ConfigsWorkspace>,
    primary_pane: ConfigsPrimaryPane,
    data: &ConfigsViewData,
    cx: &mut Context<ConfigsWorkspace>,
) -> Div {
    h_flex()
        .items_center()
        .gap_2()
        .w_full()
        .rounded_xl()
        .border_1()
        .border_color(cx.theme().border.alpha(0.7))
        .bg(cx.theme().background.alpha(0.9))
        .p_1()
        .child(configs_primary_pane_button(
            "configs-pane-library",
            "Library",
            ConfigsPrimaryPane::Library,
            primary_pane,
            workspace,
            cx,
        ))
        .child(configs_primary_pane_button(
            "configs-pane-editor",
            if data.has_selection || !data.draft.name.is_empty() {
                "Editor"
            } else {
                "Draft"
            },
            ConfigsPrimaryPane::Editor,
            primary_pane,
            workspace,
            cx,
        ))
        .child(configs_primary_pane_button(
            "configs-pane-inspector",
            "Inspector",
            ConfigsPrimaryPane::Inspector,
            primary_pane,
            workspace,
            cx,
        ))
}

fn configs_primary_pane_button(
    id: &'static str,
    label: &'static str,
    value: ConfigsPrimaryPane,
    current: ConfigsPrimaryPane,
    workspace: &Entity<ConfigsWorkspace>,
    cx: &mut Context<ConfigsWorkspace>,
) -> Stateful<Div> {
    let selected = current == value;
    let bg = if selected {
        cx.theme().group_box
    } else {
        cx.theme().background.alpha(0.0)
    };
    let border = if selected {
        cx.theme().accent.alpha(0.32)
    } else {
        cx.theme().background.alpha(0.0)
    };
    let text_color = if selected {
        cx.theme().foreground
    } else {
        cx.theme().muted_foreground
    };
    let hover_bg = if selected {
        cx.theme().group_box
    } else {
        cx.theme().list_hover
    };

    div()
        .id(id)
        .flex()
        .flex_1()
        .items_center()
        .justify_center()
        .px_3()
        .py_2()
        .rounded_lg()
        .border_1()
        .border_color(border)
        .bg(bg)
        .cursor_pointer()
        .hover(move |this| this.bg(hover_bg))
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
        .on_click({
            let workspace = workspace.clone();
            move |_, _, cx| {
                workspace.update(cx, |workspace, cx| {
                    if workspace.set_primary_pane(value) {
                        cx.notify();
                    }
                });
            }
        })
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

pub(super) fn render_diagnostics_strip(
    data: &ConfigsViewData,
    cx: &mut Context<ConfigsWorkspace>,
) -> Div {
    let (tone_bg, tone_border, tone_bar, title, detail, icon, line_tag) =
        match &data.draft.validation {
            DraftValidationState::Idle => (
                cx.theme().secondary.alpha(0.45),
                cx.theme().border.alpha(0.45),
                cx.theme().muted_foreground.alpha(0.5),
                "Draft not validated".to_string(),
                "Start editing to validate this config.".to_string(),
                IconName::Info,
                None,
            ),
            DraftValidationState::Valid { .. } => (
                cx.theme().success.alpha(0.12),
                cx.theme().success.alpha(0.28),
                cx.theme().success,
                if data.shared.draft_dirty {
                    "Unsaved changes".to_string()
                } else {
                    "Saved config".to_string()
                },
                if data.shared.needs_restart {
                    "Syntax looks good. Save and restart the running tunnel to apply the changes."
                        .to_string()
                } else if data.shared.draft_dirty {
                    "Syntax looks good. Save this draft to update the stored config.".to_string()
                } else {
                    "WireGuard config parsed successfully.".to_string()
                },
                IconName::CircleCheck,
                None,
            ),
            DraftValidationState::Invalid { line, message, .. } => (
                cx.theme().danger.alpha(0.08),
                cx.theme().danger.alpha(0.3),
                cx.theme().danger,
                "Validation error".to_string(),
                match line {
                    Some(line) => format!("Line {line}: {message}"),
                    None => message.to_string(),
                },
                IconName::CircleX,
                line.map(|line| format!("Line {line}")),
            ),
        };

    div()
        .flex()
        .items_start()
        .gap_3()
        .px_0()
        .py_0()
        .rounded_lg()
        .border_1()
        .border_color(tone_border)
        .bg(tone_bg)
        .child(div().w(px(3.0)).h_full().rounded_lg().bg(tone_bar))
        .child(
            h_flex()
                .items_start()
                .justify_between()
                .gap_3()
                .flex_1()
                .px_3()
                .py_3()
                .child(
                    h_flex()
                        .items_start()
                        .gap_3()
                        .child(
                            div()
                                .mt(px(1.0))
                                .child(Icon::new(icon).size_4().text_color(tone_bar)),
                        )
                        .child(
                            v_flex()
                                .gap_1()
                                .child(div().text_sm().font_semibold().child(title))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(detail),
                                ),
                        ),
                )
                .child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .flex_wrap()
                        .when_some(line_tag, |this, line_tag| {
                            this.child(Tag::danger().small().child(line_tag))
                        })
                        .when(data.shared.needs_restart, |this| {
                            this.child(Tag::warning().small().child("Restart"))
                        })
                        .when(data.is_running_draft, |this| {
                            this.child(Tag::success().small().child("Running"))
                        }),
                ),
        )
}

pub(super) fn editor_action_bar(
    data: &ConfigsViewData,
    app_handle: &Entity<WgApp>,
    _cx: &mut Context<ConfigsWorkspace>,
) -> Div {
    let manage_button = if data.is_busy || !data.has_saved_source || !data.has_selection {
        Button::new("cfg-manage")
            .icon(Icon::new(IconName::Menu).size_3())
            .ghost()
            .small()
            .compact()
            .disabled(true)
            .into_any_element()
    } else {
        let menu_handle = app_handle.clone();
        Button::new("cfg-manage")
            .icon(Icon::new(IconName::Menu).size_3())
            .ghost()
            .small()
            .compact()
            .dropdown_caret(true)
            .dropdown_menu_with_anchor(Corner::TopRight, move |menu: PopupMenu, _, _| {
                let rename_handle = menu_handle.clone();
                let export_handle = menu_handle.clone();
                let copy_handle = menu_handle.clone();
                let delete_handle = menu_handle.clone();
                menu.item(PopupMenuItem::new("Rename").on_click({
                    move |_, window, cx| {
                        rename_handle.update(cx, |this, cx| {
                            this.handle_rename_click(window, cx);
                        });
                    }
                }))
                .item(PopupMenuItem::new("Export").on_click({
                    move |_, _, cx| {
                        export_handle.update(cx, |this, cx| {
                            this.handle_export_click(cx);
                        });
                    }
                }))
                .item(PopupMenuItem::new("Copy").on_click({
                    move |_, _, cx| {
                        copy_handle.update(cx, |this, cx| {
                            this.handle_copy_click(cx);
                        });
                    }
                }))
                .item(PopupMenuItem::separator())
                .item(PopupMenuItem::new("Delete").on_click({
                    move |_, window, cx| {
                        delete_handle.update(cx, |this, cx| {
                            this.handle_delete_click(window, cx);
                        });
                    }
                }))
            })
            .into_any_element()
    };

    h_flex()
        .items_center()
        .gap_2()
        .flex_wrap()
        .child(
            Button::new("cfg-save")
                .icon(Icon::new(IconName::Check).size_3())
                .label("Save")
                .primary()
                .small()
                .compact()
                .disabled(!data.can_save)
                .on_click({
                    let app = app_handle.clone();
                    move |_, window, cx| {
                        app.update(cx, |this, cx| {
                            this.handle_save_click(window, cx);
                        });
                    }
                }),
        )
        .child(
            Button::new("cfg-save-as")
                .icon(Icon::new(IconName::Copy).size_3())
                .label("Save as new")
                .outline()
                .small()
                .compact()
                .disabled(data.is_busy)
                .on_click({
                    let app = app_handle.clone();
                    move |_, window, cx| {
                        app.update(cx, |this, cx| {
                            this.handle_save_as_click(window, cx);
                        });
                    }
                }),
        )
        .when(data.can_restart, |this| {
            this.child(
                Button::new("cfg-save-restart")
                    .icon(Icon::new(IconName::Redo2).size_3())
                    .label("Save & Restart")
                    .outline()
                    .small()
                    .compact()
                    .on_click({
                        let app = app_handle.clone();
                        move |_, window, cx| {
                            app.update(cx, |this, cx| {
                                this.handle_save_and_restart_click(window, cx);
                            });
                        }
                    }),
            )
        })
        .child(manage_button)
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
        .rounded_xl()
        .border_1()
        .border_color(cx.theme().border.alpha(0.56))
        .bg(if compact {
            cx.theme().group_box
        } else {
            cx.theme().group_box.alpha(0.84)
        })
        .p_3()
        .child(
            v_flex()
                .gap_1()
                .child(div().text_sm().font_semibold().child(title))
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(subtitle),
                ),
        )
        .child(body)
}

fn inspector_activity_row(
    label: &'static str,
    value: String,
    cx: &mut Context<ConfigsWorkspace>,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .rounded_lg()
        .border_1()
        .border_color(cx.theme().border.alpha(0.42))
        .bg(cx.theme().background.alpha(0.72))
        .px_3()
        .py_2()
        .child(
            div()
                .text_xs()
                .font_semibold()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(div().text_sm().child(value))
}

fn activity_value_or_fallback(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() || value == "-" || value.eq_ignore_ascii_case("none") || value == "never" {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

pub(super) fn source_tag(source: &ConfigSource) -> Tag {
    match source {
        ConfigSource::File { .. } => Tag::secondary().small().child("Imported"),
        ConfigSource::Paste => Tag::secondary().small().child("Saved"),
    }
}

pub(super) fn endpoint_family_tag(family: EndpointFamily) -> Tag {
    match family {
        EndpointFamily::V4 => Tag::secondary().small().child("IPv4"),
        EndpointFamily::V6 => Tag::secondary().small().child("IPv6"),
        EndpointFamily::Dual => Tag::secondary().small().child("Dual"),
        EndpointFamily::Unknown => Tag::secondary().small().child("Unknown"),
    }
}
