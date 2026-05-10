use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    button::Button,
    group_box::{GroupBox, GroupBoxVariants},
    h_flex,
    input::{Input, Position},
    switch::Switch,
    v_flex, ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, Size,
};
use r_wg::log::events::ui as log_ui;

use super::super::state::WgApp;

/// 日志页：展示完整日志并提供复制入口。
pub(crate) fn render_logs(
    app: &mut WgApp,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) -> impl IntoElement {
    app.ensure_log_input(window, cx);
    let log_viewer_enabled = app.ui_prefs.log_viewer_enabled;
    if log_viewer_enabled {
        app.ensure_backend_log_polling(cx);
    } else {
        app.stop_backend_log_polling();
    }
    let language = app.language();
    let log_input = app
        .ui
        .log_input
        .clone()
        .expect("log input should be initialized");
    let latest_lines = app.merged_log_lines();
    let latest_text = if latest_lines.is_empty() {
        String::new()
    } else {
        latest_lines.join("\n")
    };

    let (current_text, cursor_at_end) = {
        let state = log_input.read(cx);
        let current_text = state.value().to_string();
        let cursor_at_end = state.cursor_position() == log_end_position(&current_text);
        (current_text, cursor_at_end)
    };

    if !log_viewer_enabled && !current_text.is_empty() {
        log_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
    } else if log_viewer_enabled
        && app.ui_prefs.log_auto_follow
        && cursor_at_end
        && current_text != latest_text
    {
        let latest_text = latest_text.clone();
        log_input.update(cx, |input, cx| {
            input.set_value(latest_text.clone(), window, cx);
            if !latest_text.is_empty() {
                input.set_cursor_position(log_end_position(&latest_text), window, cx);
            }
        });
    }

    let display_text = log_input.read(cx).value().to_string();
    let line_count = if display_text.is_empty() {
        0
    } else {
        display_text.lines().count()
    };

    let actions = h_flex()
        .items_center()
        .gap_2()
        .child(
            Button::new("logs-copy")
                .label(app.t("Copy All"))
                .outline()
                .small()
                .compact()
                .disabled(!log_viewer_enabled)
                .on_click(cx.listener(|this, _, window, cx| {
                    let text = this
                        .ui
                        .log_input
                        .as_ref()
                        .map(|input| input.read(cx).value().to_string())
                        .unwrap_or_default();
                    let line_count = if text.is_empty() {
                        0
                    } else {
                        text.lines().count()
                    };
                    log_ui::logs_copied(line_count);
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                    this.push_success_toast(this.t("Logs copied"), window, cx);
                })),
        )
        .child(
            Button::new("logs-clear")
                .label(app.t("Clear"))
                .outline()
                .small()
                .compact()
                .disabled(!log_viewer_enabled)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.clear_all_logs(window, cx);
                    let status = this.t("Logs cleared");
                    this.set_status(status);
                    cx.notify();
                })),
        );

    let auto_follow = Switch::new("logs-auto-follow")
        .label(app.t("Auto Follow (Lock Selection)"))
        .checked(app.ui_prefs.log_auto_follow)
        .with_size(Size::Small)
        .disabled(!log_viewer_enabled)
        .on_click({
            let app_handle = cx.entity();
            let log_input = log_input.clone();
            move |checked: &bool, window, cx| {
                app_handle.update(cx, |app, cx| {
                    app.set_log_auto_follow_pref(*checked, cx);
                    if *checked && app.ui_prefs.log_viewer_enabled {
                        let latest_lines = app.merged_log_lines();
                        let latest_text = if latest_lines.is_empty() {
                            String::new()
                        } else {
                            latest_lines.join("\n")
                        };
                        log_input.update(cx, |input, cx| {
                            input.set_value(latest_text.clone(), window, cx);
                            if !latest_text.is_empty() {
                                input.set_cursor_position(
                                    log_end_position(&latest_text),
                                    window,
                                    cx,
                                );
                            }
                        });
                    }
                });
            }
        });

    let header = h_flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format!(
                    "{line_count} {}",
                    if line_count == 1 {
                        app.t("line")
                    } else {
                        app.t("lines")
                    }
                )),
        )
        .child(
            h_flex()
                .items_center()
                .gap_3()
                .child(auto_follow)
                .child(actions),
        );

    let log_editor = div()
        .flex()
        .flex_col()
        .gap_1()
        .p_3()
        .rounded_lg()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().secondary)
        .min_h(px(0.0))
        .text_xs()
        .font_family(cx.theme().mono_font_family.clone())
        .child(
            Input::new(&log_input)
                .appearance(false)
                .bordered(false)
                .disabled(true)
                .h_full(),
        );

    let content_style = StyleRefinement::default().flex_grow().min_h(px(0.0));

    div()
        .flex()
        .flex_col()
        .gap_3()
        .flex_grow()
        .min_h(px(0.0))
        .child(
            GroupBox::new()
                .fill()
                .flex_grow()
                .content_style(content_style)
                .title(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(Icon::new(IconName::SquareTerminal).size_4())
                        .child(crate::ui::i18n::tr(language, "Logs")),
                )
                .child(
                    v_flex()
                        .gap_3()
                        .flex_grow()
                        .child(header)
                        .when(!log_viewer_enabled, |this| {
                            this.child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(app.t("Log viewer disabled in Preferences.")),
                            )
                        })
                        .child(log_editor.flex_grow().min_h(px(0.0))),
                ),
        )
}

fn log_end_position(text: &str) -> Position {
    if text.is_empty() {
        return Position::new(0, 0);
    }

    let mut lines = text.split('\n');
    let mut line_count = 0usize;
    let mut last_line = "";
    for line in &mut lines {
        last_line = line;
        line_count += 1;
    }

    let line_index = line_count.saturating_sub(1) as u32;
    let column = last_line.encode_utf16().count() as u32;
    Position::new(line_index, column)
}
