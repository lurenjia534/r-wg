use std::env::consts::{ARCH, OS};
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Local};
use gpui::{
    div, prelude::FluentBuilder as _, px, Axis, Context, Div, Entity, Hsla, IntoElement,
    ParentElement, SharedString, Styled, Timer, Window,
};
use gpui_component::theme::{Colorize as _, Theme, ThemeMode};
use gpui_component::{
    button::{Button, ButtonGroup, ButtonVariant, ButtonVariants},
    description_list::DescriptionList,
    dialog::DialogButtonProps,
    group_box::GroupBoxVariant,
    h_flex,
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
    setting::{SettingField, SettingGroup, SettingItem, SettingPage, Settings},
    v_flex, ActiveTheme as _, Disableable as _, Selectable, Sizable as _, StyledExt as _,
    WindowExt,
};
use r_wg::backend::wg::PrivilegedServiceAction;
use r_wg::dns::{DnsMode, DnsPreset};

use super::super::persistence;
use super::super::state::{
    BackendDiagnostic, BackendHealth, ConfigInspectorTab, SidebarItem, TrafficPeriod, WgApp,
};
use super::super::theme_lint::{self, ThemeLintItem, ThemeLintSeverity};
use super::super::themes::{self, AppearancePolicy};
use super::widgets::{backend_status_tag, PageShell, PageShellHeader};

pub(crate) fn render_advanced(_app: &mut WgApp, cx: &mut Context<WgApp>) -> Div {
    let app_handle = cx.entity();

    let general_page = SettingPage::new("General")
        .description("Appearance and remembered app defaults.")
        .default_open(true)
        .group(
            SettingGroup::new()
                .title("Appearance")
                .description(
                    "Separate the appearance policy from the light and dark palettes it resolves to.",
                )
                .item(theme_mode_item(app_handle.clone()))
                .item(theme_palette_item(app_handle.clone(), ThemeMode::Light))
                .item(theme_palette_item(app_handle.clone(), ThemeMode::Dark))
                .item(reset_theme_item(app_handle.clone()))
                .item(theme_file_workflow_item(app_handle.clone()))
                .item(theme_preview_item(app_handle.clone())),
        )
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

fn theme_mode_item(app: Entity<WgApp>) -> SettingItem {
    SettingItem::new(
        "Appearance Policy",
        SettingField::render(move |_, _window, cx| {
            let current = app.read(cx).ui_prefs.appearance_policy;
            let system_handle = app.clone();
            let light_handle = app.clone();
            let dark_handle = app.clone();

            div().child(
                ButtonGroup::new("advanced-theme-mode")
                    .outline()
                    .small()
                    .compact()
                    .child(
                        Button::new("advanced-theme-system")
                            .label("Follow System")
                            .selected(current == AppearancePolicy::System)
                            .on_click(move |_, _, cx| {
                                system_handle.update(cx, |app, cx| {
                                    app.set_appearance_policy_pref(
                                        AppearancePolicy::System,
                                        None,
                                        cx,
                                    );
                                });
                            }),
                    )
                    .child(
                        Button::new("advanced-theme-light")
                            .label("Light")
                            .selected(current == AppearancePolicy::Light)
                            .on_click(move |_, _, cx| {
                                light_handle.update(cx, |app, cx| {
                                    app.set_appearance_policy_pref(
                                        AppearancePolicy::Light,
                                        None,
                                        cx,
                                    );
                                });
                            }),
                    )
                    .child(
                        Button::new("advanced-theme-dark")
                            .label("Dark")
                            .selected(current == AppearancePolicy::Dark)
                            .on_click(move |_, _, cx| {
                                dark_handle.update(cx, |app, cx| {
                                    app.set_appearance_policy_pref(
                                        AppearancePolicy::Dark,
                                        None,
                                        cx,
                                    );
                                });
                            }),
                    ),
            )
        }),
    )
    .description("Choose whether the app follows the OS, or stays pinned to light or dark.")
}

fn theme_palette_item(app: Entity<WgApp>, mode: ThemeMode) -> SettingItem {
    let title = match mode {
        ThemeMode::Light => "Light Palette",
        ThemeMode::Dark => "Dark Palette",
    };
    let description = match mode {
        ThemeMode::Light => "Used whenever the appearance policy resolves to light.",
        ThemeMode::Dark => "Used whenever the appearance policy resolves to dark.",
    };

    SettingItem::new(
        title,
        SettingField::render(move |_, _window, cx| {
            render_theme_palette_field(app.clone(), mode, cx)
        }),
    )
    .description(description)
}

fn reset_theme_item(app: Entity<WgApp>) -> SettingItem {
    SettingItem::new(
        "Reset Palettes",
        SettingField::render(move |_, _window, _cx| {
            div().child(
                Button::new("advanced-theme-reset")
                    .label("Use Default Light and Default Dark")
                    .outline()
                    .small()
                    .compact()
                    .on_click({
                        let app = app.clone();
                        move |_, window, cx| {
                            app.update(cx, |app, cx| {
                                app.reset_theme_prefs(Some(window), cx);
                            });
                        }
                    }),
            )
        }),
    )
    .description("Clear stored palette names and fall back to the registry defaults for each mode.")
}

fn theme_file_workflow_item(app: Entity<WgApp>) -> SettingItem {
    SettingItem::new(
        "Theme Files",
        SettingField::render(move |_, _window, cx| {
            render_theme_file_workflow_field(app.clone(), cx)
        }),
    )
    .layout(Axis::Vertical)
    .description("File-based workflow for importing, templating, and restoring curated themes.")
}

fn theme_preview_item(app: Entity<WgApp>) -> SettingItem {
    SettingItem::new(
        "Preview",
        SettingField::render(move |_, _window, cx| render_theme_preview_field(app.clone(), cx)),
    )
    .layout(Axis::Vertical)
    .description(
        "Preview the current light and dark palettes as real product surfaces, with lint for weak contrast and muddy semantics.",
    )
}

fn render_theme_palette_field(app: Entity<WgApp>, mode: ThemeMode, cx: &mut gpui::App) -> Div {
    let storage = persistence::ensure_storage_dirs().ok();
    let (preferred_key, preferred_name) = {
        let app = app.read(cx);
        (
            app.ui_prefs
                .theme_palette_key(mode)
                .map(|key| key.to_string()),
            app.ui_prefs
                .theme_palette_name(mode)
                .map(|name| name.to_string()),
        )
    };
    let resolved = themes::resolve_theme_preference(
        mode,
        preferred_key.as_deref(),
        preferred_name.as_deref(),
        storage.as_ref(),
        cx,
    );
    let current_label = resolved
        .entry
        .badge_label()
        .map(|badge| format!("{} · {}", resolved.entry.name, badge))
        .unwrap_or_else(|| resolved.entry.name.to_string());
    let selected_key = resolved.entry.key.to_string();
    let button_id = match mode {
        ThemeMode::Light => "advanced-theme-light-palette",
        ThemeMode::Dark => "advanced-theme-dark-palette",
    };
    let set_handle = app;

    div().child(
        Button::new(button_id)
            .label(current_label)
            .outline()
            .small()
            .compact()
            .dropdown_caret(true)
            .dropdown_menu_with_anchor(gpui::Corner::TopRight, move |menu: PopupMenu, _, cx| {
                let available = themes::available_themes(mode, storage.as_ref(), cx);
                let mut builtin = Vec::new();
                let mut recommended = Vec::new();
                let mut more = Vec::new();
                let mut custom = Vec::new();

                for theme in available {
                    match theme.menu_group_label() {
                        "Default" => builtin.push(theme),
                        "Recommended" => recommended.push(theme),
                        "More Themes" => more.push(theme),
                        "Custom" => custom.push(theme),
                        _ => {}
                    }
                }

                let menu = add_theme_menu_section(
                    menu,
                    "Default",
                    builtin,
                    &selected_key,
                    set_handle.clone(),
                    mode,
                );
                let menu = if !recommended.is_empty() {
                    add_theme_menu_section(
                        menu.separator(),
                        "Recommended",
                        recommended,
                        &selected_key,
                        set_handle.clone(),
                        mode,
                    )
                } else {
                    menu
                };
                let menu = if !more.is_empty() {
                    add_theme_menu_section(
                        menu.separator(),
                        "More Themes",
                        more,
                        &selected_key,
                        set_handle.clone(),
                        mode,
                    )
                } else {
                    menu
                };
                let menu = if !custom.is_empty() {
                    add_theme_menu_section(
                        menu.separator(),
                        "Custom",
                        custom,
                        &selected_key,
                        set_handle.clone(),
                        mode,
                    )
                } else {
                    menu
                };

                menu.separator().item(
                    PopupMenuItem::new("Use Default")
                        .checked(selected_key.starts_with("builtin:"))
                        .on_click({
                            let set_handle = set_handle.clone();
                            move |_, window, cx| {
                                set_handle.update(cx, |app, cx| {
                                    app.set_theme_palette_pref(mode, None, Some(window), cx);
                                });
                            }
                        }),
                )
            }),
    )
}

fn render_theme_preview_field(app: Entity<WgApp>, cx: &mut gpui::App) -> Div {
    let storage = persistence::ensure_storage_dirs().ok();
    let (light_key, light_name, dark_key, dark_name, current_mode, show_alternate_preview) = {
        let app = app.read(cx);
        (
            app.ui_prefs
                .theme_palette_key(ThemeMode::Light)
                .map(|key| key.to_string()),
            app.ui_prefs
                .theme_palette_name(ThemeMode::Light)
                .map(|name| name.to_string()),
            app.ui_prefs
                .theme_palette_key(ThemeMode::Dark)
                .map(|key| key.to_string()),
            app.ui_prefs
                .theme_palette_name(ThemeMode::Dark)
                .map(|name| name.to_string()),
            app.ui_prefs.resolved_theme_mode,
            app.ui_session.show_alternate_theme_preview,
        )
    };
    let light_resolved = themes::resolve_theme_preference(
        ThemeMode::Light,
        light_key.as_deref(),
        light_name.as_deref(),
        storage.as_ref(),
        cx,
    );
    let dark_resolved = themes::resolve_theme_preference(
        ThemeMode::Dark,
        dark_key.as_deref(),
        dark_name.as_deref(),
        storage.as_ref(),
        cx,
    );
    let mut light_preview = theme_preview_tokens(light_resolved.entry.clone());
    light_preview.notice = light_resolved.notice.clone().map(Into::into);
    let mut dark_preview = theme_preview_tokens(dark_resolved.entry.clone());
    dark_preview.notice = dark_resolved.notice.clone().map(Into::into);
    let (primary_preview, alternate_preview) = match current_mode {
        ThemeMode::Light => (&light_preview, &dark_preview),
        ThemeMode::Dark => (&dark_preview, &light_preview),
    };
    let alternate_label = match current_mode {
        ThemeMode::Light => "Show alternate dark palette",
        ThemeMode::Dark => "Show alternate light palette",
    };
    let hide_alternate_label = match current_mode {
        ThemeMode::Light => "Hide alternate dark palette",
        ThemeMode::Dark => "Hide alternate light palette",
    };
    let toggle_handle = app.clone();

    v_flex()
        .w_full()
        .gap_3()
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child("Appearance policy picks the active side. This panel defaults to the palette you are actually using, with the alternate palette available on demand."),
        )
        .child(render_theme_preview_card(primary_preview))
        .child(
            h_flex().items_center().gap_2().child(
                Button::new("advanced-theme-preview-toggle")
                    .label(if show_alternate_preview {
                        hide_alternate_label
                    } else {
                        alternate_label
                    })
                    .outline()
                    .small()
                    .compact()
                    .on_click(move |_, _, cx| {
                        toggle_handle.update(cx, |app, cx| {
                            app.set_show_alternate_theme_preview(
                                !app.ui_session.show_alternate_theme_preview,
                                cx,
                            );
                        });
                    }),
            ),
        )
        .when(show_alternate_preview, |this| {
            this.child(render_theme_preview_card(alternate_preview))
        })
}

