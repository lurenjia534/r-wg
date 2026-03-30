use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariants},
    group_box::{GroupBox, GroupBoxVariants},
    h_flex,
    input::Input,
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
    v_flex, ActiveTheme as _, Disableable as _, Sizable as _,
};

use crate::ui::state::{AsyncJobState, CidrViewModel, ToolsWorkspace};

use super::components::{
    empty_result_state, error_banner, readonly_text_block, summary_block, warning_banner,
};

pub(super) fn render_cidr_tab(
    workspace: &ToolsWorkspace,
    stack: bool,
    window: &mut Window,
    cx: &mut Context<ToolsWorkspace>,
) -> Div {
    let include_input = workspace
        .cidr
        .include_input
        .clone()
        .expect("cidr include input should exist");
    let exclude_input = workspace
        .cidr
        .exclude_input
        .clone()
        .expect("cidr exclude input should exist");
    let inputs_disabled = workspace.cidr.job.is_running();

    let form = v_flex()
        .gap_3()
        .child(render_editor_box(
            "Include",
            "Routes you want to keep before exclusions are applied.",
            &include_input,
            render_cidr_prefill_action(workspace, window, inputs_disabled, cx),
            inputs_disabled,
            cx,
        ))
        .child(render_editor_box(
            "Exclude",
            "Routes to subtract from the include set. Supports single IPs and CIDR.",
            &exclude_input,
            None,
            inputs_disabled,
            cx,
        ))
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
                        .child("Input accepts newline or comma separation, and bare IPs auto-expand to /32 or /128."),
                )
                .child(
                    Button::new("tools-cidr-compute")
                        .label(if workspace.cidr.job.is_running() {
                            "Computing..."
                        } else {
                            "Compute"
                        })
                        .primary()
                        .small()
                        .disabled(workspace.cidr.job.is_running())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.compute_cidr(cx);
                        })),
                ),
        );

    let result = render_cidr_result_panel(workspace, stack, cx);

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
            .gap_4()
            .flex_1()
            .min_h(px(0.0))
            .items_start()
            .child(div().w(px(460.0)).min_w(px(360.0)).max_w(px(520.0)).child(form))
            .child(div().flex_1().min_w(px(0.0)).child(result))
    }
}

fn render_editor_box(
    title: &str,
    description: &str,
    input: &Entity<gpui_component::input::InputState>,
    action: Option<AnyElement>,
    disabled: bool,
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
                    .h(px(180.0))
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border.alpha(0.7))
                    .bg(cx.theme().group_box)
                    .px_2()
                    .py_2()
                    .child(
                        Input::new(input)
                            .appearance(false)
                            .bordered(false)
                            .disabled(disabled)
                            .h_full(),
                    ),
            ),
    )
}

