use std::collections::HashMap;
use std::rc::Rc;

use gpui::{App, SharedString};
use gpui_component::theme::{ThemeMode, ThemeRegistry, ThemeSet};

use crate::ui::persistence::StoragePaths;

use super::files::{bundled_theme_files, is_bundled_theme_file, themes_dir};
use super::model::{ThemeApproval, ThemeCatalogEntry, ThemeCollection};

#[derive(Clone, Copy)]
struct CuratedThemeProfile {
    theme_name: &'static str,
    approval: ThemeApproval,
    tags: &'static [&'static str],
}

const CURATED_THEME_PROFILES: &[CuratedThemeProfile] = &[
    CuratedThemeProfile {
        theme_name: "Ayu Light",
        approval: ThemeApproval::Approved,
        tags: &["Balanced", "Warm"],
    },
    CuratedThemeProfile {
        theme_name: "Ayu Dark",
        approval: ThemeApproval::Experimental,
        tags: &["High Contrast", "Warm"],
    },
    CuratedThemeProfile {
        theme_name: "Catppuccin Latte",
        approval: ThemeApproval::Experimental,
        tags: &["Calm", "Warm"],
    },
    CuratedThemeProfile {
        theme_name: "Catppuccin Frappe",
        approval: ThemeApproval::Experimental,
        tags: &["Calm"],
    },
    CuratedThemeProfile {
        theme_name: "Catppuccin Macchiato",
        approval: ThemeApproval::Experimental,
        tags: &["Calm", "OLED-ish"],
    },
    CuratedThemeProfile {
        theme_name: "Catppuccin Mocha",
        approval: ThemeApproval::Approved,
        tags: &["Calm", "OLED-ish"],
    },
    CuratedThemeProfile {
        theme_name: "Flexoki Light",
        approval: ThemeApproval::Approved,
        tags: &["Balanced", "Warm"],
    },
    CuratedThemeProfile {
        theme_name: "Flexoki Dark",
        approval: ThemeApproval::Approved,
        tags: &["Balanced", "Calm"],
    },
    CuratedThemeProfile {
        theme_name: "Gruvbox Light",
        approval: ThemeApproval::Experimental,
        tags: &["Warm", "High Contrast"],
    },
    CuratedThemeProfile {
        theme_name: "Gruvbox Dark",
        approval: ThemeApproval::Experimental,
        tags: &["Warm", "High Contrast"],
    },
    CuratedThemeProfile {
        theme_name: "Solarized Light",
        approval: ThemeApproval::Approved,
        tags: &["Balanced", "Calm"],
    },
    CuratedThemeProfile {
        theme_name: "Solarized Dark",
        approval: ThemeApproval::Experimental,
        tags: &["Calm"],
    },
    CuratedThemeProfile {
        theme_name: "Tokyo Night",
        approval: ThemeApproval::Approved,
        tags: &["High Contrast", "OLED-ish"],
    },
    CuratedThemeProfile {
        theme_name: "Tokyo Storm",
        approval: ThemeApproval::Experimental,
        tags: &["Balanced", "Calm"],
    },
    CuratedThemeProfile {
        theme_name: "Tokyo Moon",
        approval: ThemeApproval::Experimental,
        tags: &["OLED-ish"],
    },
];

pub(crate) fn available_themes(
    mode: ThemeMode,
    storage: Option<&StoragePaths>,
    cx: &App,
) -> Vec<ThemeCatalogEntry> {
    let mut themes = collect_available_themes(mode, storage, cx);
    let name_counts = theme_name_counts(storage, cx);

    for theme in &mut themes {
        theme.name_conflict = name_counts
            .get(&theme_name_key(&theme.name))
            .copied()
            .unwrap_or_default()
            > 1;
    }

    themes.sort_by(|a, b| {
        a.sort_rank()
            .cmp(&b.sort_rank())
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then(
                a.source_label
                    .to_lowercase()
                    .cmp(&b.source_label.to_lowercase()),
            )
    });
    themes
}

pub(super) fn default_theme_entry(mode: ThemeMode, cx: &App) -> ThemeCatalogEntry {
    available_themes(mode, None, cx)
        .into_iter()
        .find(|entry| entry.collection == ThemeCollection::Builtin)
        .unwrap_or_else(|| {
            let config = if mode.is_dark() {
                ThemeRegistry::global(cx).default_dark_theme().clone()
            } else {
                ThemeRegistry::global(cx).default_light_theme().clone()
            };
            ThemeCatalogEntry {
                key: builtin_theme_key(config.mode, &config.name),
                name: config.name.clone(),
                collection: ThemeCollection::Builtin,
                approval: None,
                name_conflict: false,
                tags: Vec::new(),
                source_label: "default".into(),
                config,
            }
        })
}

pub(super) fn theme_name_counts(
    storage: Option<&StoragePaths>,
    cx: &App,
) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for theme in collect_available_themes(mode, storage, cx) {
            *counts.entry(theme_name_key(&theme.name)).or_insert(0usize) += 1;
        }
    }
    counts
}

