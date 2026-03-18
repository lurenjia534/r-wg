use gpui::*;
use gpui_component::{
    button::Button,
    group_box::{GroupBox, GroupBoxVariants},
    h_flex,
    input::{Input, Position},
    switch::Switch,
    v_flex, ActiveTheme as _, Icon, IconName, Sizable as _, Size,
};
use r_wg::log;

use super::super::state::WgApp;

/// 日志页：展示完整日志并提供复制入口。
pub(crate) fn render_logs(
    app: &mut WgApp,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) -> impl IntoElement {
    app.ensure_log_input(window, cx);
    let log_input = app
        .ui
        .log_input
        .clone()
        .expect("log input should be initialized");
    let latest_lines = log::snapshot();
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

    if app.ui_prefs.log_auto_follow && cursor_at_end && current_text != latest_text {
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
                .label("Copy All")
                .outline()
                .small()
                .compact()
                .on_click(cx.listener(|this, _, window, cx| {
                    let text = this
                        .ui
                        .log_input
                        .as_ref()
                        .map(|input| input.read(cx).value().to_string())
                        .unwrap_or_default();
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                    this.push_success_toast("Logs copied", window, cx);
                })),
        )
        .child(
            Button::new("logs-clear")
                .label("Clear")
                .outline()
                .small()
                .compact()
                .on_click(cx.listener(|this, _, window, cx| {
                    log::clear();
                    if let Some(log_input) = this.ui.log_input.clone() {
                        log_input.update(cx, |input, cx| {
                            input.set_value("", window, cx);
                        });
                    }
                    this.set_status("Logs cleared");
                    cx.notify();
                })),
        );

    let auto_follow = Switch::new("logs-auto-follow")
        .label("Auto Follow (Lock Selection)")
        .checked(app.ui_prefs.log_auto_follow)
        .with_size(Size::Small)
        .on_click({
            let app_handle = cx.entity();
            let log_input = log_input.clone();
            move |checked: &bool, window, cx| {
                let _ = app_handle.update(cx, |app, cx| {
                    app.set_log_auto_follow_pref(*checked, cx);
                    if *checked {
                        let latest_lines = log::snapshot();
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
                .child(format!("{line_count} lines")),
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
                        .child("Logs"),
                )
                .child(
                    v_flex()
                        .gap_3()
                        .flex_grow()
                        .child(header)
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
