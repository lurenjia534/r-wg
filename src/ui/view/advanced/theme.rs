use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, Axis, Div, Entity, Hsla, IntoElement, ParentElement, SharedString, Styled,
};
use gpui_component::button::{Button, ButtonGroup};
use gpui_component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_component::setting::{SettingField, SettingItem};
use gpui_component::theme::{Colorize as _, Theme, ThemeMode};
use gpui_component::{
    h_flex, v_flex, ActiveTheme as _, Selectable, Sizable as _, StyledExt as _,
};

use crate::ui::persistence;
use crate::ui::state::WgApp;
use crate::ui::theme_lint::{self, ThemeLintItem, ThemeLintSeverity};
use crate::ui::themes::{self, AppearancePolicy};

// Theme policy, palette selection, preview, and lint presentation.

pub(super) fn theme_mode_item(app: Entity<WgApp>) -> SettingItem {
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

pub(super) fn theme_palette_item(app: Entity<WgApp>, mode: ThemeMode) -> SettingItem {
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

pub(super) fn reset_theme_item(app: Entity<WgApp>) -> SettingItem {
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

pub(super) fn theme_file_workflow_item(app: Entity<WgApp>) -> SettingItem {
    SettingItem::new(
        "Theme Files",
        SettingField::render(move |_, _window, cx| {
            render_theme_file_workflow_field(app.clone(), cx)
        }),
    )
    .layout(Axis::Vertical)
    .description("File-based workflow for importing, templating, and restoring curated themes.")
}

pub(super) fn theme_preview_item(app: Entity<WgApp>) -> SettingItem {
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