fn render_cidr_prefill_action(
    workspace: &ToolsWorkspace,
    _window: &mut Window,
    disabled: bool,
    cx: &mut Context<ToolsWorkspace>,
) -> Option<AnyElement> {
    let Some(parsed) = workspace.active_config.parsed_config() else {
        return Some(
            Button::new("tools-cidr-prefill-disabled")
                .label(if workspace.active_config.is_loading() {
                    "Parsing active config"
                } else {
                    "Load from active config"
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
        .filter(|(_, peer)| !peer.allowed_ips.is_empty())
        .map(|(index, _peer)| (format!("Peer {} AllowedIPs", index + 1), Some(index)))
        .collect::<Vec<_>>();
    if options.is_empty() {
        return None;
    }

    if options.len() == 1 {
        let peer_index = options[0].1.expect("peer index should exist");
        return Some(
            Button::new("tools-cidr-prefill-single")
                .label("Load from active config")
                .outline()
                .xsmall()
                .disabled(disabled)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.load_cidr_prefill_peer(peer_index, window, cx);
                }))
                .into_any_element(),
        );
    }

    Some(
        Button::new("tools-cidr-prefill-menu")
            .label("Load from active config")
            .outline()
            .xsmall()
            .disabled(disabled)
            .dropdown_caret(true)
            .dropdown_menu_with_anchor(Corner::TopRight, {
                let workspace = cx.entity();
                move |menu: PopupMenu, _, _| {
                    let mut menu = menu;
                    for (label, maybe_index) in options.clone() {
                        let workspace = workspace.clone();
                        menu =
                            menu.item(PopupMenuItem::new(label).on_click(move |_, window, cx| {
                                workspace.update(cx, |this, cx| {
                                    if let Some(peer_index) = maybe_index {
                                        this.load_cidr_prefill_peer(peer_index, window, cx);
                                    } else {
                                        this.load_cidr_prefill_union(window, cx);
                                    }
                                });
                            }));
                    }
                    menu.item(PopupMenuItem::new("All peers union").on_click({
                        let workspace = workspace.clone();
                        move |_, window, cx| {
                            workspace.update(cx, |this, cx| {
                                this.load_cidr_prefill_union(window, cx);
                            });
                        }
                    }))
                }
            })
            .into_any_element(),
    )
}

fn render_cidr_result_panel(
    workspace: &ToolsWorkspace,
    stack: bool,
    cx: &mut Context<ToolsWorkspace>,
) -> GroupBox {
    let stale_message = match &workspace.cidr.job {
        AsyncJobState::Ready(result) if workspace.current_cidr_request(cx) != result.request => {
            Some("Inputs changed since this result was produced. Re-run Compute to refresh.")
        }
        _ => None,
    };
    let result_actions = match &workspace.cidr.job {
        AsyncJobState::Ready(result) => Some(
            h_flex()
                .items_center()
                .gap_2()
                .flex_wrap()
                .child(
                    Button::new("tools-cidr-copy-remaining")
                        .label("Copy remaining")
                        .outline()
                        .xsmall()
                        .on_click({
                            let app = workspace.app.clone();
                            let remaining = result.remaining_text.clone();
                            move |_, window, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(
                                    remaining.to_string(),
                                ));
                                let _ = app.update(cx, |app, cx| {
                                    app.push_success_toast("Remaining CIDRs copied", window, cx);
                                });
                            }
                        }),
                )
                .child(
                    Button::new("tools-cidr-copy-allowedips")
                        .label("Copy as AllowedIPs")
                        .outline()
                        .xsmall()
                        .disabled(!result.has_remaining_prefixes())
                        .on_click({
                            let app = workspace.app.clone();
                            let payload = result.allowed_ips_assignment.clone();
                            move |_, window, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(
                                    payload.to_string(),
                                ));
                                let _ = app.update(cx, |app, cx| {
                                    app.push_success_toast("AllowedIPs line copied", window, cx);
                                });
                            }
                        }),
                ),
        ),
        _ => None,
    };

    GroupBox::new().fill().title("CIDR Result".to_string()).child(
        v_flex()
            .gap_3()
            .when_some(stale_message, |this, message| {
                this.child(warning_banner(message, cx))
            })
            .when_some(result_actions, |this, actions| this.child(actions))
            .child(match &workspace.cidr.job {
                AsyncJobState::Idle => empty_result_state(
                    "Run a CIDR exclusion to inspect normalized inputs, remaining routes, and summary stats.",
                    cx,
                )
                .into_any_element(),
                AsyncJobState::Running { .. } => empty_result_state("CIDR computation is running…", cx)
                    .into_any_element(),
                AsyncJobState::Failed(message) => error_banner(message.clone(), cx).into_any_element(),
                AsyncJobState::Ready(result) => render_cidr_result_sections(result, stack, cx).into_any_element(),
            }),
    )
}

fn render_cidr_result_sections(
    result: &CidrViewModel,
    stack: bool,
    cx: &mut Context<ToolsWorkspace>,
) -> Div {
    div()
        .grid()
        .gap_3()
        .grid_cols(if stack { 1 } else { 2 })
        .child(readonly_text_block(
            "Remaining CIDRs",
            result.remaining_text.as_ref(),
            true,
            cx,
        ))
        .child(summary_block("Summary", &result.summary_rows, cx))
        .child(readonly_text_block(
            "Normalized Include",
            result.normalized_include_text.as_ref(),
            true,
            cx,
        ))
        .child(readonly_text_block(
            "Normalized Exclude",
            result.normalized_exclude_text.as_ref(),
            true,
            cx,
        ))
}
