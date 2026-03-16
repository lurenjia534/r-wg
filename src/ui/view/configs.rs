use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariants},
    group_box::{GroupBox, GroupBoxVariants},
    h_flex,
    input::{Input, InputState},
    v_flex, ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _,
};

use super::super::state::WgApp;
use super::data::ViewData;
use super::widgets::status_badge;

/// Configs 页面：显示隧道名与配置内容输入框。
pub(crate) fn render_configs_editor(
    app: &mut WgApp,
    data: &ViewData,
    name_input: &Entity<InputState>,
    config_input: &Entity<InputState>,
    cx: &mut Context<WgApp>,
) -> Div {
    let action_bar = config_action_bar(app, cx);
    div()
        .flex()
        .flex_col()
        .gap_3()
        .flex_grow()
        .p_3()
        .rounded_lg()
        .bg(cx.theme().tiles)
        .border_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(div().text_xl().child("Configuration"))
                        .child(status_badge(data.config_status.as_ref())),
                )
                .child(action_bar),
        )
        .child(
            GroupBox::new().fill().title("Tunnel Name").child(
                div()
                    .w_full()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(cx.theme().secondary)
                    .child(Input::new(name_input).appearance(false).bordered(false)),
            ),
        )
        .child(
            GroupBox::new()
                .fill()
                .title("Config")
                .child(
                    div()
                        .w_full()
                        .flex_grow()
                        .min_h(px(320.0))
                        .p_2()
                        .rounded_md()
                        .bg(cx.theme().secondary)
                        .child(
                            Input::new(config_input)
                                .appearance(false)
                                .bordered(false)
                                .h_full(),
                        ),
                )
                .flex_grow(),
        )
}

fn config_action_bar(app: &WgApp, cx: &mut Context<WgApp>) -> Div {
    let busy = app.runtime.busy;
    let has_selection = app.selection.selected_id.is_some();
    let primary_actions = h_flex()
        .gap_2()
        .child(
            Button::new("cfg-import")
                .icon(Icon::new(IconName::FolderOpen).size_3())
                .label("Import File")
                .outline()
                .small()
                .compact()
                .disabled(busy)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.handle_import_click(window, cx);
                })),
        )
        .child(
            Button::new("cfg-paste")
                .icon(Icon::new(IconName::Plus).size_3())
                .label("Paste Config")
                .outline()
                .small()
                .compact()
                .disabled(busy)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.handle_paste_click(window, cx);
                })),
        )
        .child(
            Button::new("cfg-save")
                .icon(Icon::new(IconName::Check).size_3())
                .label("Save Changes")
                .success()
                .small()
                .compact()
                .disabled(busy)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.handle_save_click(window, cx);
                })),
        );

    let secondary_actions = h_flex()
        .gap_2()
        .child(
            Button::new("cfg-rename")
                .icon(Icon::new(IconName::Replace).size_3())
                .label("Rename")
                .outline()
                .small()
                .compact()
                .disabled(busy || !has_selection)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.handle_rename_click(window, cx);
                })),
        )
        .child(
            Button::new("cfg-delete")
                .icon(Icon::new(IconName::Delete).size_3())
                .label("Delete")
                .danger()
                .small()
                .compact()
                .disabled(busy || !has_selection)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.handle_delete_click(window, cx);
                })),
        )
        .child(
            Button::new("cfg-export")
                .icon(Icon::new(IconName::ExternalLink).size_3())
                .label("Export")
                .outline()
                .small()
                .compact()
                .disabled(busy || !has_selection)
                .on_click(cx.listener(|this, _, _window, cx| {
                    this.handle_export_click(cx);
                })),
        )
        .child(
            Button::new("cfg-copy")
                .icon(Icon::new(IconName::Copy).size_3())
                .label("Copy Config")
                .outline()
                .small()
                .compact()
                .disabled(busy || !has_selection)
                .on_click(cx.listener(|this, _, _window, cx| {
                    this.handle_copy_click(cx);
                })),
        );

    v_flex()
        .gap_2()
        .child(primary_actions)
        .child(secondary_actions)
}
