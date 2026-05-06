use gpui::{div, px, Context, Div, ParentElement, Styled};
use gpui_component::group_box::GroupBoxVariant;
use gpui_component::setting::{SettingGroup, SettingPage, Settings};
use gpui_component::ActiveTheme as _;

use crate::ui::features::theme_settings_group;
use crate::ui::i18n::tr;
use crate::ui::state::WgApp;
use crate::ui::view::widgets::{PageShell, PageShellHeader};

#[cfg(target_os = "linux")]
use super::preferences::wireguard_backend_item;
use super::preferences::{
    connect_password_item, daita_mode_item, daita_resources_item, dns_mode_item, dns_preset_item,
    inspector_tab_item, kill_switch_item, language_item, log_auto_follow_item, quantum_mode_item,
    traffic_period_item,
};
use super::system::{privileged_backend_item, troubleshooting_item};

// Settings page composition.

pub(crate) fn render_advanced(app: &mut WgApp, cx: &mut Context<WgApp>) -> Div {
    let app_handle = cx.entity();
    let language = app.language();

    let general_page = SettingPage::new(tr(language, "General"))
        .description(tr(language, "Appearance and remembered app defaults."))
        .default_open(true)
        .group(theme_settings_group(app_handle.clone(), language))
        .group(
            SettingGroup::new()
                .title(tr(language, "Language"))
                .description(tr(
                    language,
                    "Set the UI language. System follows your OS locale when it is available.",
                ))
                .item(language_item(app_handle.clone(), language)),
        )
        .group(
            SettingGroup::new()
                .title(tr(language, "Workspace"))
                .description(tr(
                    language,
                    "Choose which right-side panel opens first in Configs.",
                ))
                .item(inspector_tab_item(app_handle.clone(), language)),
        );

    let connection_security_group = SettingGroup::new()
        .title(tr(language, "Connection Security"))
        .description(tr(
            language,
            "Require local approval and control upcoming tunnel hardening behavior.",
        ))
        .item(kill_switch_item(app_handle.clone(), language))
        .item(connect_password_item(app_handle.clone(), language));
    #[cfg(target_os = "linux")]
    let connection_security_group =
        connection_security_group.item(wireguard_backend_item(app_handle.clone()));
    let connection_security_group = connection_security_group
        .item(quantum_mode_item(app_handle.clone()))
        .item(daita_mode_item(app_handle.clone()))
        .item(daita_resources_item(app_handle.clone()));

    let network_page = SettingPage::new(tr(language, "Network"))
        .description(tr(
            language,
            "Defaults used when tunnel configs do not fully define DNS behavior.",
        ))
        .default_open(true)
        .group(
            SettingGroup::new()
                .title(tr(language, "DNS"))
                .description(tr(
                    language,
                    "Keep DNS handling predictable across imported configs.",
                ))
                .item(dns_mode_item(app_handle.clone(), language))
                .item(dns_preset_item(app_handle.clone(), language)),
        )
        .group(connection_security_group);

    let monitoring_page = SettingPage::new(tr(language, "Monitoring"))
        .description(tr(
            language,
            "Remembered monitoring behavior and chart defaults.",
        ))
        .default_open(true)
        .group(
            SettingGroup::new()
                .title(tr(language, "Logs"))
                .description(tr(language, "Control how the runtime log viewer behaves."))
                .item(log_auto_follow_item(app_handle.clone(), language)),
        )
        .group(
            SettingGroup::new()
                .title(tr(language, "Traffic"))
                .description(tr(
                    language,
                    "Choose the default range for charts and summaries.",
                ))
                .item(traffic_period_item(app_handle.clone(), language)),
        );

    let system_page = SettingPage::new(tr(language, "System"))
        .description(tr(
            language,
            "Manage the helper service required for routes, DNS changes, and tunnel startup.",
        ))
        .default_open(true)
        .group(
            SettingGroup::new()
                .title(tr(language, "Privileged Backend"))
                .description("Helper service status, diagnostics, and recovery actions.")
                .item(privileged_backend_item(app_handle.clone(), language))
                .item(troubleshooting_item(app_handle.clone())),
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
            tr(language, "SETTINGS"),
            tr(language, "Preferences"),
            tr(
                language,
                "Manage appearance, defaults, and system integration in one place.",
            ),
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