fn render_theme_file_workflow_field(app: Entity<WgApp>, cx: &mut gpui::App) -> Div {
    let open_handle = app.clone();
    let import_handle = app.clone();
    let duplicate_handle = app.clone();
    let restore_handle = app;

    v_flex()
        .w_full()
        .gap_3()
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child("Custom imports are sanitized to colors and highlight only. Curated themes can be restored without affecting your config library."),
        )
        .child(
            h_flex()
                .items_center()
                .gap_2()
                .flex_wrap()
                .child(
                    Button::new("advanced-theme-open-folder")
                        .label("Open Themes Folder")
                        .outline()
                        .small()
                        .compact()
                        .on_click(move |_, window, cx| {
                            open_handle.update(cx, |app, cx| {
                                app.open_themes_folder(window, cx);
                            });
                        }),
                )
                .child(
                    Button::new("advanced-theme-import-json")
                        .label("Import Theme JSON")
                        .outline()
                        .small()
                        .compact()
                        .on_click(move |_, window, cx| {
                            import_handle.update(cx, |app, cx| {
                                app.handle_theme_import_click(window, cx);
                            });
                        }),
                )
                .child(
                    Button::new("advanced-theme-duplicate-template")
                        .label("Duplicate Current Theme")
                        .outline()
                        .small()
                        .compact()
                        .on_click(move |_, window, cx| {
                            duplicate_handle.update(cx, |app, cx| {
                                app.duplicate_current_theme_template(window, cx);
                            });
                        }),
                )
                .child(
                    Button::new("advanced-theme-restore-curated")
                        .label("Restore Curated Themes")
                        .outline()
                        .small()
                        .compact()
                        .on_click(move |_, window, cx| {
                            restore_handle.update(cx, |app, cx| {
                                app.restore_curated_theme_files(window, cx);
                            });
                        }),
                ),
        )
}

