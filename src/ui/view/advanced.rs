use gpui::prelude::FluentBuilder;
use gpui::{div, px, Context, Div, Entity, ParentElement, SharedString, Styled};
use gpui_component::setting::{SettingField, SettingGroup, SettingItem, SettingPage, Settings};
use gpui_component::theme::{Theme, ThemeMode};
use r_wg::dns::{DnsMode, DnsPreset};

use super::super::state::{RightTab, TrafficPeriod, WgApp};

pub(crate) fn render_advanced(_app: &mut WgApp, cx: &mut Context<WgApp>) -> Div {
    let app_handle = cx.entity();

    let general_page = SettingPage::new("General")
        .description("Common preferences for appearance and behavior.")
        .default_open(true)
        .group(
            SettingGroup::new()
                .title("Appearance")
                .description("Customize how the app looks.")
                .item(theme_mode_item(app_handle.clone())),
        )
        .group(
            SettingGroup::new()
                .title("Behavior")
                .description("Adjust how the app behaves during use.")
                .item(log_auto_follow_item(app_handle.clone())),
        );

    let network_page = SettingPage::new("Network")
        .description("Settings applied when starting the tunnel.")
        .default_open(true)
        .group(
            SettingGroup::new()
                .title("DNS")
                .description("Control how DNS values are derived.")
                .item(dns_mode_item(app_handle.clone()))
                .item(dns_preset_item(app_handle.clone())),
        );

    let monitoring_page = SettingPage::new("Monitoring")
        .description("Customize dashboards and logs.")
        .default_open(true)
        .group(
            SettingGroup::new()
                .title("Traffic")
                .description("Set defaults for stats and panels.")
                .item(traffic_period_item(app_handle.clone()))
                .item(right_tab_item(app_handle.clone())),
        );

    let settings = Settings::new("advanced-settings")
        .sidebar_width(px(230.0))
        .page(general_page)
        .page(network_page)
        .page(monitoring_page);

    div()
        .flex()
        .flex_col()
        .flex_1()
        .w_full()
        .min_h(px(0.0))
        .child(settings)
}

fn theme_mode_item(app: Entity<WgApp>) -> SettingItem {
    let options = theme_mode_options();
    let get_handle = app.clone();
    let set_handle = app;

    SettingItem::new(
        "Theme Mode",
        SettingField::dropdown(
            options,
            move |cx| theme_mode_value(get_handle.read(cx).theme_mode),
            move |value, cx| {
                let next = theme_mode_from_value(&value);
                let _ = set_handle.update(cx, |app, cx| {
                    if app.theme_mode != next {
                        app.theme_mode = next;
                        Theme::change(next, None, cx);
                        cx.refresh_windows();
                        app.persist_state_async(cx);
                    }
                    cx.notify();
                });
            },
        ),
    )
    .description("Switch between light and dark themes.")
}

fn log_auto_follow_item(app: Entity<WgApp>) -> SettingItem {
    let get_handle = app.clone();
    let set_handle = app;

    SettingItem::new(
        "Auto Follow Logs",
        SettingField::switch(
            move |cx| get_handle.read(cx).log_auto_follow,
            move |value, cx| {
                let _ = set_handle.update(cx, |app, cx| {
                    app.log_auto_follow = value;
                    cx.notify();
                });
            },
        ),
    )
    .description("Keep logs scrolled to the latest entries.")
}

fn dns_mode_item(app: Entity<WgApp>) -> SettingItem {
    let options = dns_mode_options();
    let get_handle = app.clone();
    let set_handle = app;

    SettingItem::new(
        "DNS Mode",
        SettingField::dropdown(
            options,
            move |cx| dns_mode_value(get_handle.read(cx).dns_mode),
            move |value, cx| {
                let next = dns_mode_from_value(&value);
                let _ = set_handle.update(cx, |app, cx| {
                    app.dns_mode = next;
                    cx.notify();
                });
            },
        ),
    )
    .description("Controls how DNS is derived from configs and presets.")
}

fn dns_preset_item(app: Entity<WgApp>) -> SettingItem {
    let options = dns_preset_options();
    let get_handle = app.clone();
    let set_handle = app;

    SettingItem::new(
        "DNS Preset",
        SettingField::dropdown(
            options,
            move |cx| dns_preset_value(get_handle.read(cx).dns_preset),
            move |value, cx| {
                let next = dns_preset_from_value(&value);
                let _ = set_handle.update(cx, |app, cx| {
                    app.dns_preset = next;
                    cx.notify();
                });
            },
        ),
    )
    .description("Used when DNS mode fills or overrides missing records.")
}

fn traffic_period_item(app: Entity<WgApp>) -> SettingItem {
    let options = traffic_period_options();
    let get_handle = app.clone();
    let set_handle = app;

    SettingItem::new(
        "Default Traffic Period",
        SettingField::dropdown(
            options,
            move |cx| traffic_period_value(get_handle.read(cx).traffic_period),
            move |value, cx| {
                let next = traffic_period_from_value(&value);
                let _ = set_handle.update(cx, |app, cx| {
                    app.traffic_period = next;
                    cx.notify();
                });
            },
        ),
    )
    .description("Sets the default range for traffic charts.")
}

