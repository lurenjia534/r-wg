use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    h_flex,
    scroll::ScrollableElement as _,
    tab::{Tab, TabBar},
    tag::Tag,
    ActiveTheme as _, Sizable as _,
};

use crate::ui::state::{ActiveConfigParseState, ToolsTab, ToolsWorkspace, WgApp};
use crate::ui::view::{PageShell, PageShellHeader};

use super::{
    cidr_tab::render_cidr_tab, components::active_config_source_tag,
    reachability_tab::render_reachability_tab,
};

const TOOLS_STACK_BREAKPOINT: f32 = 1240.0;

pub(crate) fn render_tools(app: &mut WgApp, window: &mut Window, cx: &mut Context<WgApp>) -> Div {
    let workspace = app.ensure_tools_workspace(window, cx);
    app.refresh_tools_active_config_for_display(cx);
    div().flex().flex_1().min_h(px(0.0)).child(workspace)
}

impl Render for ToolsWorkspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let stack = window.viewport_size().width < px(TOOLS_STACK_BREAKPOINT);
        let body = match self.active_tab {
            ToolsTab::Cidr => render_cidr_tab(self, stack, window, cx).into_any_element(),
            ToolsTab::Reachability => {
                render_reachability_tab(self, stack, window, cx).into_any_element()
            }
        };

        PageShell::new(
            PageShellHeader::new(
                "TOOLS",
                "Network Tools",
                "CIDR set operations and host-side endpoint diagnostics.",
            )
            .actions(render_header_actions(self, cx)),
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h(px(0.0))
                .overflow_y_scrollbar()
                .p_3()
                .child(body),
        )
        .toolbar(render_toolbar(&cx.entity(), self.active_tab, cx))
        .render(cx)
    }
}

fn render_header_actions(workspace: &ToolsWorkspace, cx: &mut Context<ToolsWorkspace>) -> Div {
    let parse_badge = match &workspace.active_config.parse_state {
        ActiveConfigParseState::Loading => Tag::info().small().rounded_full().child("Parsing"),
        ActiveConfigParseState::Invalid(_) => {
            Tag::warning().small().rounded_full().child("Parse Issue")
        }
        ActiveConfigParseState::Ready(_) => Tag::success().small().rounded_full().child("Ready"),
        ActiveConfigParseState::None => Tag::secondary().small().rounded_full().child("Idle"),
    };

    h_flex()
        .items_center()
        .gap_2()
        .flex_wrap()
        .child(active_config_source_tag(workspace.active_config.source))
        .child(parse_badge)
        .child(
            Tag::secondary()
                .small()
                .rounded_full()
                .child(workspace.active_config.source_label.clone()),
        )
        .when_some(
            workspace.active_config.parse_error().cloned(),
            |this, message| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(message),
                )
            },
        )
}

fn render_toolbar(
    workspace: &Entity<ToolsWorkspace>,
    active_tab: ToolsTab,
    cx: &mut Context<ToolsWorkspace>,
) -> Div {
    let workspace_handle = workspace.clone();
    let tabs = TabBar::new("tools-tabs")
        .underline()
        .small()
        .selected_index(match active_tab {
            ToolsTab::Cidr => 0,
            ToolsTab::Reachability => 1,
        })
        .on_click(move |index, _window, app| {
            let next = match *index {
                0 => ToolsTab::Cidr,
                1 => ToolsTab::Reachability,
                _ => return,
            };
            app.update_entity(&workspace_handle, |this, cx| {
                if this.set_active_tab(next) {
                    cx.notify();
                }
            });
        })
        .child(Tab::new().label(ToolsTab::Cidr.label()).small())
        .child(Tab::new().label(ToolsTab::Reachability.label()).small());

    div()
        .px_6()
        .py_2()
        .border_b_1()
        .border_color(cx.theme().border.alpha(0.6))
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .gap_3()
                .flex_wrap()
                .child(tabs)
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(
                        "Tools run locally in the shared host process. No privileged IPC involved.",
                    ),
                ),
        )
}