struct ThemePreviewTokens {
    mode: ThemeMode,
    name: SharedString,
    notice: Option<SharedString>,
    collection_label: SharedString,
    approval_label: Option<SharedString>,
    tags: Vec<SharedString>,
    lint_items: Vec<ThemeLintItem>,
    background: Hsla,
    panel: Hsla,
    border: Hsla,
    foreground: Hsla,
    muted_foreground: Hsla,
    accent: Hsla,
    success: Hsla,
    success_foreground: Hsla,
    warning: Hsla,
    warning_foreground: Hsla,
    info: Hsla,
    info_foreground: Hsla,
    danger: Hsla,
    danger_foreground: Hsla,
    chart_1: Hsla,
    chart_2: Hsla,
    chart_3: Hsla,
    chart_4: Hsla,
    chart_5: Hsla,
    input_border: Hsla,
    list_hover: Hsla,
    list_active: Hsla,
    list_active_border: Hsla,
    sidebar: Hsla,
    sidebar_foreground: Hsla,
    sidebar_border: Hsla,
    sidebar_accent: Hsla,
    sidebar_accent_foreground: Hsla,
    sidebar_primary: Hsla,
    sidebar_primary_foreground: Hsla,
    popover: Hsla,
    popover_foreground: Hsla,
    overlay: Hsla,
    secondary: Hsla,
    secondary_foreground: Hsla,
    ring: Hsla,
    muted: Hsla,
}

fn theme_preview_tokens(entry: themes::ThemeCatalogEntry) -> ThemePreviewTokens {
    let mut theme = Theme::default();
    theme.apply_config(&entry.config);

    ThemePreviewTokens {
        mode: entry.config.mode,
        name: entry.name.clone(),
        notice: None,
        collection_label: entry.collection.label().into(),
        approval_label: entry.badge_label().map(Into::into),
        tags: entry.tags.clone(),
        lint_items: theme_lint::lint_theme_config(entry.config.as_ref()),
        background: theme.background,
        panel: theme.group_box,
        border: theme.border,
        foreground: theme.foreground,
        muted_foreground: theme.muted_foreground,
        accent: theme.accent,
        success: theme.success,
        success_foreground: theme.success_foreground,
        warning: theme.warning,
        warning_foreground: theme.warning_foreground,
        info: theme.info,
        info_foreground: theme.info_foreground,
        danger: theme.danger,
        danger_foreground: theme.danger_foreground,
        chart_1: theme.chart_1,
        chart_2: theme.chart_2,
        chart_3: theme.chart_3,
        chart_4: theme.chart_4,
        chart_5: theme.chart_5,
        input_border: theme.input,
        list_hover: theme.list_hover,
        list_active: theme.list_active,
        list_active_border: theme.list_active_border,
        sidebar: theme.sidebar,
        sidebar_foreground: theme.sidebar_foreground,
        sidebar_border: theme.sidebar_border,
        sidebar_accent: theme.sidebar_accent,
        sidebar_accent_foreground: theme.sidebar_accent_foreground,
        sidebar_primary: theme.sidebar_primary,
        sidebar_primary_foreground: theme.sidebar_primary_foreground,
        popover: theme.popover,
        popover_foreground: theme.popover_foreground,
        overlay: theme.overlay,
        secondary: theme.secondary,
        secondary_foreground: theme.secondary_foreground,
        ring: theme.ring,
        muted: theme.muted,
    }
}

fn add_theme_menu_section(
    mut menu: PopupMenu,
    title: &'static str,
    entries: Vec<themes::ThemeCatalogEntry>,
    selected_key: &str,
    set_handle: Entity<WgApp>,
    mode: ThemeMode,
) -> PopupMenu {
    if entries.is_empty() {
        return menu;
    }

    menu = menu.label(title);
    for theme in entries {
        let checked = theme.key == selected_key;
        menu = menu.item(
            PopupMenuItem::new(theme.menu_label().to_string())
                .checked(checked)
                .on_click({
                    let set_handle = set_handle.clone();
                    let key = theme.key.clone();
                    move |_, window, cx| {
                        set_handle.update(cx, |app, cx| {
                            app.set_theme_palette_pref(mode, Some(key.clone()), Some(window), cx);
                        });
                    }
                }),
        );
    }

    menu
}