pub(super) fn theme_name_key(name: &str) -> String {
    name.trim().to_lowercase()
}

pub(super) fn theme_mode_label(mode: ThemeMode) -> &'static str {
    match mode {
        ThemeMode::Light => "Light",
        ThemeMode::Dark => "Dark",
    }
}

fn collect_available_themes(
    mode: ThemeMode,
    storage: Option<&StoragePaths>,
    cx: &App,
) -> Vec<ThemeCatalogEntry> {
    let mut themes = Vec::new();
    push_builtin_theme_entries(mode, &mut themes, cx);
    push_curated_theme_entries(mode, &mut themes);
    push_custom_theme_entries(mode, storage, &mut themes);
    themes
}

fn push_builtin_theme_entries(mode: ThemeMode, entries: &mut Vec<ThemeCatalogEntry>, cx: &App) {
    let config = if mode.is_dark() {
        ThemeRegistry::global(cx).default_dark_theme().clone()
    } else {
        ThemeRegistry::global(cx).default_light_theme().clone()
    };
    entries.push(ThemeCatalogEntry {
        key: builtin_theme_key(config.mode, &config.name),
        name: config.name.clone(),
        collection: ThemeCollection::Builtin,
        approval: None,
        name_conflict: false,
        tags: Vec::new(),
        source_label: "default".into(),
        config,
    });
}

fn push_curated_theme_entries(mode: ThemeMode, entries: &mut Vec<ThemeCatalogEntry>) {
    for file in bundled_theme_files() {
        if let Ok(theme_set) = serde_json::from_str::<ThemeSet>(file.contents) {
            push_theme_set_entries(
                &theme_set,
                mode,
                ThemeCollection::Curated,
                file.file_name,
                entries,
            );
        }
    }
}

fn push_custom_theme_entries(
    mode: ThemeMode,
    storage: Option<&StoragePaths>,
    entries: &mut Vec<ThemeCatalogEntry>,
) {
    let Some(storage) = storage else {
        return;
    };
    let themes_dir = themes_dir(storage);
    let Ok(read_dir) = std::fs::read_dir(&themes_dir) else {
        return;
    };

    for entry in read_dir.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if path.extension().and_then(|ext| ext.to_str()) != Some("json")
            || is_bundled_theme_file(file_name)
        {
            continue;
        }
        let Ok(contents) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(theme_set) = serde_json::from_str::<ThemeSet>(&contents) else {
            continue;
        };
        push_theme_set_entries(
            &theme_set,
            mode,
            ThemeCollection::Custom,
            file_name,
            entries,
        );
    }
}

fn push_theme_set_entries(
    theme_set: &ThemeSet,
    mode: ThemeMode,
    collection: ThemeCollection,
    file_name: &str,
    entries: &mut Vec<ThemeCatalogEntry>,
) {
    for theme in theme_set.themes.iter().filter(|theme| theme.mode == mode) {
        let (approval, tags) = theme_catalog_metadata(collection, &theme.name);
        entries.push(ThemeCatalogEntry {
            key: file_theme_key(collection, file_name, theme.mode, &theme.name),
            name: theme.name.clone(),
            collection,
            approval,
            name_conflict: false,
            tags,
            source_label: file_name.to_string().into(),
            config: Rc::new(theme.clone()),
        });
    }
}

fn theme_catalog_metadata(
    collection: ThemeCollection,
    theme_name: &str,
) -> (Option<ThemeApproval>, Vec<SharedString>) {
    if collection != ThemeCollection::Curated {
        return (None, Vec::new());
    }

    let Some(profile) = CURATED_THEME_PROFILES
        .iter()
        .find(|profile| profile.theme_name.eq_ignore_ascii_case(theme_name))
    else {
        return (Some(ThemeApproval::Experimental), Vec::new());
    };

    (
        Some(profile.approval),
        profile.tags.iter().map(|tag| (*tag).into()).collect(),
    )
}

fn builtin_theme_key(mode: ThemeMode, name: &str) -> SharedString {
    format!("builtin:{}#{}", theme_mode_key(mode), sanitize_key(name)).into()
}

fn file_theme_key(
    collection: ThemeCollection,
    file_name: &str,
    mode: ThemeMode,
    name: &str,
) -> SharedString {
    format!(
        "{}:{}#{}-{}",
        match collection {
            ThemeCollection::Builtin => "builtin",
            ThemeCollection::Curated => "curated",
            ThemeCollection::Custom => "custom",
        },
        sanitize_key(file_name),
        theme_mode_key(mode),
        sanitize_key(name)
    )
    .into()
}

fn sanitize_key(value: &str) -> String {
    let value = value.trim().to_lowercase();
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            out.push(ch);
        } else {
            out.push('-');
        }
    }
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    out.trim_matches('-').to_string()
}

fn theme_mode_key(mode: ThemeMode) -> &'static str {
    match mode {
        ThemeMode::Light => "light",
        ThemeMode::Dark => "dark",
    }
}
