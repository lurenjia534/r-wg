use gpui::*;

use super::data::ViewData;
use super::super::components::{action_button, ButtonTone};
use super::super::state::{TunnelConfig, WgApp};

/// 左侧面板：隧道列表 + 主操作按钮区。
pub(crate) fn render_left_panel(
    app: &mut WgApp,
    data: &ViewData,
    cx: &mut Context<WgApp>,
) -> Div {
    // 构建隧道列表行，标记选中与运行状态。
    let list_items = app
        .configs
        .iter()
        .enumerate()
        .map(|(idx, config)| {
            tunnel_row(
                idx,
                config,
                app.selected == Some(idx),
                app.running_name.as_deref() == Some(config.name.as_str()),
                cx,
            )
        })
        .collect::<Vec<_>>();

    let list_block = if list_items.is_empty() {
        div()
            .w_full()
            .text_sm()
            .text_color(rgb(0x8a939c))
            .child("No tunnels yet")
            .into_any_element()
    } else {
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_1()
            .children(list_items)
            .into_any_element()
    };

    // 控制按钮状态，避免并发操作或无选中时执行。
    let can_start = !app.busy && app.selected.is_some() && !app.running;
    let can_stop = !app.busy && app.running;
    let can_import = !app.busy;
    let can_save = !app.busy;
    let can_manage_selected = !app.busy && app.selected.is_some();

    // 导入/粘贴/保存等按钮仅做事件绑定，具体逻辑在 actions 中。
    let mut import_button =
        action_button("import-button", "Import File", can_import, ButtonTone::Neutral);
    if can_import {
        import_button = import_button.on_click(cx.listener(|this, _event, window, cx| {
            this.handle_import_click(window, cx);
        }));
    }

    let mut paste_button =
        action_button("paste-button", "Paste Config", can_import, ButtonTone::Neutral);
    if can_import {
        paste_button = paste_button.on_click(cx.listener(|this, _event, window, cx| {
            this.handle_paste_click(window, cx);
        }));
    }

    let mut save_button =
        action_button("save-button", "Save Changes", can_save, ButtonTone::Neutral);
    if can_save {
        save_button = save_button.on_click(cx.listener(|this, _event, window, cx| {
            this.handle_save_click(window, cx);
        }));
    }

    let mut rename_button =
        action_button("rename-button", "Rename", can_manage_selected, ButtonTone::Neutral);
    if can_manage_selected {
        rename_button = rename_button.on_click(cx.listener(|this, _event, window, cx| {
            this.handle_rename_click(window, cx);
        }));
    }

    let mut delete_button =
        action_button("delete-button", "Delete", can_manage_selected, ButtonTone::Danger);
    if can_manage_selected {
        delete_button = delete_button.on_click(cx.listener(|this, _event, window, cx| {
            this.handle_delete_click(window, cx);
        }));
    }

    let mut export_button =
        action_button("export-button", "Export", can_manage_selected, ButtonTone::Neutral);
    if can_manage_selected {
        export_button = export_button.on_click(cx.listener(|this, _event, _window, cx| {
            this.handle_export_click(cx);
        }));
    }

    let mut copy_button =
        action_button("copy-button", "Copy Config", can_manage_selected, ButtonTone::Neutral);
    if can_manage_selected {
        copy_button = copy_button.on_click(cx.listener(|this, _event, _window, cx| {
            this.handle_copy_click(cx);
        }));
    }

    let start_label = if app.running { "Stop" } else { "Start" };
    let start_tone = if app.running {
        ButtonTone::Danger
    } else {
        ButtonTone::Accent
    };
    let start_enabled = if app.running { can_stop } else { can_start };

    let mut start_button = action_button("start-button", start_label, start_enabled, start_tone);
    if start_enabled {
        start_button = start_button.on_click(cx.listener(|this, _event, window, cx| {
            this.handle_start_stop(window, cx);
        }));
    }

    div()
        .w(px(280.0))
        .h_full()
        .flex()
        .flex_col()
        .gap_3()
        .p_3()
        .rounded_lg()
        .bg(rgb(0x141b22))
        .border_1()
        .border_color(rgb(0x202a33))
        .child(div().text_lg().child("Tunnels"))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .flex_grow()
                .id("tunnel-list-scroll")
                .overflow_y_scroll()
                .child(list_block),
        )
        .child(
            div()
                .text_sm()
                .text_color(rgb(0x8a939c))
                .child(data.running_label.clone()),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(start_button)
                .child(import_button)
                .child(paste_button)
                .child(save_button)
                .child(rename_button)
                .child(delete_button)
                .child(export_button)
                .child(copy_button),
        )
}

fn tunnel_row(
    idx: usize,
    config: &TunnelConfig,
    selected: bool,
    is_running: bool,
    cx: &mut Context<WgApp>,
) -> Stateful<Div> {
    // 每一行只负责展示与点击切换选中。
    let label = if is_running {
        format!("● {}", config.label())
    } else {
        config.label()
    };
    let mut row = div()
        .w_full()
        .px_2()
        .py_1()
        .rounded_md()
        .text_sm()
        .cursor_pointer()
        .child(label)
        .bg(if selected {
            rgb(0x2d3640)
        } else if is_running {
            rgb(0x1f3a30)
        } else {
            rgb(0x1a2026)
        })
        .id(idx);

    row = row.on_click(cx.listener(move |this, _event, window, cx| {
        this.select_tunnel(idx, window, cx);
    }));

    row
}