fn render_theme_preview_card(preview: &ThemePreviewTokens) -> Div {
    let mode_label = match preview.mode {
        ThemeMode::Light => "LIGHT",
        ThemeMode::Dark => "DARK",
    };
    let metadata = preview.approval_label.iter().fold(
        h_flex().items_center().gap_2().child(preview_outline_chip(
            preview.collection_label.clone(),
            preview.border,
            preview.muted_foreground,
            preview.panel,
        )),
        |row, label| {
            row.child(preview_color_chip(
                label.clone(),
                preview.accent.alpha(0.18),
                preview.accent,
            ))
        },
    );
    let metadata = preview.tags.iter().fold(metadata, |row, tag| {
        row.child(preview_outline_chip(
            tag.clone(),
            preview.border.alpha(0.82),
            preview.muted_foreground,
            preview.background,
        ))
    });
    div()
        .w_full()
        .rounded(px(18.0))
        .border_1()
        .border_color(preview.border)
        .bg(preview.background)
        .p_4()
        .child(
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
                                .child(
                                    div()
                                        .text_xs()
                                        .font_semibold()
                                        .text_color(preview.muted_foreground)
                                        .child(mode_label),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .font_semibold()
                                        .text_color(preview.foreground)
                                .child(preview.name.clone()),
                                ),
                        )
                )
                .child(metadata.flex_wrap())
                .child(
                    h_flex()
                        .items_start()
                        .gap_3()
                        .flex_wrap()
                        .child(
                            preview_section(
                                preview,
                                "Top Bar Status",
                                "Status should stay semantic, not brand-colored.",
                                h_flex()
                                    .items_center()
                                    .justify_between()
                                    .gap_2()
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .gap_2()
                                            .flex_wrap()
                                            .child(preview_status_chip(
                                                "Connected",
                                                preview.success,
                                                preview.success_foreground,
                                            ))
                                            .child(preview_status_chip(
                                                "Idle",
                                                preview.secondary,
                                                preview.secondary_foreground,
                                            ))
                                            .child(preview_status_chip(
                                                "Warning",
                                                preview.warning,
                                                preview.warning_foreground,
                                            )),
                                    )
                                    .child(preview_outline_chip(
                                        "Brand Accent",
                                        preview.accent.alpha(0.32),
                                        preview.accent,
                                        preview.background,
                                    )),
                            )
                            .flex_1(),
                        )
                        .child(
                            preview_section(
                                preview,
                                "Sidebar Selection",
                                "Selected and hover states should read from list and sidebar tokens.",
                                div()
                                    .rounded(px(12.0))
                                    .border_1()
                                    .border_color(preview.sidebar_border)
                                    .bg(preview.sidebar)
                                    .p_2()
                                    .child(
                                        v_flex()
                                            .gap_1()
                                            .child(preview_sidebar_row(
                                                preview, "Overview", false, false,
                                            ))
                                            .child(preview_sidebar_row(
                                                preview, "Configs", true, false,
                                            ))
                                            .child(preview_sidebar_row(
                                                preview, "Logs", false, true,
                                            )),
                                    ),
                            )
                            .flex_1(),
                        ),
                )
                .child(
                    h_flex()
                        .items_start()
                        .gap_3()
                        .flex_wrap()
                        .child(
                            preview_section(
                                preview,
                                "Inputs",
                                "Normal, focused, and disabled states need visible boundaries.",
                                v_flex()
                                    .gap_2()
                                    .child(preview_input_row(
                                        preview,
                                        "Normal",
                                        "DNS override",
                                        preview.input_border,
                                        preview.background,
                                        preview.foreground,
                                    ))
                                    .child(preview_input_row(
                                        preview,
                                        "Focused",
                                        "10.64.0.1",
                                        preview.ring,
                                        preview.background,
                                        preview.foreground,
                                    ))
                                    .child(preview_input_row(
                                        preview,
                                        "Disabled",
                                        "Managed by system",
                                        preview.border,
                                        preview.muted,
                                        preview.muted_foreground,
                                    )),
                            )
                            .flex_1(),
                        )
                        .child(
                            preview_section(
                                preview,
                                "Dialog, Sheet, Notification",
                                "Overlay surfaces should separate cleanly from the canvas.",
                                v_flex()
                                    .gap_2()
                                    .child(
                                        h_flex()
                                            .items_start()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .rounded(px(12.0))
                                                    .border_1()
                                                    .border_color(preview.border)
                                                    .bg(preview.popover)
                                                    .p_3()
                                                    .child(
                                                        v_flex()
                                                            .gap_1()
                                                            .child(
                                                                div()
                                                                    .text_xs()
                                                                    .font_semibold()
                                                                    .text_color(
                                                                        preview.popover_foreground,
                                                                    )
                                                                    .child("Dialog"),
                                                            )
                                                            .child(
                                                                div()
                                                                    .text_xs()
                                                                    .text_color(
                                                                        preview.muted_foreground,
                                                                    )
                                                                    .child("Apply imported theme?"),
                                                            ),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .rounded(px(12.0))
                                                    .border_1()
                                                    .border_color(preview.sidebar_border)
                                                    .bg(preview.panel)
                                                    .p_3()
                                                    .child(
                                                        v_flex()
                                                            .gap_1()
                                                            .child(
                                                                div()
                                                                    .text_xs()
                                                                    .font_semibold()
                                                                    .text_color(preview.foreground)
                                                                    .child("Sheet"),
                                                            )
                                                            .child(
                                                                div()
                                                                    .text_xs()
                                                                    .text_color(
                                                                        preview.muted_foreground,
                                                                    )
                                                                    .child("Files, restore, migration."),
                                                            ),
                                                    ),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .rounded(px(12.0))
                                            .border_1()
                                            .border_color(preview.info.alpha(0.35))
                                            .bg(preview.overlay.mix(preview.info, 0.82))
                                            .p_3()
                                            .child(
                                                h_flex()
                                                    .items_start()
                                                    .justify_between()
                                                    .gap_2()
                                                    .child(
                                                        v_flex()
                                                            .gap_1()
                                                            .child(
                                                                div()
                                                                    .text_xs()
                                                                    .font_semibold()
                                                                    .text_color(
                                                                        preview.info_foreground,
                                                                    )
                                                                    .child("Notification"),
                                                            )
                                                            .child(
                                                                div()
                                                                    .text_xs()
                                                                    .text_color(
                                                                        preview.popover_foreground,
                                                                    )
                                                                    .child(preview.notice.clone().unwrap_or_else(
                                                                        || "Palette preview is ready.".into(),
                                                                    )),
                                                            ),
                                                    )
                                                    .child(preview_color_chip(
                                                        "Toast",
                                                        preview.info,
                                                        preview.info_foreground,
                                                    )),
                                            ),
                                    ),
                            )
                            .flex_1(),
                        ),
                )
                .child(
                    h_flex()
                        .items_start()
                        .gap_3()
                        .flex_wrap()
                        .child(
                            preview_section(
                                preview,
                                "Traffic Chart",
                                "Series should stay separable across light and dark palettes.",
                                v_flex()
                                    .gap_2()
                                    .child(
                                        h_flex()
                                            .items_end()
                                            .gap_2()
                                            .child(preview_chart_bar(preview.chart_1, 28.0))
                                            .child(preview_chart_bar(preview.chart_2, 42.0))
                                            .child(preview_chart_bar(preview.chart_3, 18.0))
                                            .child(preview_chart_bar(preview.chart_4, 56.0))
                                            .child(preview_chart_bar(preview.chart_5, 34.0)),
                                    )
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .gap_2()
                                            .flex_wrap()
                                            .child(preview_chart_chip("Upload", preview.chart_1))
                                            .child(preview_chart_chip(
                                                "Download",
                                                preview.chart_2,
                                            ))
                                            .child(preview_chart_chip("Peers", preview.chart_3))
                                            .child(preview_chart_chip("Latency", preview.chart_4))
                                            .child(preview_chart_chip("Errors", preview.chart_5)),
                                    ),
                            )
                            .flex_1(),
                        )
                        .child(
                            preview_section(
                                preview,
                                "Error And Repair",
                                "Recovery actions should stay unmistakable even when accent is loud.",
                                v_flex()
                                    .gap_2()
                                    .child(
                                        div()
                                            .rounded(px(12.0))
                                            .border_1()
                                            .border_color(preview.danger.alpha(0.32))
                                            .bg(preview.danger.alpha(0.1))
                                            .p_3()
                                            .child(
                                                v_flex()
                                                    .gap_2()
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .font_semibold()
                                                            .text_color(preview.danger)
                                                            .child(
                                                                "Handshake lost on preferred peer",
                                                            ),
                                                    )
                                                    .child(
                                                        h_flex()
                                                            .items_center()
                                                            .gap_2()
                                                            .flex_wrap()
                                                            .child(preview_action_chip(
                                                                "Retry Handshake",
                                                                preview.info,
                                                                preview.info_foreground,
                                                            ))
                                                            .child(preview_action_chip(
                                                                "Repair Config",
                                                                preview.warning,
                                                                preview.warning_foreground,
                                                            ))
                                                            .child(preview_action_chip(
                                                                "Discard Node",
                                                                preview.danger,
                                                                preview.danger_foreground,
                                                            )),
                                                    ),
                                            ),
                                    ),
                            )
                            .flex_1(),
                        ),
                )
                .child(render_theme_lint_panel(preview, &preview.lint_items)),
        )
}

