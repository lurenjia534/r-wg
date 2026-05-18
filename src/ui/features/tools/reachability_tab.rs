use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonGroup, ButtonVariants},
    group_box::{GroupBox, GroupBoxVariants},
    h_flex,
    input::Input,
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
    tab::{Tab, TabBar},
    v_flex, ActiveTheme as _, Disableable as _, Selectable, Sizable as _, StyledExt as _,
};

use r_wg::backend::wg::tools::{AddressFamilyPreference, ReachabilityMode};

use super::reachability_results::render_reachability_result_panel;
use super::state::{ReachabilityFormState, ReachabilityTab, ToolsWorkspace};

pub(crate) fn render_reachability_tab(
    workspace: &ToolsWorkspace,
    stack: bool,
    window: &mut Window,
    cx: &mut Context<ToolsWorkspace>,
) -> Div {
    let inputs_disabled =
        workspace.reachability.single.is_running() || workspace.reachability.audit.is_running();

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
        .child(match workspace.reachability.active_tab {
            ReachabilityTab::Single => {
                render_single_form(workspace, window, inputs_disabled, cx).into_any_element()
            }
            ReachabilityTab::Audit => {
                render_audit_form(workspace, inputs_disabled, cx).into_any_element()
            }
        });

    let result = render_reachability_result_panel(workspace, window, cx);

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
            .child(
                div()
                    .w(px(460.0))
                    .min_w(px(360.0))
                    .max_w(px(520.0))
                    .child(form),
            )
            .child(div().flex_1().min_w(px(0.0)).child(result))
    }
}

fn render_single_input_box(
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
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border.alpha(0.7))
                    .bg(cx.theme().group_box)
                    .px_2()
                    .py_1()
                    .child(
                        Input::new(input)
                            .appearance(false)
                            .bordered(false)
                            .disabled(disabled),
                    ),
            ),
    )
}

fn render_single_form(
    workspace: &ToolsWorkspace,
    window: &mut Window,
    inputs_disabled: bool,
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
        .single_timeout_input
        .clone()
        .expect("reach single timeout input should exist");

    v_flex()
        .gap_3()
        .child(render_single_input_box(
            "Target",
            "Hostname, IP literal, or host:port.",
            &target_input,
            render_reachability_prefill_action(workspace, window, inputs_disabled, cx),
            inputs_disabled,
            cx,
        ))
        .child(render_single_input_box(
            "Port Override",
            "Optional when the target already includes :port.",
            &port_input,
            None,
            inputs_disabled,
            cx,
        ))
        .child(render_single_input_box(
            "Timeout (ms)",
            "Per resolve or connect attempt timeout.",
            &timeout_input,
            None,
            inputs_disabled,
            cx,
        ))
        .child(render_reachability_toggles(
            ReachabilityTab::Single,
            &workspace.reachability.single_form,
            inputs_disabled,
            |this, value| this.set_single_reachability_mode(value),
            |this, value| this.set_single_family_preference(value),
            |this, value| this.set_single_stop_on_first_success(value),
            cx,
        ))
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("Resolve checks the current host resolver. TCP Connect checks host-side TCP reachability only. It does not prove WireGuard UDP liveness."),
        )
        .child(
            h_flex()
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
                        .disabled(inputs_disabled)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.run_reachability(cx);
                        })),
                ),
        )
}

fn render_audit_form(
    workspace: &ToolsWorkspace,
    inputs_disabled: bool,
    cx: &mut Context<ToolsWorkspace>,
) -> Div {
    let timeout_input = workspace
        .reachability
        .audit_timeout_input
        .clone()
        .expect("reach audit timeout input should exist");

    v_flex()
        .gap_3()
        .child(render_single_input_box(
            "Timeout (ms)",
            "Per resolve or connect attempt timeout for each saved endpoint.",
            &timeout_input,
            None,
            inputs_disabled,
            cx,
        ))
        .child(render_reachability_toggles(
            ReachabilityTab::Audit,
            &workspace.reachability.audit_form,
            inputs_disabled,
            |this, value| this.set_audit_reachability_mode(value),
            |this, value| this.set_audit_family_preference(value),
            |this, value| this.set_audit_stop_on_first_success(value),
            cx,
        ))
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("Audit runs against every saved config and checks each peer endpoint it can parse."),
        )
        .child(
            h_flex()
                .justify_between()
                .gap_2()
                .flex_wrap()
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("Target, port override, and active-config endpoint prefill only apply to Single Test."),
                )
                .child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(
                            Button::new("tools-reach-audit-run")
                                .label(if workspace.reachability.audit_cancelling {
                                    "Cancelling..."
                                } else if workspace.reachability.audit.is_running() {
                                    "Auditing..."
                                } else {
                                    "Run Audit"
                                })
                                .primary()
                                .small()
                                .disabled(inputs_disabled)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.run_reachability_audit(cx);
                                })),
                        )
                        .child(
                            Button::new("tools-reach-audit-cancel")
                                .label(if workspace.reachability.audit_cancelling {
                                    "Cancelling..."
                                } else {
                                    "Cancel"
                                })
                                .outline()
                                .small()
                                .disabled(
                                    !workspace.reachability.audit.is_running()
                                        || workspace.reachability.audit_cancelling,
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.cancel_reachability_audit(cx);
                                })),
                        ),
                ),
        )
}

