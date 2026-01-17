use gpui::*;
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, StyledExt as _,
    button::{Button, ButtonVariants},
    h_flex, scroll::ScrollableElement, tag::Tag, v_flex,
};

use super::super::state::{ConfigSource, TunnelConfig, WgApp};

/// Proxies 页面：配置列表入口，用于快速选择隧道。
pub(crate) fn render_proxies(app: &mut WgApp, cx: &mut Context<WgApp>) -> Div {
    let mut list = div().flex().flex_wrap().gap_2();
    if app.configs.is_empty() {
        list = list.child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("No configs yet"),
        );
    } else {
        for (idx, config) in app.configs.iter().enumerate() {
            list = list.child(config_list_item(app, idx, config, cx));
        }
    }

    div()
        .flex()
        .flex_col()
        .gap_3()
        .flex_grow()
        .w_full()
        .min_h(px(0.0))
        .p_3()
        .rounded_lg()
        .bg(cx.theme().tiles)
        .border_1()
        .border_color(cx.theme().border)
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .child(div().text_lg().child("Tunnels"))
                .child(
                    Button::new("cfg-list-import")
                        .icon(Icon::new(IconName::FolderOpen).size_3())
                        .label("Import")
                        .outline()
                        .xsmall()
                        .disabled(app.busy)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.handle_import_click(window, cx);
                        })),
                ),
        )
        .child(
            list.flex_1()
                .min_h(px(0.0))
                .overflow_y_scrollbar(),
        )
}

fn config_list_item(
    app: &WgApp,
    idx: usize,
    config: &TunnelConfig,
    cx: &mut Context<WgApp>,
) -> Stateful<Div> {
    let is_selected = app.selected == Some(idx);
    let is_running = app.running_name.as_deref() == Some(config.name.as_str());
    let name_color = cx.theme().foreground;
    let bg = if is_selected {
        cx.theme().accent.alpha(0.16)
    } else {
        cx.theme().secondary
    };
    let border_color = if is_selected {
        cx.theme().accent
    } else if cx.theme().is_dark() {
        cx.theme().foreground.alpha(0.12)
    } else {
        cx.theme().border
    };

    let mut badges = h_flex()
        .gap_1()
        .child(Tag::secondary().small().child(config_source_label(&config.source)));
    if is_running {
        badges = badges.child(Tag::success().small().child("Running"));
    }

    let mut item = div()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .rounded_lg()
        .border_1()
        .border_color(border_color)
        .bg(bg)
        .shadow_sm()
        .w(px(240.0))
        .min_h(px(64.0))
        .child(
            div()
                .text_sm()
                .font_semibold()
                .text_color(name_color)
                .child(config.name.clone()),
        );

    item = item.child(badges);

    if !app.busy {
        item = item.cursor_pointer();
    }

    let mut item = item.id(("config-item", idx));
    item.on_click(cx.listener(move |this, _event, window, cx| {
        if this.busy {
            return;
        }
        this.select_tunnel(idx, window, cx);
    }))
}

fn config_source_label(source: &ConfigSource) -> &'static str {
    match source {
        ConfigSource::File(_) => "File",
        ConfigSource::Paste => "Pasted",
    }
}
