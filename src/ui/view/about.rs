use std::env::consts::{ARCH, OS};

use gpui::*;
use gpui_component::{
    button::Button,
    description_list::DescriptionList,
    group_box::{GroupBox, GroupBoxVariants},
    h_flex,
    scroll::ScrollableElement,
    tag::Tag,
    v_flex, ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _,
};

use super::super::state::WgApp;
use super::super::themes::AppearancePolicy;
use super::widgets::backend_status_badge;

const BRAND_FONT: &str = "Plus Jakarta Sans";
const CHANGELOG: &str = include_str!("../../../CHANGELOG.md");
const ABOUT_MAX_WIDTH: f32 = 1240.0;
const ABOUT_TWO_COLUMN_BREAKPOINT: f32 = 1120.0;
const ABOUT_READINESS_BREAKPOINT: f32 = 900.0;

/// About page: product identity, release context, and runtime diagnostics.
pub(crate) fn render_about(app: &mut WgApp, window: &mut Window, cx: &mut Context<WgApp>) -> Div {
    let version = env!("CARGO_PKG_VERSION");
    let version_text = format!("v{version}");
    let profile_text = if cfg!(debug_assertions) {
        "Debug"
    } else {
        "Release"
    };
    let platform_text = format!("{OS} / {ARCH}");
    let theme_text = format!(
        "{} · {}",
        appearance_policy_text(
            app.ui_prefs.appearance_policy,
            app.ui_prefs.resolved_theme_mode
        ),
        cx.theme().theme_name()
    );
    let (latest_version, latest_changes) = latest_release_notes();
    let two_column_layout = window.viewport_size().width >= px(ABOUT_TWO_COLUMN_BREAKPOINT);
    let wide_readiness = window.viewport_size().width >= px(ABOUT_READINESS_BREAKPOINT);

    let copy_version_text = format!("r-wg {version_text}");
    let copy_build_text =
        format!("r-wg {version_text} ({profile_text}, {platform_text}, theme: {theme_text})");

    let primary_column = v_flex()
        .gap_3()
        .flex_1()
        .min_w(px(0.0))
        .child(render_capabilities(cx))
        .child(render_whats_new(&latest_version, &latest_changes, cx));
    let side_column = v_flex()
        .gap_3()
        .w(px(336.0))
        .child(render_system_card(app, &theme_text, cx));

    let body = if two_column_layout {
        h_flex()
            .items_start()
            .gap_3()
            .child(primary_column)
            .child(side_column)
            .into_any_element()
    } else {
        v_flex()
            .gap_3()
            .child(primary_column)
            .child(render_system_card(app, &theme_text, cx))
            .into_any_element()
    };

    let content = v_flex()
        .id("about-scroll")
        .w_full()
        .max_w(px(ABOUT_MAX_WIDTH))
        .flex_1()
        .min_h(px(0.0))
        .gap_3()
        .child(render_about_hero(
            app,
            &version_text,
            profile_text,
            &platform_text,
            copy_version_text,
            copy_build_text,
            cx,
        ))
        .child(render_readiness_strip(wide_readiness, cx))
        .child(body);

    let scrollable = div()
        .w_full()
        .flex_1()
        .min_h(px(0.0))
        .overflow_y_scrollbar()
        .child(h_flex().w_full().justify_center().child(content));

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

fn render_about_hero(
    app: &WgApp,
    version_text: &str,
    profile_text: &str,
    platform_text: &str,
    copy_version_text: String,
    copy_build_text: String,
    cx: &mut Context<WgApp>,
) -> impl IntoElement {
    let border = cx.theme().accent.alpha(0.16);
    let tile_border = cx.theme().accent.alpha(0.48);
    let hero_bg = linear_gradient(
        140.0,
        linear_color_stop(cx.theme().background, 0.0),
        linear_color_stop(cx.theme().muted.alpha(0.92), 1.0),
    );

    div()
        .w_full()
        .p_5()
        .rounded(px(20.0))
        .border_1()
        .border_color(border)
        .bg(hero_bg)
        .child(
            h_flex()
                .items_start()
                .justify_between()
                .flex_wrap()
                .gap_5()
                .child(
                    h_flex()
                        .items_start()
                        .gap_4()
                        .child(render_brand_tile(tile_border, cx))
                        .child(
                            v_flex()
                                .gap_3()
                                .max_w(px(560.0))
                                .child(
                                    v_flex()
                                        .gap_1()
                                        .child(
                                            h_flex()
                                                .items_center()
                                                .flex_wrap()
                                                .gap_2()
                                                .child(
                                                    div()
                                                        .text_2xl()
                                                        .font_family(BRAND_FONT)
                                                        .font_semibold()
                                                        .child("r-wg"),
                                                )
                                                .child(
                                                    Tag::secondary()
                                                        .small()
                                                        .rounded_full()
                                                        .child(version_text.to_string()),
                                                )
                                                .child(
                                                    Tag::info()
                                                        .small()
                                                        .rounded_full()
                                                        .child("Desktop WireGuard Client"),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(
                                                    "A native desktop client for managing WireGuard tunnels, routing, DNS, and tunnel observability.",
                                                ),
                                        ),
                                )
                                .child(
                                    h_flex()
                                        .items_center()
                                        .flex_wrap()
                                        .gap_2()
                                        .child(runtime_status_badge(app).into_any_element())
                                        .child(backend_status_badge(&app.ui.backend).into_any_element())
                                ),
                        ),
                )
                .child(
                    v_flex()
                        .gap_3()
                        .min_w(px(240.0))
                        .child(
                            div()
                                .px_4()
                                .py_0()
                                .rounded(px(16.0))
                                .border_1()
                                .border_color(cx.theme().border)
                                .bg(cx.theme().background.alpha(0.72))
                                .child(
                                    v_flex()
                                        .gap_0()
                                        .child(
                                            v_flex()
                                                .gap_2()
                                                .px_0()
                                                .py_3()
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .font_family(BRAND_FONT)
                                                        .font_semibold()
                                                        .text_color(cx.theme().muted_foreground)
                                                        .child("Release Identity"),
                                                )
                                                .child(
                                                    div()
                                                        .text_lg()
                                                        .font_semibold()
                                                        .child(version_text.to_string()),
                                                )
                                                .child(
                                                    h_flex()
                                                        .items_center()
                                                        .flex_wrap()
                                                        .gap_2()
                                                        .child(profile_badge(profile_text))
                                                        .child(
                                                            Tag::secondary()
                                                                .small()
                                                                .rounded_full()
                                                                .child(platform_text.to_string()),
                                                        ),
                                                )
                                                .child(
                                                    div()
                                                        .text_sm()
                                                        .text_color(cx.theme().muted_foreground)
                                                        .child(
                                                            "Keep release identity and the primary copy actions in one place.",
                                                        ),
                                                ),
                                        )
                                        .child(
                                            h_flex()
                                                .items_center()
                                                .flex_wrap()
                                                .gap_2()
                                                .px_0()
                                                .py_3()
                                                .border_t_1()
                                                .border_color(cx.theme().border)
                                                .child(copy_button(
                                                    "about-hero-copy-version",
                                                    "Copy Version",
                                                    copy_version_text,
                                                    cx,
                                                ))
                                                .child(copy_button(
                                                    "about-hero-copy-build",
                                                    "Copy Build Info",
                                                    copy_build_text,
                                                    cx,
                                                )),
                                        ),
                                ),
                        ),
                ),
        )
}

fn render_brand_tile(tile_border: Hsla, cx: &mut Context<WgApp>) -> Div {
    div()
        .size(px(76.0))
        .rounded(px(18.0))
        .border_1()
        .border_color(tile_border)
        .bg(linear_gradient(
            155.0,
            linear_color_stop(cx.theme().accent.alpha(0.48), 0.0),
            linear_color_stop(cx.theme().secondary.alpha(0.92), 1.0),
        ))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .text_2xl()
                .font_family(BRAND_FONT)
                .font_semibold()
                .text_color(cx.theme().foreground)
                .child("R-"),
        )
}

