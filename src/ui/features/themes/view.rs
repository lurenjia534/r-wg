use gpui::{div, Axis, Div, Entity, ParentElement, Styled};
use gpui_component::button::{Button, ButtonGroup};
use gpui_component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_component::setting::{SettingField, SettingGroup, SettingItem};
use gpui_component::theme::ThemeMode;
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Selectable, Sizable as _};

use super::preview::render_theme_preview_field;
use super::{available_themes, resolve_theme_preference, AppearancePolicy, ThemeCatalogEntry};
use crate::ui::i18n::{tr, Language};
use crate::ui::persistence;
use crate::ui::state::WgApp;

// Theme policy, palette selection, preview, and lint presentation.

pub(crate) fn theme_settings_group(app: Entity<WgApp>, language: Language) -> SettingGroup {
    SettingGroup::new()
        .title(tr(language, "Appearance"))
        .description(
            "Separate the appearance policy from the light and dark palettes it resolves to.",
        )
        .item(theme_mode_item(app.clone(), language))
        .item(theme_palette_item(app.clone(), ThemeMode::Light, language))
        .item(theme_palette_item(app.clone(), ThemeMode::Dark, language))
        .item(reset_theme_item(app.clone()))
        .item(theme_file_workflow_item(app.clone()))
        .item(theme_preview_item(app))
}

fn theme_mode_item(app: Entity<WgApp>, language: Language) -> SettingItem {
    SettingItem::new(
        tr(language, "Appearance Policy"),
        SettingField::render(move |_, _window, cx| {
            let current = app.read(cx).ui_prefs.appearance_policy;
            let language = app.read(cx).language();
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
                            .label(tr(language, "Follow System"))
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
                            .label(tr(language, "Light"))
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
                            .label(tr(language, "Dark"))
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
    .description(tr(
        language,
        "Choose whether the app follows the OS, or stays pinned to light or dark.",
    ))
}

fn theme_palette_item(app: Entity<WgApp>, mode: ThemeMode, language: Language) -> SettingItem {
    let title = match mode {
        ThemeMode::Light => tr(language, "Light Palette"),
        ThemeMode::Dark => tr(language, "Dark Palette"),
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
    let resolved = resolve_theme_preference(
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
                let available = available_themes(mode, storage.as_ref(), cx);
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

fn add_theme_menu_section(
    mut menu: PopupMenu,
    title: &'static str,
    entries: Vec<ThemeCatalogEntry>,
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
