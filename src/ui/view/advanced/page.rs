use gpui::{div, px, Context, Div, ParentElement, Styled};
use gpui_component::group_box::GroupBoxVariant;
use gpui_component::setting::{SettingGroup, SettingPage, Settings};
use gpui_component::ActiveTheme as _;

use crate::ui::features::theme_settings_group;
use crate::ui::state::WgApp;
use crate::ui::view::widgets::{PageShell, PageShellHeader};

use super::preferences::{
    connect_password_item, daita_mode_item, daita_resources_item, dns_mode_item,
    dns_preset_item, inspector_tab_item, log_auto_follow_item, quantum_mode_item,
    traffic_period_item,
};
use super::system::{privileged_backend_item, troubleshooting_item};

// Settings page composition.

pub(crate) fn render_advanced(_app: &mut WgApp, cx: &mut Context<WgApp>) -> Div {
    let app_handle = cx.entity();

    let general_page = SettingPage::new("General")
        .description("Appearance and remembered app defaults.")
        .default_open(true)
        .group(theme_settings_group(app_handle.clone()))
        .group(
            SettingGroup::new()
                .title("Workspace")
                .description("Choose which right-side panel opens first in Configs.")
                .item(inspector_tab_item(app_handle.clone())),
        );

    let network_page = SettingPage::new("Network")
        .description("Defaults used when tunnel configs do not fully define DNS behavior.")
        .default_open(true)
        .group(
            SettingGroup::new()
                .title("DNS")
                .description("Keep DNS handling predictable across imported configs.")
                .item(dns_mode_item(app_handle.clone()))
                .item(dns_preset_item(app_handle.clone())),
        )
        .group(
            SettingGroup::new()
                .title("Connection Security")
                .description(
                    "Require local approval and control upcoming tunnel hardening behavior.",
                )
                .item(connect_password_item(app_handle.clone()))
                .item(quantum_mode_item(app_handle.clone()))
                .item(daita_mode_item(app_handle.clone()))
                .item(daita_resources_item(app_handle.clone())),
        );

    let monitoring_page = SettingPage::new("Monitoring")
        .description("Remembered monitoring behavior and chart defaults.")
        .default_open(true)
        .group(
            SettingGroup::new()
                .title("Logs")
                .description("Control how the runtime log viewer behaves.")
                .item(log_auto_follow_item(app_handle.clone())),
        )
        .group(
            SettingGroup::new()
                .title("Traffic")
                .description("Choose the default range for charts and summaries.")
                .item(traffic_period_item(app_handle.clone())),
        );

    let system_page = SettingPage::new("System")
        .description(
            "Manage the helper service required for routes, DNS changes, and tunnel startup.",
        )
        .default_open(true)
        .group(
            SettingGroup::new()
                .title("Privileged Backend")
                .description("Helper service status, diagnostics, and recovery actions.")
                .item(privileged_backend_item(app_handle.clone()))
                .item(troubleshooting_item()),
        );

    let settings = Settings::new("advanced-settings")
        .with_group_variant(GroupBoxVariant::Fill)
        .sidebar_width(px(210.0))
        .sidebar_style(&settings_sidebar_style(cx))
        .page(general_page)
        .page(network_page)
        .page(monitoring_page)
        .page(system_page);

    PageShell::new(
        PageShellHeader::new(
            "SETTINGS",
            "Preferences",
            "Manage appearance, defaults, and system integration in one place.",
        ),
        div()
            .flex_1()
            .min_h(px(0.0))
            .w_full()
            .flex()
            .justify_center()
            .child(div().h_full().w_full().max_w(px(1040.0)).child(settings)),
    )
    .render(cx)
}

fn settings_sidebar_style(cx: &mut Context<WgApp>) -> gpui::StyleRefinement {
    let mut style = div()
        .bg(cx.theme().sidebar.alpha(0.72))
        .border_color(cx.theme().sidebar_border.alpha(0.28));
    style.style().clone()
}