fn render_readiness_strip(wide_layout: bool, cx: &mut Context<WgApp>) -> impl IntoElement {
    let content = if wide_layout {
        h_flex()
            .items_start()
            .gap_0()
            .child(
                readiness_tile(
                    IconName::CircleCheck,
                    "Linux",
                    "Production ready",
                    "Routing, DNS, and privileged backend flows are the primary desktop path.",
                    Tag::success().small().rounded_full().child("Ready"),
                    cx.theme().success,
                    true,
                    cx,
                ),
            )
            .child(
                readiness_tile(
                    IconName::Globe,
                    "Windows",
                    "Supported",
                    "Tunnel control and tray behavior are integrated, with desktop notifications wired in.",
                    Tag::info().small().rounded_full().child("Supported"),
                    cx.theme().info,
                    true,
                    cx,
                ),
            )
            .child(
                readiness_tile(
                    IconName::Frame,
                    "macOS",
                    "Scaffold only",
                    "Platform scaffolding exists, but the runtime path is not yet a first-class desktop target.",
                    Tag::warning().small().rounded_full().child("Scaffold"),
                    cx.theme().warning,
                    false,
                    cx,
                ),
            )
            .into_any_element()
    } else {
        v_flex()
            .gap_0()
            .child(
                readiness_tile(
                    IconName::CircleCheck,
                    "Linux",
                    "Production ready",
                    "Routing, DNS, and privileged backend flows are the primary desktop path.",
                    Tag::success().small().rounded_full().child("Ready"),
                    cx.theme().success,
                    false,
                    cx,
                )
                .border_b_1()
                .border_color(cx.theme().border),
            )
            .child(
                readiness_tile(
                    IconName::Globe,
                    "Windows",
                    "Supported",
                    "Tunnel control and tray behavior are integrated, with desktop notifications wired in.",
                    Tag::info().small().rounded_full().child("Supported"),
                    cx.theme().info,
                    false,
                    cx,
                )
                .border_b_1()
                .border_color(cx.theme().border),
            )
            .child(
                readiness_tile(
                    IconName::Frame,
                    "macOS",
                    "Scaffold only",
                    "Platform scaffolding exists, but the runtime path is not yet a first-class desktop target.",
                    Tag::warning().small().rounded_full().child("Scaffold"),
                    cx.theme().warning,
                    false,
                    cx,
                ),
            )
            .into_any_element()
    };

    div()
        .w_full()
        .rounded(px(18.0))
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().group_box)
        .child(content)
}

