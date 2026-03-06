use std::env::consts::{ARCH, OS};

use gpui::*;
use gpui_component::{
    alert::Alert,
    avatar::Avatar,
    button::Button,
    description_list::DescriptionList,
    group_box::{GroupBox, GroupBoxVariants},
    h_flex,
    scroll::ScrollableElement,
    tag::Tag,
    v_flex, ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _,
};

use super::super::state::WgApp;

/// About page: product summary, build info, and platform support notes.
pub(crate) fn render_about(app: &mut WgApp, cx: &mut Context<WgApp>) -> Div {
    let version = env!("CARGO_PKG_VERSION");
    let version_text = format!("v{version}");
    let profile_text = if cfg!(debug_assertions) {
        "Debug"
    } else {
        "Release"
    };
    let platform_text = format!("{OS} / {ARCH}");

    let runtime_tag = if app.runtime.running {
        Tag::success().small().rounded_full().child("Running")
    } else {
        Tag::secondary().small().rounded_full().child("Idle")
    };

    let profile_tag = if cfg!(debug_assertions) {
        Tag::warning().small().rounded_full().child(profile_text)
    } else {
        Tag::success().small().rounded_full().child(profile_text)
    };

    let header = h_flex()
        .items_center()
        .gap_3()
        .child(Avatar::new().name("r-wg").large())
        .child(
            v_flex()
                .gap_1()
                .child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(div().text_lg().font_semibold().child("r-wg"))
                        .child(
                            Tag::secondary()
                                .small()
                                .rounded_full()
                                .child(version_text.clone()),
                        )
                        .child(Tag::info().small().rounded_full().child("WireGuard Client")),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("Rust-based WireGuard client with a GPUI front end."),
                ),
        );

    let summary = GroupBox::new().fill().title("About").child(
        v_flex().gap_3().child(header).child(
            Alert::info(
                "about-status",
                "Linux networking is production-ready. Windows is supported. macOS is scaffolding only.",
            )
            .text_xs(),
        ),
    );

    let build_info = GroupBox::new().fill().title("Build & Runtime").child(
        DescriptionList::new()
            .columns(2)
            .item("Version", version_text.clone(), 1)
            .item("Profile", profile_tag.into_any_element(), 1)
            .item("Platform", platform_text.clone(), 1)
            .item("Runtime", runtime_tag.into_any_element(), 1)
            .item("Backend", "gotatun", 1)
            .item("UI", "gpui + gpui-component", 1),
    );

    let features = GroupBox::new().fill().title("Capabilities").child(
        v_flex()
            .gap_2()
            .child(feature_row(
                IconName::File,
                "Parse WireGuard configs with wg-quick fields.".into(),
                cx,
            ))
            .child(feature_row(
                IconName::CircleCheck,
                "Start or stop tunnels with live status tags.".into(),
                cx,
            ))
            .child(feature_row(
                IconName::ChartPie,
                "Track peer handshake and traffic summaries.".into(),
                cx,
            )),
    );

    let platform_support = GroupBox::new().fill().title("Platform Support").child(
        v_flex()
            .gap_2()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("Use elevated capabilities on Linux for netlink routing/DNS."),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(Tag::success().small().rounded_full().child("Linux ready"))
                    .child(
                        Tag::warning()
                            .small()
                            .rounded_full()
                            .child("macOS scaffold"),
                    )
                    .child(Tag::success().small().rounded_full().child("Windows ready")),
            ),
    );

    let stack = GroupBox::new().fill().title("Stack").child(
        h_flex()
            .items_center()
            .gap_2()
            .child(Tag::info().small().rounded_full().child("gpui"))
            .child(Tag::info().small().rounded_full().child("gpui-component"))
            .child(Tag::secondary().small().rounded_full().child("gotatun"))
            .child(Tag::secondary().small().rounded_full().child("tokio")),
    );

    let copy_version_text = format!("r-wg {version_text}");
    let copy_build_text = format!("r-wg {version_text} ({profile_text}, {platform_text})");

    let actions = GroupBox::new().fill().title("Actions").child(
        h_flex()
            .items_center()
            .gap_2()
            .child(
                Button::new("about-copy-version")
                    .label("Copy Version")
                    .outline()
                    .small()
                    .compact()
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(copy_version_text.clone()));
                    })),
            )
            .child(
                Button::new("about-copy-build")
                    .label("Copy Build Info")
                    .outline()
                    .small()
                    .compact()
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(copy_build_text.clone()));
                    })),
            ),
    );

    let scrollable = v_flex()
        .id("about-scroll")
        .w_full()
        .flex_1()
        .min_h(px(0.0))
        .gap_3()
        .child(summary)
        .child(build_info)
        .child(features)
        .child(platform_support)
        .child(stack)
        .child(actions)
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

fn feature_row(icon: IconName, text: SharedString, cx: &mut Context<WgApp>) -> Div {
    h_flex()
        .items_center()
        .gap_2()
        .child(Icon::new(icon).size_4().text_color(cx.theme().accent))
        .child(div().text_sm().child(text))
}
