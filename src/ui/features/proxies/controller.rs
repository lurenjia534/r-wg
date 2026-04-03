use gpui::{div, App, Context, Entity, IntoElement, ParentElement, Styled, Window};
use gpui_component::{
    button::{Button, ButtonVariant, ButtonVariants},
    dialog::DialogButtonProps,
    ActiveTheme as _, WindowExt,
};

use crate::ui::state::WgApp;

#[allow(clippy::too_many_arguments)]
pub(crate) fn open_delete_dialog(
    window: &mut Window,
    cx: &mut Context<WgApp>,
    title: impl Into<String>,
    body: impl Into<String>,
    note: Option<String>,
    ids: Vec<u64>,
    skip_running: bool,
    clear_selection: bool,
) {
    let app_handle = cx.entity();
    let title = title.into();
    let body = body.into();
    let note = note.clone();

    window.open_dialog(cx, move |dialog, _window, cx| {
        let app_handle = app_handle.clone();
        let ids = ids.clone();
        let note_skip = skip_running;
        let clear_selection = clear_selection;
        let mut dialog = dialog
            .title(div().text_lg().child(title.clone()))
            .confirm()
            .button_props(
                DialogButtonProps::default()
                    .ok_text("Delete")
                    .ok_variant(ButtonVariant::Danger)
                    .cancel_text("Cancel"),
            )
            .child(div().text_sm().child(body.clone()));

        if let Some(note) = note.clone() {
            dialog = dialog.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(note),
            );
        }

        let delete_action = {
            let app_handle = app_handle.clone();
            let ids = ids.clone();
            move |window: &mut Window, cx: &mut App| {
                perform_delete(&app_handle, &ids, note_skip, clear_selection, window, cx);
            }
        };

        dialog = dialog.footer(move |_ok, _cancel, _window, _cx| {
            let app_handle = app_handle.clone();
            let ids = ids.clone();
            let delete_button = Button::new("proxy-dialog-delete")
                .label("Delete")
                .danger()
                .on_click(move |_, window, cx| {
                    perform_delete(&app_handle, &ids, note_skip, clear_selection, window, cx);
                    window.close_dialog(cx);
                });
            let cancel_button = Button::new("proxy-dialog-cancel")
                .label("Cancel")
                .outline()
                .on_click(|_, window, cx| {
                    window.close_dialog(cx);
                });
            vec![
                cancel_button.into_any_element(),
                delete_button.into_any_element(),
            ]
        });

        dialog = dialog.on_ok(move |_, window, cx| {
            delete_action(window, cx);
            true
        });

        dialog
    });
}

pub(crate) fn perform_delete(
    app_handle: &Entity<WgApp>,
    ids: &[u64],
    skip_running: bool,
    clear_selection: bool,
    window: &mut Window,
    _cx: &mut App,
) {
    let app_handle = app_handle.clone();
    let ids = ids.to_vec();
    let note_skip = skip_running;
    window.on_next_frame(move |window, cx| {
        app_handle.update(cx, |this, cx| {
            this.command_delete_proxy_configs(&ids, note_skip, clear_selection, window, cx);
        });
    });
}