fn preview_section(
    preview: &ThemePreviewTokens,
    title: &'static str,
    description: &'static str,
    content: impl IntoElement,
) -> Div {
    div()
        .rounded(px(14.0))
        .border_1()
        .border_color(preview.border)
        .bg(preview.panel)
        .p_3()
        .child(
            v_flex()
                .gap_3()
                .child(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_sm()
                                .font_semibold()
                                .text_color(preview.foreground)
                                .child(title),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(preview.muted_foreground)
                                .child(description),
                        ),
                )
                .child(content),
        )
}

fn render_theme_lint_panel(preview: &ThemePreviewTokens, lint_items: &[ThemeLintItem]) -> Div {
    let body = if lint_items.is_empty() {
        v_flex().gap_2().child(
            div()
                .rounded(px(12.0))
                .border_1()
                .border_color(preview.success.alpha(0.28))
                .bg(preview.success.alpha(0.1))
                .p_3()
                .child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(preview_color_chip(
                            "Pass",
                            preview.success,
                            preview.success_foreground,
                        ))
                        .child(
                            div()
                                .text_xs()
                                .text_color(preview.foreground)
                                .child("No obvious contrast or semantic collisions found."),
                        ),
                ),
        )
    } else {
        lint_items.iter().fold(v_flex().gap_2(), |column, item| {
            let (tone, text) = lint_severity_colors(preview, item.severity);
            column.child(
                div()
                    .rounded(px(12.0))
                    .border_1()
                    .border_color(tone.alpha(0.32))
                    .bg(tone.alpha(0.1))
                    .p_3()
                    .child(
                        v_flex()
                            .gap_2()
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_2()
                                    .child(preview_color_chip(item.severity.label(), tone, text))
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_semibold()
                                            .text_color(preview.foreground)
                                            .child(item.title),
                                    ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(preview.muted_foreground)
                                    .child(item.detail.clone()),
                            ),
                    ),
            )
        })
    };

    preview_section(
        preview,
        "Theme Lint",
        "Fast heuristics for contrast, boundary clarity, semantic separation, and chart distinctness.",
        body,
    )
}

fn lint_severity_colors(preview: &ThemePreviewTokens, severity: ThemeLintSeverity) -> (Hsla, Hsla) {
    match severity {
        ThemeLintSeverity::Error => (preview.danger, preview.danger_foreground),
        ThemeLintSeverity::Warning => (preview.warning, preview.warning_foreground),
        ThemeLintSeverity::Info => (preview.info, preview.info_foreground),
    }
}

fn preview_sidebar_row(
    preview: &ThemePreviewTokens,
    label: &'static str,
    selected: bool,
    hovered: bool,
) -> Div {
    let (background, foreground, marker) = if selected {
        (
            preview.sidebar_primary,
            preview.sidebar_primary_foreground,
            preview.sidebar_primary,
        )
    } else if hovered {
        (
            preview.sidebar_accent,
            preview.sidebar_accent_foreground,
            preview.sidebar_accent,
        )
    } else {
        (
            preview.sidebar,
            preview.sidebar_foreground,
            preview.sidebar_border,
        )
    };

    h_flex()
        .items_center()
        .justify_between()
        .gap_2()
        .rounded(px(10.0))
        .border_1()
        .border_color(if selected {
            preview.list_active_border
        } else {
            preview.sidebar_border.alpha(0.32)
        })
        .bg(if selected {
            preview.list_active
        } else if hovered {
            preview.list_hover
        } else {
            background
        })
        .px_3()
        .py_2()
        .child(
            h_flex()
                .items_center()
                .gap_2()
                .child(div().size(px(8.0)).rounded_full().bg(marker))
                .child(
                    div()
                        .text_xs()
                        .font_semibold()
                        .text_color(foreground)
                        .child(label),
                ),
        )
        .when(selected, |this| {
            this.child(preview_color_chip(
                "Live",
                preview.info,
                preview.info_foreground,
            ))
        })
        .when(hovered && !selected, |this| {
            this.child(
                div()
                    .text_xs()
                    .text_color(preview.muted_foreground)
                    .child("Hover"),
            )
        })
}

fn preview_input_row(
    preview: &ThemePreviewTokens,
    label: &'static str,
    value: &'static str,
    border: Hsla,
    background: Hsla,
    foreground: Hsla,
) -> Div {
    div()
        .rounded(px(12.0))
        .border_1()
        .border_color(border)
        .bg(background)
        .px_3()
        .py_2()
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_xs()
                                .font_semibold()
                                .text_color(preview.muted_foreground)
                                .child(label),
                        )
                        .child(div().text_xs().text_color(foreground).child(value)),
                )
                .child(div().size(px(8.0)).rounded_full().bg(border)),
        )
}

fn preview_chart_bar(color: Hsla, height: f32) -> Div {
    v_flex()
        .w(px(24.0))
        .h(px(64.0))
        .justify_end()
        .child(div().w_full().h(px(height)).rounded(px(8.0)).bg(color))
}

fn preview_action_chip(label: impl Into<SharedString>, background: Hsla, foreground: Hsla) -> Div {
    div()
        .px_3()
        .py_1()
        .rounded(px(10.0))
        .bg(background)
        .text_color(foreground)
        .text_xs()
        .font_semibold()
        .child(label.into())
}

fn preview_status_chip(label: impl Into<SharedString>, background: Hsla, foreground: Hsla) -> Div {
    preview_action_chip(label, background, foreground)
}

fn preview_color_chip(label: impl Into<SharedString>, background: Hsla, foreground: Hsla) -> Div {
    div()
        .px_3()
        .py_1()
        .rounded_full()
        .bg(background)
        .text_color(foreground)
        .text_xs()
        .font_semibold()
        .child(label.into())
}

fn preview_outline_chip(
    label: impl Into<SharedString>,
    border: Hsla,
    foreground: Hsla,
    background: Hsla,
) -> Div {
    div()
        .px_3()
        .py_1()
        .rounded_full()
        .border_1()
        .border_color(border)
        .bg(background)
        .text_color(foreground)
        .text_xs()
        .font_semibold()
        .child(label.into())
}

fn preview_chart_chip(label: &'static str, color: Hsla) -> Div {
    div()
        .px_2()
        .py_1()
        .rounded(px(8.0))
        .bg(color.alpha(0.18))
        .border_1()
        .border_color(color.alpha(0.38))
        .child(
            h_flex()
                .items_center()
                .gap_2()
                .child(div().size(px(8.0)).rounded_full().bg(color))
                .child(div().text_xs().child(label)),
        )
}

