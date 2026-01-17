use gpui::*;
use gpui::prelude::FluentBuilder;
use gpui_component::{
    ActiveTheme as _, Selectable, Sizable as _, StyledExt as _,
    Icon, IconName,
    button::{Button, ButtonGroup, ButtonVariants},
    group_box::{GroupBox, GroupBoxVariants},
    h_flex, scroll::ScrollableElement, tag::Tag, v_flex,
};

use super::super::state::{DnsMode, DnsPreset, WgApp};

/// DNS 页面：模式选择 + 预设 DNS 卡片。
pub(crate) fn render_dns(app: &mut WgApp, cx: &mut Context<WgApp>) -> Div {
    let mode_group = ButtonGroup::new("dns-mode")
        .outline()
        .compact()
        .small()
        .child(
            Button::new("dns-mode-follow")
                .label(DnsMode::FollowConfig.label())
                .selected(app.dns_mode == DnsMode::FollowConfig)
                .tooltip("Use DNS only from the config file")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.dns_mode = DnsMode::FollowConfig;
                    cx.notify();
                })),
        )
        .child(
            Button::new("dns-mode-system")
                .label(DnsMode::UseSystemDns.label())
                .selected(app.dns_mode == DnsMode::UseSystemDns)
                .tooltip("Use DNS from the system resolver")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.dns_mode = DnsMode::UseSystemDns;
                    cx.notify();
                })),
        )
        .child(
            Button::new("dns-mode-auto")
                .label(DnsMode::AutoFillMissingFamilies.label())
                .selected(app.dns_mode == DnsMode::AutoFillMissingFamilies)
                .tooltip("Only fill missing IPv4/IPv6 DNS families")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.dns_mode = DnsMode::AutoFillMissingFamilies;
                    cx.notify();
                })),
        )
        .child(
            Button::new("dns-mode-override")
                .label(DnsMode::OverrideAll.label())
                .selected(app.dns_mode == DnsMode::OverrideAll)
                .tooltip("Ignore config DNS and force selected provider")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.dns_mode = DnsMode::OverrideAll;
                    cx.notify();
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

    let mode_hint = match app.dns_mode {
        DnsMode::FollowConfig => Some("Use DNS settings from the config file."),
        DnsMode::UseSystemDns => Some("Use system default DNS."),
        _ => None,
    };

    let show_cards = matches!(
        app.dns_mode,
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
            .child(dns_section_title(
                "Cloudflare (1.1.1.1)",
                "Plain / 53",
            ))
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap_3()
                    .child(dns_card(
                        app,
                        cx,
                        DnsCardData {
                            preset: DnsPreset::CloudflareStandard,
                            title: "Standard",
                            note: "No filtering",
                            ipv4: &["1.1.1.1", "1.0.0.1"],
                            ipv6: &["2606:4700:4700::1111", "2606:4700:4700::1001"],
                        },
                    ))
                    .child(dns_card(
                        app,
                        cx,
                        DnsCardData {
                            preset: DnsPreset::CloudflareMalware,
                            title: "Malware Blocking",
                            note: "Families - Malware",
                            ipv4: &["1.1.1.2", "1.0.0.2"],
                            ipv6: &["2606:4700:4700::1112", "2606:4700:4700::1002"],
                        },
                    ))
                    .child(dns_card(
                        app,
                        cx,
                        DnsCardData {
                            preset: DnsPreset::CloudflareMalwareAdult,
                            title: "Malware + Adult",
                            note: "Families - Malware + Adult",
                            ipv4: &["1.1.1.3", "1.0.0.3"],
                            ipv6: &["2606:4700:4700::1113", "2606:4700:4700::1003"],
                        },
                    )),
            );

        let adguard_cards = v_flex()
            .gap_3()
            .child(dns_section_title("AdGuard DNS", "Plain / 53"))
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap_3()
                    .child(dns_card(
                        app,
                        cx,
                        DnsCardData {
                            preset: DnsPreset::AdguardDefault,
                            title: "Default",
                            note: "Ads/trackers blocked",
                            ipv4: &["94.140.14.14", "94.140.15.15"],
                            ipv6: &["2a10:50c0::ad1:ff", "2a10:50c0::ad2:ff"],
                        },
                    ))
                    .child(dns_card(
                        app,
                        cx,
                        DnsCardData {
                            preset: DnsPreset::AdguardUnfiltered,
                            title: "Unfiltered",
                            note: "No filtering",
                            ipv4: &["94.140.14.140", "94.140.14.141"],
                            ipv6: &["2a10:50c0::1:ff", "2a10:50c0::2:ff"],
                        },
                    ))
                    .child(dns_card(
                        app,
                        cx,
                        DnsCardData {
                            preset: DnsPreset::AdguardFamily,
                            title: "Family",
                            note: "Ads/trackers/adult blocked",
                            ipv4: &["94.140.14.15", "94.140.15.16"],
                            ipv6: &["2a10:50c0::bad1:ff", "2a10:50c0::bad2:ff"],
                        },
                    )),
            );

        content = content.child(cloudflare_cards).child(adguard_cards);
    }

    let group = GroupBox::new()
        .title("DNS")
        .w_full()
        .child(content);
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

struct DnsCardData {
    preset: DnsPreset,
    title: &'static str,
    note: &'static str,
    ipv4: &'static [&'static str],
    ipv6: &'static [&'static str],
}

fn dns_section_title(title: &'static str, subtitle: &'static str) -> Div {
    h_flex()
        .items_center()
        .gap_2()
        .child(div().text_sm().font_semibold().child(title))
        .child(Tag::secondary().small().child(subtitle))
}

fn dns_card(app: &mut WgApp, cx: &mut Context<WgApp>, data: DnsCardData) -> Stateful<Div> {
    let selected = app.dns_preset == data.preset;
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
        .id(dns_preset_id(data.preset))
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .child(
                    v_flex()
                        .gap_1()
                        .child(div().text_sm().text_color(title_color).child(data.title))
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(data.note),
                        ),
                )
                .child(if selected {
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(
                            Tag::success()
                                .small()
                                .child("Selected"),
                        )
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
        .child(dns_address_block("IPv4", data.ipv4, cx))
        .child(dns_address_block("IPv6", data.ipv6, cx));

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
        this.dns_preset = data.preset;
        cx.notify();
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
    let mut list = v_flex()
        .gap_1()
        .child(
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
