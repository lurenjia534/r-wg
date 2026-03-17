use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonGroup},
    group_box::GroupBox,
    h_flex,
    scroll::ScrollableElement,
    tag::Tag,
    v_flex, ActiveTheme as _, Icon, IconName, Selectable, Sizable as _, StyledExt as _,
};

use super::super::state::WgApp;
use r_wg::dns::{DnsMode, DnsPreset};

/// DNS 页面：模式选择 + 预设 DNS 卡片。
pub(crate) fn render_dns(app: &mut WgApp, cx: &mut Context<WgApp>) -> Div {
    let mode_group = ButtonGroup::new("dns-mode")
        .outline()
        .compact()
        .small()
        .child(
            Button::new("dns-mode-follow")
                .label(DnsMode::FollowConfig.label())
                .selected(app.ui_prefs.dns_mode == DnsMode::FollowConfig)
                .tooltip("Use DNS only from the config file")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.set_dns_mode_pref(DnsMode::FollowConfig, cx);
                })),
        )
        .child(
            Button::new("dns-mode-system")
                .label(DnsMode::UseSystemDns.label())
                .selected(app.ui_prefs.dns_mode == DnsMode::UseSystemDns)
                .tooltip("Use DNS from the system resolver")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.set_dns_mode_pref(DnsMode::UseSystemDns, cx);
                })),
        )
        .child(
            Button::new("dns-mode-auto")
                .label(DnsMode::AutoFillMissingFamilies.label())
                .selected(app.ui_prefs.dns_mode == DnsMode::AutoFillMissingFamilies)
                .tooltip("Only fill missing IPv4/IPv6 DNS families")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.set_dns_mode_pref(DnsMode::AutoFillMissingFamilies, cx);
                })),
        )
        .child(
            Button::new("dns-mode-override")
                .label(DnsMode::OverrideAll.label())
                .selected(app.ui_prefs.dns_mode == DnsMode::OverrideAll)
                .tooltip("Ignore config DNS and force selected provider")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.set_dns_mode_pref(DnsMode::OverrideAll, cx);
                })),
        );

    let mode_section = v_flex()
        .gap_2()
        .child(div().text_sm().font_semibold().child("DNS Mode"))
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child("Choose how DNS values are derived."),
        )
        .child(h_flex().items_center().child(mode_group));

    let mode_hint = match app.ui_prefs.dns_mode {
        DnsMode::FollowConfig => Some("Use DNS settings from the config file."),
        DnsMode::UseSystemDns => Some("Use system default DNS."),
        _ => None,
    };

    let show_cards = matches!(
        app.ui_prefs.dns_mode,
        DnsMode::AutoFillMissingFamilies | DnsMode::OverrideAll
    );

    let mut content = v_flex()
        .gap_3()
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("DNS settings will appear here."),
        )
        .child(mode_section)
        .when_some(mode_hint, |this, hint| {
            this.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(hint),
            )
        });

    if show_cards {
        let cloudflare_cards = v_flex()
            .gap_3()
            .child(dns_section_title("Cloudflare (1.1.1.1)", "Plain / 53"))
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap_3()
                    .child(dns_card(app, cx, DnsPreset::CloudflareStandard))
                    .child(dns_card(app, cx, DnsPreset::CloudflareMalware))
                    .child(dns_card(app, cx, DnsPreset::CloudflareMalwareAdult)),
            );

        let adguard_cards = v_flex()
            .gap_3()
            .child(dns_section_title("AdGuard DNS", "Plain / 53"))
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap_3()
                    .child(dns_card(app, cx, DnsPreset::AdguardDefault))
                    .child(dns_card(app, cx, DnsPreset::AdguardUnfiltered))
                    .child(dns_card(app, cx, DnsPreset::AdguardFamily)),
            );

        content = content.child(cloudflare_cards).child(adguard_cards);
    }

    let group = GroupBox::new().title("DNS").w_full().child(content);
    let scrollable = v_flex()
        .id("dns-scroll")
        .w_full()
        .flex_1()
        .min_h(px(0.0))
        .gap_3()
        .child(group)
        .overflow_y_scrollbar();

    div()
        .flex()
        .flex_col()
        .gap_3()
        .flex_grow()
        .min_h(px(0.0))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h(px(0.0))
                .w_full()
                .overflow_hidden()
                .child(scrollable),
        )
}

fn dns_section_title(title: &'static str, subtitle: &'static str) -> Div {
    h_flex()
        .items_center()
        .gap_2()
        .child(div().text_sm().font_semibold().child(title))
        .child(Tag::secondary().small().child(subtitle))
}

fn dns_card(app: &mut WgApp, cx: &mut Context<WgApp>, preset: DnsPreset) -> Stateful<Div> {
    let info = preset.info();
    let selected = app.ui_prefs.dns_preset == preset;
    let border_color = if selected {
        cx.theme().accent
    } else {
        cx.theme().border
    };
    let background = if selected {
        cx.theme().accent.alpha(0.14)
    } else {
        cx.theme().group_box
    };
    let title_color = if selected {
        cx.theme().accent
    } else {
        cx.theme().foreground
    };

    let mut card = div()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .rounded_lg()
        .border_1()
        .border_color(border_color)
        .bg(background)
        .cursor_pointer()
        .relative()
        .id(dns_preset_id(preset))
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .child(
                    v_flex()
                        .gap_1()
                        .child(div().text_sm().text_color(title_color).child(info.title))
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(info.note),
                        ),
                )
                .child(if selected {
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(Tag::success().small().child("Selected"))
                        .child(
                            Icon::new(IconName::CircleCheck)
                                .size_4()
                                .text_color(cx.theme().accent),
                        )
                        .into_any_element()
                } else {
                    div().into_any_element()
                }),
        )
        .child(dns_address_block("IPv4", info.ipv4, cx))
        .child(dns_address_block("IPv6", info.ipv6, cx));

    if selected {
        card = card.child(
            div()
                .absolute()
                .top(px(8.0))
                .bottom(px(8.0))
                .left(px(0.0))
                .w(px(3.0))
                .rounded_md()
                .bg(cx.theme().accent),
        );
    }

    card.on_click(cx.listener(move |this, _, _, cx| {
        this.set_dns_preset_pref(preset, cx);
    }))
}

fn dns_preset_id(preset: DnsPreset) -> &'static str {
    match preset {
        DnsPreset::CloudflareStandard => "dns-card-cf-standard",
        DnsPreset::CloudflareMalware => "dns-card-cf-malware",
        DnsPreset::CloudflareMalwareAdult => "dns-card-cf-family",
        DnsPreset::AdguardDefault => "dns-card-adg-default",
        DnsPreset::AdguardUnfiltered => "dns-card-adg-unfiltered",
        DnsPreset::AdguardFamily => "dns-card-adg-family",
    }
}

fn dns_address_block(
    label: &'static str,
    addrs: &'static [&'static str],
    cx: &mut Context<WgApp>,
) -> Div {
    let mut list = v_flex().gap_1().child(
        div()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(label),
    );
    for addr in addrs {
        list = list.child(div().text_sm().child(*addr));
    }
    list
}