#[allow(clippy::too_many_arguments)]
fn readiness_tile(
    icon: IconName,
    label: &'static str,
    title: &'static str,
    detail: &'static str,
    tag: impl IntoElement,
    tone: Hsla,
    border_right: bool,
    cx: &mut Context<WgApp>,
) -> Div {
    let tile = v_flex().gap_2().flex_1().min_w(px(220.0)).p_4();
    let tile = if border_right {
        tile.border_r_1().border_color(cx.theme().border)
    } else {
        tile
    };

    tile.child(
        h_flex()
            .items_center()
            .justify_between()
            .gap_3()
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .size_8()
                            .rounded(px(10.0))
                            .bg(tone.alpha(0.18))
                            .text_color(tone)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(Icon::new(icon).size_5()),
                    )
                    .child(div().text_sm().font_semibold().child(label)),
            )
            .child(tag),
    )
    .child(div().text_sm().font_semibold().child(title))
    .child(
        div()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(detail),
    )
}

fn render_capabilities(cx: &mut Context<WgApp>) -> impl IntoElement {
    GroupBox::new().fill().title("Capabilities").child(
        h_flex()
            .items_start()
            .flex_wrap()
            .gap_3()
            .child(feature_card(
                IconName::File,
                "Config Compatibility",
                "Parse WireGuard configuration files with wg-quick style fields and native persistence flow.",
                cx.theme().info,
                cx,
            ))
            .child(feature_card(
                IconName::CircleCheck,
                "Tunnel Lifecycle",
                "Start and stop tunnels with unified status, tray control, and desktop notification feedback.",
                cx.theme().success,
                cx,
            ))
            .child(feature_card(
                IconName::ChartPie,
                "Observability",
                "Track peer handshake timing, traffic summaries, and runtime diagnostics without leaving the app.",
                cx.theme().chart_3,
                cx,
            )),
    )
}

fn feature_card(
    icon: IconName,
    title: &'static str,
    detail: &'static str,
    tone: Hsla,
    cx: &mut Context<WgApp>,
) -> Div {
    div()
        .flex_1()
        .min_w(px(220.0))
        .p_4()
        .rounded(px(16.0))
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().background.alpha(0.72))
        .child(
            v_flex()
                .gap_3()
                .child(
                    div()
                        .size_9()
                        .rounded(px(12.0))
                        .bg(tone.alpha(0.18))
                        .text_color(tone)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(Icon::new(icon).size_5()),
                )
                .child(
                    v_flex()
                        .gap_1()
                        .child(div().text_sm().font_semibold().child(title))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(detail),
                        ),
                ),
        )
}