fn right_tab_item(app: Entity<WgApp>) -> SettingItem {
    let options = right_tab_options();
    let get_handle = app.clone();
    let set_handle = app;

    SettingItem::new(
        "Right Panel Default",
        SettingField::dropdown(
            options,
            move |cx| right_tab_value(get_handle.read(cx).right_tab),
            move |value, cx| {
                let next = right_tab_from_value(&value);
                let _ = set_handle.update(cx, |app, cx| {
                    app.right_tab = next;
                    cx.notify();
                });
            },
        ),
    )
    .description("Choose which tab opens in the right panel.")
}

fn shared(value: &'static str) -> SharedString {
    SharedString::new_static(value)
}

fn theme_mode_options() -> Vec<(SharedString, SharedString)> {
    vec![(shared("light"), shared("Light")), (shared("dark"), shared("Dark"))]
}

fn theme_mode_value(mode: ThemeMode) -> SharedString {
    shared(mode.name())
}

fn theme_mode_from_value(value: &SharedString) -> ThemeMode {
    match value.as_ref() {
        "dark" => ThemeMode::Dark,
        _ => ThemeMode::Light,
    }
}

fn dns_mode_options() -> Vec<(SharedString, SharedString)> {
    vec![
        (shared("follow_config"), shared(DnsMode::FollowConfig.label())),
        (shared("system"), shared(DnsMode::UseSystemDns.label())),
        (shared("auto_fill"), shared(DnsMode::AutoFillMissingFamilies.label())),
        (shared("override"), shared(DnsMode::OverrideAll.label())),
    ]
}

fn dns_mode_value(mode: DnsMode) -> SharedString {
    match mode {
        DnsMode::FollowConfig => shared("follow_config"),
        DnsMode::UseSystemDns => shared("system"),
        DnsMode::AutoFillMissingFamilies => shared("auto_fill"),
        DnsMode::OverrideAll => shared("override"),
    }
}

fn dns_mode_from_value(value: &SharedString) -> DnsMode {
    match value.as_ref() {
        "system" => DnsMode::UseSystemDns,
        "auto_fill" => DnsMode::AutoFillMissingFamilies,
        "override" => DnsMode::OverrideAll,
        _ => DnsMode::FollowConfig,
    }
}

fn dns_preset_options() -> Vec<(SharedString, SharedString)> {
    vec![
        (shared("cloudflare_standard"), shared("Cloudflare: Standard")),
        (shared("cloudflare_malware"), shared("Cloudflare: Malware")),
        (
            shared("cloudflare_malware_adult"),
            shared("Cloudflare: Malware + Adult"),
        ),
        (shared("adguard_default"), shared("AdGuard: Default")),
        (shared("adguard_unfiltered"), shared("AdGuard: Unfiltered")),
        (shared("adguard_family"), shared("AdGuard: Family")),
    ]
}

fn dns_preset_value(preset: DnsPreset) -> SharedString {
    match preset {
        DnsPreset::CloudflareStandard => shared("cloudflare_standard"),
        DnsPreset::CloudflareMalware => shared("cloudflare_malware"),
        DnsPreset::CloudflareMalwareAdult => shared("cloudflare_malware_adult"),
        DnsPreset::AdguardDefault => shared("adguard_default"),
        DnsPreset::AdguardUnfiltered => shared("adguard_unfiltered"),
        DnsPreset::AdguardFamily => shared("adguard_family"),
    }
}

fn dns_preset_from_value(value: &SharedString) -> DnsPreset {
    match value.as_ref() {
        "cloudflare_malware" => DnsPreset::CloudflareMalware,
        "cloudflare_malware_adult" => DnsPreset::CloudflareMalwareAdult,
        "adguard_default" => DnsPreset::AdguardDefault,
        "adguard_unfiltered" => DnsPreset::AdguardUnfiltered,
        "adguard_family" => DnsPreset::AdguardFamily,
        _ => DnsPreset::CloudflareStandard,
    }
}

fn traffic_period_options() -> Vec<(SharedString, SharedString)> {
    vec![
        (shared("today"), shared("Today")),
        (shared("this_month"), shared("This Month")),
        (shared("last_month"), shared("Last Month")),
    ]
}

fn traffic_period_value(period: TrafficPeriod) -> SharedString {
    match period {
        TrafficPeriod::Today => shared("today"),
        TrafficPeriod::ThisMonth => shared("this_month"),
        TrafficPeriod::LastMonth => shared("last_month"),
    }
}

fn traffic_period_from_value(value: &SharedString) -> TrafficPeriod {
    match value.as_ref() {
        "this_month" => TrafficPeriod::ThisMonth,
        "last_month" => TrafficPeriod::LastMonth,
        _ => TrafficPeriod::Today,
    }
}

fn right_tab_options() -> Vec<(SharedString, SharedString)> {
    vec![(shared("status"), shared("Status")), (shared("logs"), shared("Logs"))]
}

fn right_tab_value(tab: RightTab) -> SharedString {
    match tab {
        RightTab::Status => shared("status"),
        RightTab::Logs => shared("logs"),
    }
}

fn right_tab_from_value(value: &SharedString) -> RightTab {
    match value.as_ref() {
        "logs" => RightTab::Logs,
        _ => RightTab::Status,
    }
}
