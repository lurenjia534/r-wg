use gpui::{
    div, Context, Div, Entity, FontWeight, InteractiveElement, ParentElement, Stateful,
    StatefulInteractiveElement, Styled,
};
use gpui_component::{h_flex, ActiveTheme as _};

use crate::ui::features::configs::state::ConfigsWorkspace;
use crate::ui::state::ConfigsPrimaryPane;

use super::ConfigsViewData;

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