fn render_reachability_toggles(
    tab: ReachabilityTab,
    form: &ReachabilityFormState,
    disabled: bool,
    set_mode: impl Fn(&mut ToolsWorkspace, ReachabilityMode) -> bool + 'static + Copy,
    set_family: impl Fn(&mut ToolsWorkspace, AddressFamilyPreference) -> bool + 'static + Copy,
    set_stop_mode: impl Fn(&mut ToolsWorkspace, bool) -> bool + 'static + Copy,
    cx: &mut Context<ToolsWorkspace>,
) -> GroupBox {
    let (
        mode_group_id,
        mode_resolve_id,
        mode_tcp_id,
        family_group_id,
        stop_group_id,
        stop_first_id,
        stop_all_id,
    ) = match tab {
        ReachabilityTab::Single => (
            "tools-single-mode",
            "tools-single-mode-resolve",
            "tools-single-mode-tcp",
            "tools-single-family",
            "tools-single-stop",
            "tools-single-stop-first",
            "tools-single-stop-all",
        ),
        ReachabilityTab::Audit => (
            "tools-audit-mode",
            "tools-audit-mode-resolve",
            "tools-audit-mode-tcp",
            "tools-audit-family",
            "tools-audit-stop",
            "tools-audit-stop-first",
            "tools-audit-stop-all",
        ),
    };
    let mode_group = ButtonGroup::new(mode_group_id)
        .outline()
        .compact()
        .small()
        .child(
            Button::new(mode_resolve_id)
                .label("Resolve")
                .selected(form.mode == ReachabilityMode::ResolveOnly)
                .disabled(disabled)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if set_mode(this, ReachabilityMode::ResolveOnly) {
                        cx.notify();
                    }
                })),
        )
        .child(
            Button::new(mode_tcp_id)
                .label("TCP Connect")
                .selected(form.mode == ReachabilityMode::TcpConnect)
                .disabled(disabled)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if set_mode(this, ReachabilityMode::TcpConnect) {
                        cx.notify();
                    }
                })),
        );

    let family_group = ButtonGroup::new(family_group_id)
        .outline()
        .compact()
        .small()
        .child(family_button(
            tab,
            AddressFamilyPreference::System,
            form,
            disabled,
            set_family,
            cx,
        ))
        .child(family_button(
            tab,
            AddressFamilyPreference::PreferIpv4,
            form,
            disabled,
            set_family,
            cx,
        ))
        .child(family_button(
            tab,
            AddressFamilyPreference::PreferIpv6,
            form,
            disabled,
            set_family,
            cx,
        ));

    let stop_group = ButtonGroup::new(stop_group_id)
        .outline()
        .compact()
        .small()
        .child(
            Button::new(stop_first_id)
                .label("Stop on First Success")
                .selected(form.stop_on_first_success)
                .disabled(disabled)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if set_stop_mode(this, true) {
                        cx.notify();
                    }
                })),
        )
        .child(
            Button::new(stop_all_id)
                .label("Try All Addresses")
                .selected(!form.stop_on_first_success)
                .disabled(disabled)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if set_stop_mode(this, false) {
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
    disabled: bool,
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
                .disabled(disabled)
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
            .disabled(disabled)
            .dropdown_caret(true)
            .dropdown_menu_with_anchor(Anchor::TopRight, {
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

fn family_button(
    tab: ReachabilityTab,
    family: AddressFamilyPreference,
    form: &ReachabilityFormState,
    disabled: bool,
    set_family: impl Fn(&mut ToolsWorkspace, AddressFamilyPreference) -> bool + 'static + Copy,
    cx: &mut Context<ToolsWorkspace>,
) -> Button {
    let (id, label) = match family {
        AddressFamilyPreference::System => (
            match tab {
                ReachabilityTab::Single => "tools-single-family-system",
                ReachabilityTab::Audit => "tools-audit-family-system",
            },
            "System",
        ),
        AddressFamilyPreference::PreferIpv4 => (
            match tab {
                ReachabilityTab::Single => "tools-single-family-v4",
                ReachabilityTab::Audit => "tools-audit-family-v4",
            },
            "Prefer IPv4",
        ),
        AddressFamilyPreference::PreferIpv6 => (
            match tab {
                ReachabilityTab::Single => "tools-single-family-v6",
                ReachabilityTab::Audit => "tools-audit-family-v6",
            },
            "Prefer IPv6",
        ),
    };
    Button::new(id)
        .label(label)
        .selected(form.family_preference == family)
        .disabled(disabled)
        .on_click(cx.listener(move |this, _, _, cx| {
            if set_family(this, family) {
                cx.notify();
            }
        }))
}