fn render_whats_new(
    latest_version: &str,
    latest_changes: &[String],
    cx: &mut Context<WgApp>,
) -> impl IntoElement {
    let latest_header = format!("What's new in v{latest_version}");

    GroupBox::new().fill().title("Release Notes").child(
        v_flex()
            .gap_3()
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(div().text_sm().font_semibold().child(latest_header))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(
                                        "Recent release notes keep About closer to a living desktop release panel than a static info sheet.",
                                    ),
                            ),
                    )
                    .child(
                        Tag::secondary()
                            .small()
                            .rounded_full()
                            .child("CHANGELOG.md"),
                    ),
            )
            .children(latest_changes.iter().map(|item| {
                let item = sanitize_release_note(item);
                h_flex()
                    .items_start()
                    .gap_3()
                    .child(
                        div()
                            .mt(px(5.0))
                            .size(px(8.0))
                            .rounded_full()
                            .bg(cx.theme().accent.alpha(0.9)),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().foreground)
                            .child(item),
                    )
            })),
    )
}

fn render_system_card(app: &WgApp, theme_text: &str, cx: &mut Context<WgApp>) -> impl IntoElement {
    GroupBox::new().fill().title("System").child(
        v_flex()
            .gap_3()
            .child(
                DescriptionList::new()
                    .columns(1)
                    .item(
                        "Runtime",
                        inline_tag(runtime_status_badge(app)).into_any_element(),
                        1,
                    )
                    .item(
                        "Backend",
                        inline_tag(backend_status_badge(&app.ui.backend)).into_any_element(),
                        1,
                    )
                    .item("Theme", theme_text.to_string(), 1),
            )
            .child(
                div()
                    .rounded(px(14.0))
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background.alpha(0.72))
                    .px_3()
                    .py_2()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                h_flex()
                                    .items_center()
                                    .justify_between()
                                    .gap_3()
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_semibold()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("Backend Detail"),
                                    )
                                    .child(
                                        Button::new("about-copy-backend-status")
                                            .label("Copy")
                                            .outline()
                                            .small()
                                            .compact()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                cx.write_to_clipboard(ClipboardItem::new_string(
                                                    this.ui.backend.detail.to_string(),
                                                ));
                                            })),
                                    ),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().foreground)
                                    .child(app.ui.backend.detail.clone()),
                            ),
                    ),
            ),
    )
}

fn copy_button(
    id: &'static str,
    label: &'static str,
    text: String,
    cx: &mut Context<WgApp>,
) -> Button {
    Button::new(id)
        .label(label)
        .outline()
        .small()
        .compact()
        .on_click(cx.listener(move |_, _, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
        }))
}

fn inline_tag(tag: Tag) -> Div {
    h_flex().items_center().justify_start().child(tag)
}

fn runtime_status_badge(app: &WgApp) -> Tag {
    if app.runtime.running {
        Tag::success().small().rounded_full().child("Running")
    } else {
        Tag::secondary().small().rounded_full().child("Idle")
    }
}

fn profile_badge(profile_text: &str) -> Tag {
    if cfg!(debug_assertions) {
        Tag::warning()
            .small()
            .rounded_full()
            .child(profile_text.to_string())
    } else {
        Tag::info()
            .small()
            .rounded_full()
            .child(profile_text.to_string())
    }
}

fn appearance_policy_text(
    policy: AppearancePolicy,
    resolved_mode: gpui_component::theme::ThemeMode,
) -> &'static str {
    match policy {
        AppearancePolicy::System => match resolved_mode {
            gpui_component::theme::ThemeMode::Light => "Follow System (Light)",
            gpui_component::theme::ThemeMode::Dark => "Follow System (Dark)",
        },
        AppearancePolicy::Light => "Light",
        AppearancePolicy::Dark => "Dark",
    }
}

fn latest_release_notes() -> (String, Vec<String>) {
    let mut version = None;
    let mut changes = Vec::new();
    let mut in_section = false;

    for raw_line in CHANGELOG.lines() {
        let line = raw_line.trim();

        if let Some(rest) = line.strip_prefix("## ") {
            if in_section {
                break;
            }
            version = Some(rest.split(" - ").next().unwrap_or(rest).trim().to_string());
            in_section = true;
            continue;
        }

        if !in_section {
            continue;
        }

        if let Some(item) = line.strip_prefix("- ") {
            changes.push(item.to_string());
            if changes.len() == 3 {
                break;
            }
        }
    }

    if let Some(version) = version {
        if !changes.is_empty() {
            return (version, changes);
        }
    }

    (
        env!("CARGO_PKG_VERSION").to_string(),
        vec![
            "Recent release notes are not available.".to_string(),
            "Check CHANGELOG.md for the latest project history.".to_string(),
        ],
    )
}

fn sanitize_release_note(item: &str) -> String {
    item.replace('`', "")
}
