use std::collections::HashSet;
use std::rc::Rc;

use gpui::{App, Window};
use gpui_component::theme::{Theme, ThemeConfig, ThemeMode};

use crate::ui::persistence::StoragePaths;

use super::catalog::{available_themes, default_theme_entry, theme_mode_label, theme_name_key};
use super::model::{AppearancePolicy, ResolvedThemePreference};

pub(crate) fn resolve_theme_preference(
    mode: ThemeMode,
    preferred_key: Option<&str>,
    preferred_name: Option<&str>,
    storage: Option<&StoragePaths>,
    cx: &App,
) -> ResolvedThemePreference {
    let available = available_themes(mode, storage, cx);

    if let Some(key) = preferred_key {
        if let Some(entry) = available.iter().find(|entry| entry.key == key).cloned() {
            let migrated = preferred_name
                .map(|name| !entry.name.eq_ignore_ascii_case(name))
                .unwrap_or(false);
            return ResolvedThemePreference {
                entry,
                migrated,
                notice: None,
            };
        }
    }

    if let Some(name) = preferred_name {
        if let Some(entry) = available
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(name))
            .cloned()
        {
            return ResolvedThemePreference {
                notice: preferred_key
                    .map(|_| format!("{} palette moved to {}", theme_mode_label(mode), entry.name))
                    .or_else(|| {
                        preferred_key.is_none().then(|| {
                            format!(
                                "{} palette migrated to {}",
                                theme_mode_label(mode),
                                entry.name
                            )
                        })
                    }),
                entry,
                migrated: true,
            };
        }
    }

    let entry = default_theme_entry(mode, cx);
    ResolvedThemePreference {
        notice: preferred_key.or(preferred_name).map(|_| {
            format!(
                "{} palette fell back to {}",
                theme_mode_label(mode),
                entry.name
            )
        }),
        entry,
        migrated: preferred_key.is_some() || preferred_name.is_some(),
    }
}

pub(crate) fn resolve_theme_config(
    mode: ThemeMode,
    preferred_key: Option<&str>,
    preferred_name: Option<&str>,
    storage: Option<&StoragePaths>,
    cx: &App,
) -> Rc<ThemeConfig> {
    resolve_theme_preference(mode, preferred_key, preferred_name, storage, cx)
        .entry
        .config
}

pub(crate) fn unique_theme_name(
    preferred_name: &str,
    mode: ThemeMode,
    prefer_mode_variant: bool,
    names_in_use: &mut HashSet<String>,
) -> gpui::SharedString {
    let base = preferred_name.trim();
    let base = if base.is_empty() {
        "Custom Theme".to_string()
    } else {
        base.to_string()
    };
    let mode_variant = theme_name_with_mode(&base, mode);
    let mut stems = Vec::new();

    if prefer_mode_variant {
        stems.push(mode_variant.clone());
        if mode_variant != base {
            stems.push(base.clone());
        }
    } else {
        stems.push(base.clone());
        if mode_variant != base {
            stems.push(mode_variant.clone());
        }
    }

    for stem in &stems {
        if names_in_use.insert(theme_name_key(stem)) {
            return stem.clone().into();
        }
    }

    for stem in &stems {
        let custom = format!("{stem} (Custom)");
        if names_in_use.insert(theme_name_key(&custom)) {
            return custom.into();
        }
    }

    for stem in &stems {
        for index in 2..10_000 {
            let candidate = format!("{stem} (Custom {index})");
            if names_in_use.insert(theme_name_key(&candidate)) {
                return candidate.into();
            }
        }
    }

    format!("{} (Imported)", stems[0]).into()
}

pub(crate) fn apply_resolved_theme_preferences(
    policy: AppearancePolicy,
    light_theme: Rc<ThemeConfig>,
    dark_theme: Rc<ThemeConfig>,
    window: Option<&mut Window>,
    cx: &mut App,
) -> ThemeMode {
    if !cx.has_global::<Theme>() {
        Theme::change(resolve_theme_mode(policy, None, cx), None, cx);
    }

    {
        let theme = Theme::global_mut(cx);
        theme.light_theme = light_theme;
        theme.dark_theme = dark_theme;
    }

    let mode = resolve_theme_mode(policy, window.as_deref(), cx);
    Theme::change(mode, window, cx);
    mode
}

pub(crate) fn resolve_theme_mode(
    policy: AppearancePolicy,
    window: Option<&Window>,
    cx: &App,
) -> ThemeMode {
    match policy {
        AppearancePolicy::System => window
            .map(|window| window.appearance().into())
            .unwrap_or_else(|| cx.window_appearance().into()),
        AppearancePolicy::Light => ThemeMode::Light,
        AppearancePolicy::Dark => ThemeMode::Dark,
    }
}

fn theme_name_with_mode(name: &str, mode: ThemeMode) -> String {
    let suffix = theme_mode_label(mode);
    if name
        .trim()
        .to_lowercase()
        .ends_with(&format!(" {}", suffix.to_lowercase()))
    {
        name.trim().to_string()
    } else {
        format!("{} {}", name.trim(), suffix)
    }
}
