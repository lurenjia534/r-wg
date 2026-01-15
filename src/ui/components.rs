use gpui::*;
use gpui_component::theme::Theme;

#[derive(Clone, Copy)]
pub enum ButtonTone {
    Neutral,
    Accent,
    Danger,
}

pub fn action_button(id: &'static str, label: &str, enabled: bool, tone: ButtonTone) -> Stateful<Div> {
    let base = match tone {
        ButtonTone::Neutral => rgb(0x2a3138),
        ButtonTone::Accent => rgb(0x2b6f55),
        ButtonTone::Danger => rgb(0x7a2f2f),
    };

    let mut button = div()
        .w_full()
        .px_3()
        .py_2()
        .rounded_md()
        .text_sm()
        .bg(if enabled { base } else { rgb(0x1d2228) })
        .text_color(if enabled { rgb(0xe6e6e6) } else { rgb(0x6f7882) })
        .child(label.to_string())
        .id(id);

    if enabled {
        button = button.cursor_pointer();
    }

    button
}

pub fn card(theme: &Theme, title: impl Into<String>, body: impl IntoElement) -> Div {
    let title = SharedString::new(title.into());
    div()
        .flex()
        .flex_col()
        .gap_2()
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        .bg(theme.group_box)
        .p_3()
        .child(
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(title),
        )
        .child(body)
}

pub fn info_row(theme: &Theme, label: impl Into<String>, value: impl Into<String>) -> Div {
    let label = SharedString::new(label.into());
    let value = SharedString::new(value.into());
    div()
        .flex()
        .justify_between()
        .gap_3()
        .child(
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(label),
        )
        .child(
            div()
                .text_sm()
                .text_color(theme.foreground)
                .child(value),
        )
}