fn log_auto_follow_item(app: Entity<WgApp>) -> SettingItem {
    let get_handle = app.clone();
    let set_handle = app;

    SettingItem::new(
        "Auto Follow Logs",
        SettingField::switch(
            move |cx| get_handle.read(cx).ui_prefs.log_auto_follow,
            move |value, cx| {
                set_handle.update(cx, |app, cx| {
                    app.set_log_auto_follow_pref(value, cx);
                });
            },
        ),
    )
    .description("Keep the log pane pinned to the latest runtime events.")
}

fn dns_mode_item(app: Entity<WgApp>) -> SettingItem {
    let options = dns_mode_options();
    let get_handle = app.clone();
    let set_handle = app;

    SettingItem::new(
        "DNS Mode",
        SettingField::dropdown(
            options,
            move |cx| dns_mode_value(get_handle.read(cx).ui_prefs.dns_mode),
            move |value, cx| {
                let next = dns_mode_from_value(&value);
                set_handle.update(cx, |app, cx| {
                    app.set_dns_mode_pref(next, cx);
                });
            },
        ),
    )
    .description("Choose whether config DNS, system DNS, or presets take precedence.")
}

fn dns_preset_item(app: Entity<WgApp>) -> SettingItem {
    SettingItem::new(
        "DNS Preset",
        SettingField::render(move |_, _window, cx| render_dns_preset_field(app.clone(), cx)),
    )
    .description("Only used when DNS mode fills or overrides resolver records.")
}

fn traffic_period_item(app: Entity<WgApp>) -> SettingItem {
    SettingItem::new(
        "Preferred Traffic Range",
        SettingField::render(move |_, _window, cx| {
            let current = app.read(cx).ui_prefs.preferred_traffic_period;
            let today_handle = app.clone();
            let month_handle = app.clone();
            let last_month_handle = app.clone();

            div().child(
                ButtonGroup::new("advanced-traffic-period")
                    .outline()
                    .small()
                    .compact()
                    .child(
                        Button::new("advanced-traffic-today")
                            .label("Today")
                            .selected(current == TrafficPeriod::Today)
                            .on_click(move |_, _, cx| {
                                today_handle.update(cx, |app, cx| {
                                    app.set_preferred_traffic_period(TrafficPeriod::Today, cx);
                                });
                            }),
                    )
                    .child(
                        Button::new("advanced-traffic-this-month")
                            .label("This Month")
                            .selected(current == TrafficPeriod::ThisMonth)
                            .on_click(move |_, _, cx| {
                                month_handle.update(cx, |app, cx| {
                                    app.set_preferred_traffic_period(TrafficPeriod::ThisMonth, cx);
                                });
                            }),
                    )
                    .child(
                        Button::new("advanced-traffic-last-month")
                            .label("Last Month")
                            .selected(current == TrafficPeriod::LastMonth)
                            .on_click(move |_, _, cx| {
                                last_month_handle.update(cx, |app, cx| {
                                    app.set_preferred_traffic_period(TrafficPeriod::LastMonth, cx);
                                });
                            }),
                    ),
            )
        }),
    )
    .description("Applies now and stays remembered for future sessions.")
}

fn inspector_tab_item(app: Entity<WgApp>) -> SettingItem {
    SettingItem::new(
        "Inspector View",
        SettingField::render(move |_, _window, cx| {
            let current = app.read(cx).ui_prefs.preferred_inspector_tab;
            let preview_handle = app.clone();
            let diagnostics_handle = app.clone();
            let activity_handle = app.clone();

            div().child(
                ButtonGroup::new("advanced-inspector-default")
                    .outline()
                    .small()
                    .compact()
                    .child(
                        Button::new("advanced-inspector-preview")
                            .label("Preview")
                            .selected(current == ConfigInspectorTab::Preview)
                            .on_click(move |_, _, cx| {
                                preview_handle.update(cx, |app, cx| {
                                    app.set_preferred_inspector_tab(
                                        ConfigInspectorTab::Preview,
                                        cx,
                                    );
                                });
                            }),
                    )
                    .child(
                        Button::new("advanced-inspector-diagnostics")
                            .label("Diagnostics")
                            .selected(current == ConfigInspectorTab::Diagnostics)
                            .on_click(move |_, _, cx| {
                                diagnostics_handle.update(cx, |app, cx| {
                                    app.set_preferred_inspector_tab(
                                        ConfigInspectorTab::Diagnostics,
                                        cx,
                                    );
                                });
                            }),
                    )
                    .child(
                        Button::new("advanced-inspector-activity")
                            .label("Activity")
                            .selected(current == ConfigInspectorTab::Activity)
                            .on_click(move |_, _, cx| {
                                activity_handle.update(cx, |app, cx| {
                                    app.set_preferred_inspector_tab(
                                        ConfigInspectorTab::Activity,
                                        cx,
                                    );
                                });
                            }),
                    ),
            )
        }),
    )
    .description("Controls which Inspector view opens first in Configs.")
}

fn privileged_backend_item(app: Entity<WgApp>) -> SettingItem {
    SettingItem::new(
        "Service Status",
        SettingField::render(move |_, window, cx| {
            render_privileged_backend_panel(app.clone(), window, cx)
        }),
    )
    .layout(Axis::Vertical)
}

fn troubleshooting_item() -> SettingItem {
    SettingItem::new(
        "Troubleshooting",
        SettingField::render(|_, _, cx| {
            v_flex()
                .w_full()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("Refresh re-checks helper status only. No system changes."),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(
                            "Repair reinstalls helper integration and fixes protocol or permission drift.",
                        ),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("Remove uninstalls the helper integration only. Tunnel configs are kept."),
                )
        }),
    )
    .layout(Axis::Vertical)
    .description("What Refresh, Repair, and Remove do.")
}

