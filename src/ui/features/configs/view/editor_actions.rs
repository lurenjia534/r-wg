use gpui::prelude::FluentBuilder as _;
use gpui::{div, px, Context, Corner, Div, Entity, IntoElement, ParentElement, Styled};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex,
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
    tag::Tag,
    v_flex, ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, StyledExt as _,
};

use crate::ui::features::configs::state::{ConfigsWorkspace, DraftValidationState};
use crate::ui::i18n::{tr, Language};
use crate::ui::state::WgApp;

use super::ConfigsViewData;

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
        .justify_between()
        .gap_3()
        .px(px(12.0))
        .py(px(9.0))
        .rounded_md()
        .border_1()
        .border_color(tone_border)
        .bg(tone_bg)
        .child(
            h_flex()
                .items_start()
                .gap_2()
                .flex_1()
                .child(
                    div()
                        .mt(px(1.0))
                        .child(Icon::new(icon).size_4().text_color(tone_bar)),
                )
                .child(
                    v_flex()
                        .gap_0p5()
                        .child(div().text_xs().font_semibold().child(title))
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
        )
}

pub(super) fn editor_action_bar(
    data: &ConfigsViewData,
    app_handle: &Entity<WgApp>,
    language: Language,
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
                menu.item(PopupMenuItem::new(tr(language, "Rename")).on_click({
                    move |_, window, cx| {
                        rename_handle.update(cx, |this, cx| {
                            this.command_rename_config(window, cx);
                        });
                    }
                }))
                .item(PopupMenuItem::new(tr(language, "Export")).on_click({
                    move |_, _, cx| {
                        export_handle.update(cx, |this, cx| {
                            this.command_export_config(cx);
                        });
                    }
                }))
                .item(PopupMenuItem::new(tr(language, "Copy")).on_click({
                    move |_, _, cx| {
                        copy_handle.update(cx, |this, cx| {
                            this.command_copy_config(cx);
                        });
                    }
                }))
                .item(PopupMenuItem::separator())
                .item(PopupMenuItem::new(tr(language, "Delete")).on_click({
                    move |_, window, cx| {
                        delete_handle.update(cx, |this, cx| {
                            this.command_delete_config(window, cx);
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
                .label(tr(language, "Save"))
                .primary()
                .small()
                .compact()
                .disabled(!data.can_save)
                .on_click({
                    let app = app_handle.clone();
                    move |_, window, cx| {
                        app.update(cx, |this, cx| {
                            this.command_save_config(window, cx);
                        });
                    }
                }),
        )
        .child(
            Button::new("cfg-save-as")
                .icon(Icon::new(IconName::Copy).size_3())
                .label(tr(language, "Save as new"))
                .outline()
                .small()
                .compact()
                .disabled(data.is_busy)
                .on_click({
                    let app = app_handle.clone();
                    move |_, window, cx| {
                        app.update(cx, |this, cx| {
                            this.command_save_config_as_new(window, cx);
                        });
                    }
                }),
        )
        .when(data.can_restart, |this| {
            this.child(
                Button::new("cfg-save-restart")
                    .icon(Icon::new(IconName::Redo2).size_3())
                    .label(tr(language, "Save & Restart"))
                    .outline()
                    .small()
                    .compact()
                    .on_click({
                        let app = app_handle.clone();
                        move |_, window, cx| {
                            app.update(cx, |this, cx| {
                                this.command_save_and_restart_config(window, cx);
                            });
                        }
                    }),
            )
        })
        .child(manage_button)
}
