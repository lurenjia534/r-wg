use gpui::prelude::FluentBuilder as _;
use gpui::{StatefulInteractiveElement as _, *};
use gpui_component::{
    button::Button, h_flex, scroll::Scrollbar, switch::Switch, v_flex, ActiveTheme as _,
    Disableable as _, Icon, IconName, Sizable as _, Size,
};
use r_wg::log::events::ui as log_ui;

use super::super::state::WgApp;

const LOGS_SCROLL_STATE_ID: &str = "logs-scroll";

/// 日志页：展示完整日志并提供复制入口。
pub(crate) fn render_logs(
    app: &mut WgApp,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) -> impl IntoElement {
    let log_viewer_enabled = app.ui_prefs.log_viewer_enabled;
    if log_viewer_enabled {
        app.ensure_backend_log_polling(cx);
    } else {
        app.stop_backend_log_polling();
    }
    let language = app.language();
    let latest_lines = app.merged_log_lines();
    let has_logs = !latest_lines.is_empty();
    let line_count = latest_lines.len();
    let scroll_handle = window
        .use_keyed_state(LOGS_SCROLL_STATE_ID, cx, |_, _| ScrollHandle::new())
        .read(cx)
        .clone();
    if log_viewer_enabled && app.ui_prefs.log_auto_follow {
        scroll_handle.scroll_to_bottom();
    }

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
                    let lines = this.merged_log_lines();
                    let line_count = lines.len();
                    let text = lines.join("\n");
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
            let scroll_handle = scroll_handle.clone();
            move |checked: &bool, _window, cx| {
                app_handle.update(cx, |app, cx| {
                    app.set_log_auto_follow_pref(*checked, cx);
                    if *checked && app.ui_prefs.log_viewer_enabled {
                        scroll_handle.scroll_to_bottom();
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

    let log_rows = latest_lines.into_iter().fold(
        v_flex().gap_1().p_3().min_w_full().flex_shrink_0(),
        |this, line| {
            this.child(
                div()
                    .min_w_full()
                    .flex_shrink_0()
                    .text_xs()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_color(cx.theme().foreground)
                    .whitespace_nowrap()
                    .child(line),
            )
        },
    );

    let log_editor = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .rounded_lg()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().secondary)
        .overflow_hidden()
        .relative()
        .child(
            div()
                .id("logs-scroll-area")
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .size_full()
                .overflow_scroll()
                .track_scroll(&scroll_handle)
                .when(!has_logs, |this| {
                    this.child(
                        div()
                            .p_6()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child("No logs captured"),
                    )
                })
                .when(has_logs, |this| this.child(log_rows)),
        )
        .child(Scrollbar::new(&scroll_handle));

    div()
        .flex()
        .flex_col()
        .gap_3()
        .flex_1()
        .h_full()
        .min_h_0()
        .w_full()
        .child(
            h_flex()
                .items_center()
                .gap_2()
                .text_color(cx.theme().muted_foreground)
                .line_height(relative(1.))
                .child(Icon::new(IconName::SquareTerminal).size_4())
                .child(crate::ui::i18n::tr(language, "Logs")),
        )
        .child(
            v_flex()
                .bg(cx.theme().group_box)
                .text_color(cx.theme().group_box_foreground)
                .p_4()
                .gap_4()
                .rounded(cx.theme().radius)
                .flex_1()
                .min_h_0()
                .w_full()
                .child(header)
                .when(!log_viewer_enabled, |this| {
                    this.child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(app.t("Log viewer disabled in Preferences.")),
                    )
                })
                .child(log_editor),
        )
}
