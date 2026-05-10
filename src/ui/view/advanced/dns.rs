use gpui::prelude::FluentBuilder as _;
use gpui::{div, Div, Entity, IntoElement, ParentElement, SharedString, Styled};
use gpui_component::button::Button;
use gpui_component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_component::{v_flex, ActiveTheme as _, Disableable as _, Sizable as _};
use r_wg::dns::{DnsMode, DnsPreset};

use crate::ui::state::WgApp;

pub(super) fn render_dns_preset_field(app: Entity<WgApp>, cx: &mut gpui::App) -> Div {
    let (mode, preset) = {
        let app = app.read(cx);
        (app.ui_prefs.dns_mode, app.ui_prefs.dns_preset)
    };
    let active = dns_mode_uses_preset(mode);
    let current_label = dns_preset_label(preset);
    let set_handle = app;

    let button = Button::new("advanced-dns-preset")
        .label(current_label)
        .outline()
        .small()
        .compact()
        .disabled(!active);

    let button =
        if active {
            button
                .dropdown_caret(true)
                .dropdown_menu_with_anchor(gpui::Corner::TopRight, move |menu: PopupMenu, _, _| {
                    dns_preset_options()
                        .iter()
                        .fold(menu, |menu, (value, label)| {
                            let checked = *value == dns_preset_value(preset);
                            menu.item(PopupMenuItem::new(label.clone()).checked(checked).on_click(
                                {
                                    let set_handle = set_handle.clone();
                                    let value = value.clone();
                                    move |_, _, cx| {
                                        let next = dns_preset_from_value(&value);
                                        set_handle.update(cx, |app, cx| {
                                            app.set_dns_preset_pref(next, cx);
                                        });
                                    }
                                },
                            ))
                        })
                })
                .into_any_element()
        } else {
            button.into_any_element()
        };

    v_flex()
        .w_full()
        .gap_1()
        .child(button)
        .when(!active, |this| {
            this.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("Inactive in Follow Config and System DNS modes."),
            )
        })
}

fn dns_mode_uses_preset(mode: DnsMode) -> bool {
    matches!(
        mode,
        DnsMode::AutoFillMissingFamilies | DnsMode::OverrideAll
    )
}

fn dns_preset_label(preset: DnsPreset) -> SharedString {
    dns_preset_options()
        .into_iter()
        .find(|(value, _)| *value == dns_preset_value(preset))
        .map(|(_, label)| label)
        .unwrap_or_else(|| SharedString::from("Preset"))
}

fn shared(value: &'static str) -> SharedString {
    SharedString::new_static(value)
}

pub(super) fn dns_mode_options() -> Vec<(SharedString, SharedString)> {
    vec![
        (
            shared("follow_config"),
            shared(DnsMode::FollowConfig.label()),
        ),
        (shared("system"), shared(DnsMode::UseSystemDns.label())),
        (
            shared("auto_fill"),
            shared(DnsMode::AutoFillMissingFamilies.label()),
        ),
        (shared("override"), shared(DnsMode::OverrideAll.label())),
    ]
}

pub(super) fn dns_mode_value(mode: DnsMode) -> SharedString {
    match mode {
        DnsMode::FollowConfig => shared("follow_config"),
        DnsMode::UseSystemDns => shared("system"),
        DnsMode::AutoFillMissingFamilies => shared("auto_fill"),
        DnsMode::OverrideAll => shared("override"),
    }
}

pub(super) fn dns_mode_from_value(value: &SharedString) -> DnsMode {
    match value.as_ref() {
        "system" => DnsMode::UseSystemDns,
        "auto_fill" => DnsMode::AutoFillMissingFamilies,
        "override" => DnsMode::OverrideAll,
        _ => DnsMode::FollowConfig,
    }
}

fn dns_preset_options() -> Vec<(SharedString, SharedString)> {
    vec![
        (
            shared("cloudflare_standard"),
            shared("Cloudflare: Standard"),
        ),
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
