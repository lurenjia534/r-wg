mod editor;
mod inspector;
mod layout;
mod library;
mod view_data;
mod workspace;

use gpui::{div, px, Context, Div, InteractiveElement, ParentElement, Styled};

use crate::ui::state::WgApp;

pub(crate) use view_data::ConfigsViewData;
pub(crate) use workspace::{
    ConfigsLayoutMode, ConfigsRuntimeView, CONFIGS_LIBRARY_ROW_HEIGHT,
    CONFIGS_LIBRARY_SCROLL_STATE_ID, CONFIGS_MEDIUM_INSPECTOR_HEIGHT,
};

pub(crate) fn render_configs(app: &mut WgApp, cx: &mut Context<WgApp>) -> Div {
    let workspace = app.ensure_configs_workspace(cx);
    div()
        .flex()
        .flex_1()
        .min_h(px(0.0))
        .key_context("Configs")
        .child(workspace)
}