fn render_dns_preset_field(app: Entity<WgApp>, cx: &mut gpui::App) -> Div {
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

fn render_privileged_backend_panel(
    app: Entity<WgApp>,
    window: &mut Window,
    cx: &mut gpui::App,
) -> Div {
    ensure_backend_freshness_ticker(app.clone(), window, cx);
    let diagnostic = app.read(cx).ui.backend.clone();
    let busy = diagnostic.is_busy();
    let note = backend_recovery_note(&diagnostic);
    let details_open = window.use_keyed_state("backend-details-open", cx, |_, _| false);
    let is_details_open = *details_open.read(cx);

    let refresh_handle = app.clone();
    let install_handle = app.clone();
    let repair_handle = app.clone();
    let copy_handle = app.clone();
    let details_handle = details_open.clone();
    let details_app_handle = app.clone();
    let remove_handle = app;

    div()
        .w_full()
        .child(
            v_flex()
                .gap_3()
                .child(
                    h_flex()
                        .items_start()
                        .justify_between()
                        .gap_3()
                        .flex_wrap()
                        .child(
                            v_flex()
                                .gap_1()
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(
                                            "Helper service used for tunnel startup, routes, and DNS changes.",
                                        ),
                                ),
                        )
                        .child(backend_status_tag(
                            &diagnostic,
                            SharedString::from(diagnostic.summary()),
                        ))
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(backend_checked_label(&diagnostic)),
                        ),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().foreground)
                        .child(diagnostic.detail.clone()),
                )
                .when_some(note, |this, note| {
                    this.child(
                        div()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(note),
                            ),
                    )
                })
                .child(
                    h_flex()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .flex_wrap()
                        .child(
                            h_flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    Button::new("backend-refresh-status")
                                        .label("Refresh")
                                        .outline()
                                        .small()
                                        .compact()
                                        .loading(matches!(
                                            diagnostic.health,
                                            BackendHealth::Checking
                                        ))
                                        .disabled(busy)
                                        .on_click(move |_, _, cx| {
                                            refresh_handle.update(cx, |app, cx| {
                                                app.refresh_privileged_backend_status(cx);
                                            });
                                        }),
                                )
                                .when(
                                    diagnostic.allows_action(PrivilegedServiceAction::Install),
                                    |this| {
                                        this.child(
                                            Button::new("backend-install")
                                                .label("Install")
                                                .small()
                                                .compact()
                                                .loading(diagnostic.is_working_action(
                                                    PrivilegedServiceAction::Install,
                                                ))
                                                .disabled(busy)
                                                .on_click(move |_, _, cx| {
                                                    install_handle.update(cx, |app, cx| {
                                                            app.run_privileged_backend_action(
                                                                PrivilegedServiceAction::Install,
                                                                cx,
                                                            );
                                                        });
                                                }),
                                        )
                                    },
                                )
                                .when(should_show_repair_action(&diagnostic), |this| {
                                    this.child(
                                        Button::new("backend-repair")
                                            .label("Repair")
                                            .outline()
                                            .small()
                                            .compact()
                                            .loading(diagnostic.is_working_action(
                                                PrivilegedServiceAction::Repair,
                                            ))
                                            .disabled(busy)
                                            .on_click(move |_, _, cx| {
                                                repair_handle.update(cx, |app, cx| {
                                                    app.run_privileged_backend_action(
                                                        PrivilegedServiceAction::Repair,
                                                        cx,
                                                    );
                                                });
                                            }),
                                    )
                                })
                                .child(
                                    Button::new("backend-copy-diagnostics")
                                        .label("Copy Diagnostics")
                                        .outline()
                                        .small()
                                        .compact()
                                        .on_click({
                                            move |_, window, cx| {
                                                copy_handle.update(cx, |app, cx| {
                                                    cx.write_to_clipboard(
                                                        gpui::ClipboardItem::new_string(
                                                            build_backend_diagnostics_text(app),
                                                        ),
                                                    );
                                                    app.push_success_toast(
                                                        "Diagnostics copied",
                                                        window,
                                                        cx,
                                                    );
                                                });
                                            }
                                        }),
                                )
                                .child(
                                    Button::new("backend-toggle-details")
                                        .label(if is_details_open {
                                            "Hide Details"
                                        } else {
                                            "Details"
                                        })
                                        .outline()
                                        .small()
                                        .compact()
                                        .on_click(move |_, _, cx| {
                                            details_handle.update(cx, |open, _| {
                                                *open = !*open;
                                            });
                                        }),
                                ),
                        )
                        .when(should_show_remove_action(&diagnostic), |this| {
                            this.child(
                                Button::new("backend-remove")
                                    .label("Remove")
                                    .danger()
                                    .outline()
                                    .small()
                                    .compact()
                                    .loading(
                                        diagnostic.is_working_action(PrivilegedServiceAction::Remove),
                                    )
                                    .disabled(busy)
                                    .on_click(move |_, window, cx| {
                                        open_backend_remove_dialog(
                                            remove_handle.clone(),
                                            window,
                                            cx,
                                        );
                                    }),
                            )
                        }),
                ),
        )
        .when(is_details_open, |this| {
            this.child(render_backend_details(&details_app_handle, cx))
        })
}

fn open_backend_remove_dialog(app_handle: Entity<WgApp>, window: &mut Window, cx: &mut gpui::App) {
    window.open_dialog(cx, move |dialog, _window, cx| {
        let remove_handle = app_handle.clone();
        dialog
            .title(div().text_lg().child("Remove Privileged Backend?"))
            .confirm()
            .button_props(
                DialogButtonProps::default()
                    .ok_text("Remove")
                    .ok_variant(ButtonVariant::Danger)
                    .cancel_text("Cancel"),
            )
            .child(
                div()
                    .text_sm()
                    .child("Remove the helper integration from the operating system?"),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                    "Removes the helper integration only. Existing tunnel configs are not deleted.",
                ),
            )
            .on_ok(move |_, _window, cx| {
                remove_handle.update(cx, |app, cx| {
                    app.run_privileged_backend_action(PrivilegedServiceAction::Remove, cx);
                });
                true
            })
    });
}

fn backend_checked_label(diagnostic: &BackendDiagnostic) -> SharedString {
    match diagnostic.checked_at {
        Some(checked_at) => {
            let prefix = if diagnostic.is_busy() {
                "Last checked "
            } else {
                "Checked "
            };
            format!("{prefix}{}", format_checked_age(checked_at)).into()
        }
        None if diagnostic.is_busy() => "Checking now".into(),
        None => "Not checked yet".into(),
    }
}

fn ensure_backend_freshness_ticker(app: Entity<WgApp>, window: &mut Window, cx: &mut gpui::App) {
    let ticker_running = window.use_keyed_state("backend-freshness-ticker", cx, |_, _| false);
    if *ticker_running.read(cx) {
        return;
    }
    ticker_running.update(cx, |running, _| {
        *running = true;
    });

    cx.spawn({
        let ticker_running = ticker_running.clone();
        async move |cx| loop {
            Timer::after(Duration::from_secs(10)).await;

            let keep_running = app
                .update(cx, |app, cx| {
                    if app.ui_session.sidebar_active == SidebarItem::Advanced {
                        cx.notify();
                        true
                    } else {
                        false
                    }
                })
                .unwrap_or(false);

            if !keep_running {
                let _ = ticker_running.update(cx, |running, _| {
                    *running = false;
                });
                break;
            }
        }
    })
    .detach();
}

fn format_checked_age(checked_at: SystemTime) -> String {
    let elapsed = SystemTime::now()
        .duration_since(checked_at)
        .unwrap_or(Duration::from_secs(0));
    let seconds = elapsed.as_secs();

    match seconds {
        0..=9 => "just now".to_string(),
        10..=59 => format!("{seconds}s ago"),
        60..=3_599 => format!("{} min ago", seconds / 60),
        3_600..=86_399 => format!("{} hr ago", seconds / 3_600),
        _ => format!("{} d ago", seconds / 86_400),
    }
}

