use gpui::{IntoElement, ParentElement};
use gpui_component::{
    Disableable as _,
    button::{Button, ButtonVariants},
    description_list::{DescriptionItem, DescriptionText},
    group_box::{GroupBox, GroupBoxVariants},
};

#[derive(Clone, Copy)]
pub enum ButtonTone {
    Neutral,
    Accent,
    Danger,
}

pub fn action_button(id: &'static str, label: &str, enabled: bool, tone: ButtonTone) -> Button {
    let mut button = Button::new(id)
        .label(label.to_string())
        .disabled(!enabled);
    button = match tone {
        ButtonTone::Neutral => button,
        ButtonTone::Accent => button.primary(),
        ButtonTone::Danger => button.danger(),
    };
    button
}

pub fn card(title: impl Into<String>, body: impl IntoElement) -> GroupBox {
    GroupBox::new().fill().title(title.into()).child(body)
}

pub fn info_row(
    label: impl Into<DescriptionText>,
    value: impl Into<DescriptionText>,
) -> DescriptionItem {
    DescriptionItem::new(label).value(value).span(1)
}