fn build_backend_diagnostics_text(app: &WgApp) -> String {
    let diagnostic = &app.ui.backend;
    let checked = diagnostic
        .checked_at
        .map(format_checked_timestamp)
        .unwrap_or_else(|| "not checked yet".to_string());
    let mut lines = vec![
        format!("App: r-wg v{}", env!("CARGO_PKG_VERSION")),
        format!("Platform: {OS} / {ARCH}"),
        format!("Health: {}", diagnostic.summary()),
        format!("Checked: {checked}"),
        format!("Integration: {}", helper_platform_detail()),
        format!("Control endpoint: {}", helper_control_endpoint()),
        format!(
            "Recommended next step: {}",
            backend_recommended_action(diagnostic)
        ),
        format!("Detail: {}", diagnostic.detail),
    ];
    if let Some(last_error) = &app.ui.backend_last_error {
        lines.push(format!("Backend last error: {last_error}"));
    }

    match diagnostic.health {
        BackendHealth::VersionMismatch { expected, actual } => {
            lines.push(format!(
                "Protocol mismatch: expected v{expected}, actual v{actual}"
            ));
        }
        BackendHealth::Unreachable => {
            lines.push(format!("Unreachable message: {}", diagnostic.detail));
        }
        _ => {}
    }

    lines.join("\n")
}

fn render_backend_details(app: &Entity<WgApp>, cx: &mut gpui::App) -> Div {
    let app = app.read(cx);
    let diagnostic = &app.ui.backend;
    let checked = diagnostic
        .checked_at
        .map(format_checked_timestamp)
        .unwrap_or_else(|| "not checked yet".to_string());
    let backend_last_error = app
        .ui
        .backend_last_error
        .as_ref()
        .map(|err| err.to_string())
        .unwrap_or_else(|| "None".to_string());

    let details = DescriptionList::new()
        .columns(1)
        .item("Integration", helper_platform_detail(), 1)
        .item("Control Endpoint", helper_control_endpoint(), 1)
        .item("Health", diagnostic.summary(), 1)
        .item("Checked", checked, 1)
        .item("Recommended", backend_recommended_action(diagnostic), 1)
        .item("Backend Last Error", backend_last_error, 1);

    let details = if let BackendHealth::VersionMismatch { expected, actual } = diagnostic.health {
        details.item(
            "Protocol",
            format!("Expected v{expected}, actual v{actual}"),
            1,
        )
    } else {
        details
    };

    let details = if matches!(diagnostic.health, BackendHealth::Unreachable) {
        details.item("Transport Error", diagnostic.detail.to_string(), 1)
    } else {
        details
    };

    div()
        .pt_2()
        .border_t_1()
        .border_color(cx.theme().border)
        .child(details)
}

fn format_checked_timestamp(checked_at: SystemTime) -> String {
    let absolute = DateTime::<Local>::from(checked_at)
        .format("%Y-%m-%d %H:%M:%S local")
        .to_string();
    format!("{absolute} ({})", format_checked_age(checked_at))
}

fn backend_recommended_action(diagnostic: &BackendDiagnostic) -> &'static str {
    match diagnostic.health {
        BackendHealth::Running => {
            "Repair or Remove can stop the running helper before applying system changes."
        }
        BackendHealth::NotInstalled => "Install the helper integration.",
        BackendHealth::Installed => "Refresh first, then Repair if the helper stays unavailable.",
        BackendHealth::AccessDenied | BackendHealth::VersionMismatch { .. } => {
            "Repair the helper integration."
        }
        BackendHealth::Unreachable => "Refresh first, then Repair if the helper stays unreachable.",
        BackendHealth::Checking => "Wait for the current probe to finish.",
        BackendHealth::Working { .. } => "Wait for the current action to finish.",
        BackendHealth::Unknown => "Refresh to probe the helper state.",
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        BackendHealth::Unsupported => "No helper actions are available on this platform.",
    }
}

fn helper_platform_detail() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "Linux privileged service"
    }
    #[cfg(target_os = "windows")]
    {
        "Windows privileged service"
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        "No privileged helper on this platform"
    }
}

fn helper_control_endpoint() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "/run/r-wg/control.sock"
    }
    #[cfg(target_os = "windows")]
    {
        r"\\.\pipe\r-wg-control"
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        "Not available"
    }
}

fn should_show_repair_action(diagnostic: &BackendDiagnostic) -> bool {
    diagnostic.allows_action(PrivilegedServiceAction::Repair)
}

fn should_show_remove_action(diagnostic: &BackendDiagnostic) -> bool {
    diagnostic.allows_action(PrivilegedServiceAction::Remove)
}

fn backend_recovery_note(diagnostic: &BackendDiagnostic) -> Option<SharedString> {
    let note = match diagnostic.health {
        BackendHealth::Running => {
            "Repair or Remove can stop the running helper first when you need to recover or uninstall it."
        }
        BackendHealth::NotInstalled => {
            "Install is the recommended next step before using desktop tunnel start, route, or DNS actions."
        }
        BackendHealth::Installed => {
            "The helper is installed but not currently live. Refresh first, then repair if the control channel stays unavailable."
        }
        BackendHealth::AccessDenied => {
            "Repair is the recommended next step when the helper exists but this account cannot reach it."
        }
        BackendHealth::VersionMismatch { .. } => {
            "Repair is the recommended next step when the installed helper protocol does not match this GUI build."
        }
        BackendHealth::Unreachable => {
            "Refresh re-checks the current state. Repair is the next step if the helper path or socket still looks stale."
        }
        _ => return None,
    };

    Some(note.into())
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

fn dns_mode_options() -> Vec<(SharedString, SharedString)> {
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

#[cfg(test)]
mod tests {
    use super::{
        backend_recommended_action, backend_recovery_note, should_show_remove_action,
        should_show_repair_action,
    };
    use crate::ui::state::{BackendDiagnostic, BackendHealth};

    fn diagnostic(health: BackendHealth) -> BackendDiagnostic {
        BackendDiagnostic {
            health,
            detail: "".into(),
            checked_at: None,
        }
    }

    #[test]
    fn running_backend_keeps_repair_and_remove_available() {
        let diagnostic = diagnostic(BackendHealth::Running);

        assert!(should_show_repair_action(&diagnostic));
        assert!(should_show_remove_action(&diagnostic));
    }

    #[test]
    fn running_backend_explains_recovery_actions() {
        let diagnostic = diagnostic(BackendHealth::Running);
        let note = backend_recovery_note(&diagnostic).map(|value| value.to_string());

        assert_eq!(
            backend_recommended_action(&diagnostic),
            "Repair or Remove can stop the running helper before applying system changes."
        );
        assert_eq!(
            note.as_deref(),
            Some(
                "Repair or Remove can stop the running helper first when you need to recover or uninstall it."
            )
        );
    }
}
