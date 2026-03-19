use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::LazyLock;

use gpui::{App, SharedString, Window};
use gpui_component::theme::{Theme, ThemeConfig, ThemeMode, ThemeRegistry, ThemeSet};

use super::persistence::StoragePaths;

const THEMES_DIR_NAME: &str = "themes";
const CURATED_THEMES_FILE_NAME: &str = "r-wg-curated.json";
const CURATED_THEMES_JSON: &str = r##"{
  "$schema": "https://github.com/longbridge/gpui-component/raw/refs/heads/main/.theme-schema.json",
  "name": "r-wg Curated",
  "author": "r-wg",
  "themes": [
    {
      "name": "Signal Light",
      "mode": "light",
      "colors": {
        "background": "#f8fafc",
        "border": "#d7e0ea",
        "foreground": "#0f172a",
        "muted.background": "#eef3f7",
        "muted.foreground": "#64748b",
        "accent.background": "#dbeafe",
        "accent.foreground": "#0f172a",
        "group_box.background": "#eef4f9",
        "group_box.foreground": "#0f172a",
        "input.border": "#cbd5e1",
        "list.active.background": "#60a5fa26",
        "list.active.border": "#60a5fa",
        "list.even.background": "#f2f6fa",
        "list.head.background": "#f3f7fb",
        "list.hover.background": "#edf4fb",
        "popover.background": "#ffffff",
        "popover.foreground": "#0f172a",
        "primary.background": "#0f172a",
        "primary.foreground": "#f8fafc",
        "primary.hover.background": "#1e293b",
        "secondary.background": "#eaf1f7",
        "secondary.foreground": "#0f172a",
        "secondary.hover.background": "#dde7f1",
        "sidebar.background": "#f3f7fb",
        "sidebar.border": "#d7e0ea",
        "sidebar.foreground": "#0f172a",
        "sidebar.primary.background": "#0f172a",
        "sidebar.primary.foreground": "#f8fafc",
        "success.background": "#16a34a",
        "success.foreground": "#f8fafc",
        "danger.background": "#dc2626",
        "danger.foreground": "#fef2f2",
        "info.background": "#0284c7",
        "info.foreground": "#eff6ff",
        "warning.background": "#d97706",
        "warning.foreground": "#fff7ed",
        "table.active.background": "#60a5fa26",
        "table.active.border": "#60a5fa",
        "table.even.background": "#f2f6fa",
        "table.head.background": "#f3f7fb",
        "table.head.foreground": "#64748b",
        "table.hover.background": "#edf4fb",
        "table.row.border": "#d7e0eab3",
        "tiles.background": "#f3f7fb",
        "title_bar.background": "#f5f8fb",
        "title_bar.border": "#d7e0ea"
      }
    },
    {
      "name": "Paper Light",
      "mode": "light",
      "colors": {
        "background": "#fffdf7",
        "border": "#e8dccb",
        "foreground": "#3b322c",
        "muted.background": "#f8f0e5",
        "muted.foreground": "#7b6d62",
        "accent.background": "#f1e4d1",
        "accent.foreground": "#3b322c",
        "group_box.background": "#faf4ea",
        "group_box.foreground": "#3b322c",
        "input.border": "#dccdb8",
        "list.active.background": "#d6a15c26",
        "list.active.border": "#d6a15c",
        "list.even.background": "#fcf7ef",
        "list.head.background": "#faf1e5",
        "list.hover.background": "#f8f0e5",
        "popover.background": "#fffdf7",
        "popover.foreground": "#3b322c",
        "primary.background": "#4b3f35",
        "primary.foreground": "#fffdf7",
        "primary.hover.background": "#5b4d41",
        "secondary.background": "#f6ecdf",
        "secondary.foreground": "#3b322c",
        "secondary.hover.background": "#efe1cf",
        "sidebar.background": "#fbf5ec",
        "sidebar.border": "#e8dccb",
        "sidebar.foreground": "#3b322c",
        "sidebar.primary.background": "#4b3f35",
        "sidebar.primary.foreground": "#fffdf7",
        "success.background": "#5f8f3e",
        "success.foreground": "#f7fee7",
        "danger.background": "#c96b4b",
        "danger.foreground": "#fff1ed",
        "info.background": "#4d87a7",
        "info.foreground": "#eff6ff",
        "warning.background": "#c2882f",
        "warning.foreground": "#fffbeb",
        "table.active.background": "#d6a15c26",
        "table.active.border": "#d6a15c",
        "table.even.background": "#fcf7ef",
        "table.head.background": "#faf1e5",
        "table.head.foreground": "#7b6d62",
        "table.hover.background": "#f8f0e5",
        "table.row.border": "#e8dccbb3",
        "tiles.background": "#faf4ea",
        "title_bar.background": "#fdf7ee",
        "title_bar.border": "#e8dccb"
      }
    },
    {
      "name": "Network Dark",
      "mode": "dark",
      "colors": {
        "background": "#0b1220",
        "border": "#243247",
        "foreground": "#e5eef8",
        "muted.background": "#101a2b",
        "muted.foreground": "#8da2ba",
        "accent.background": "#16253d",
        "accent.foreground": "#e5eef8",
        "group_box.background": "#101a2c",
        "group_box.foreground": "#e5eef8",
        "input.border": "#32445d",
        "list.active.background": "#3b82f633",
        "list.active.border": "#60a5fa",
        "list.even.background": "#0f1728",
        "list.head.background": "#101a2c",
        "list.hover.background": "#132038",
        "popover.background": "#0f1728",
        "popover.foreground": "#e5eef8",
        "primary.background": "#dbeafe",
        "primary.foreground": "#0b1220",
        "primary.hover.background": "#bfdbfe",
        "secondary.background": "#132033",
        "secondary.foreground": "#e5eef8",
        "secondary.hover.background": "#16263e",
        "sidebar.background": "#0f1728",
        "sidebar.border": "#243247",
        "sidebar.foreground": "#dbe6f3",
        "sidebar.primary.background": "#dbeafe",
        "sidebar.primary.foreground": "#0b1220",
        "success.background": "#22c55e",
        "success.foreground": "#052e16",
        "danger.background": "#ef4444",
        "danger.foreground": "#fef2f2",
        "info.background": "#38bdf8",
        "info.foreground": "#082f49",
        "warning.background": "#f59e0b",
        "warning.foreground": "#431407",
        "table.active.background": "#3b82f633",
        "table.active.border": "#60a5fa",
        "table.even.background": "#0f1728",
        "table.head.background": "#101a2c",
        "table.head.foreground": "#8da2ba",
        "table.hover.background": "#132038",
        "table.row.border": "#243247cc",
        "tiles.background": "#101a2c",
        "title_bar.background": "#0d1526",
        "title_bar.border": "#243247"
      }
    },
    {
      "name": "Terminal Dark",
      "mode": "dark",
      "colors": {
        "background": "#111315",
        "border": "#2a2e32",
        "foreground": "#e6e7e8",
        "muted.background": "#171b1f",
        "muted.foreground": "#98a1a8",
        "accent.background": "#1c2427",
        "accent.foreground": "#d9f99d",
        "group_box.background": "#151b1f",
        "group_box.foreground": "#e6e7e8",
        "input.border": "#3a4248",
        "list.active.background": "#84cc1630",
        "list.active.border": "#84cc16",
        "list.even.background": "#14181b",
        "list.head.background": "#171b1f",
        "list.hover.background": "#1b2126",
        "popover.background": "#14181b",
        "popover.foreground": "#e6e7e8",
        "primary.background": "#8bd450",
        "primary.foreground": "#0d1113",
        "primary.hover.background": "#a3e56c",
        "secondary.background": "#1a1f23",
        "secondary.foreground": "#e6e7e8",
        "secondary.hover.background": "#20262b",
        "sidebar.background": "#14181b",
        "sidebar.border": "#2a2e32",
        "sidebar.foreground": "#d5d7da",
        "sidebar.primary.background": "#8bd450",
        "sidebar.primary.foreground": "#0d1113",
        "success.background": "#4ade80",
        "success.foreground": "#052e16",
        "danger.background": "#f87171",
        "danger.foreground": "#450a0a",
        "info.background": "#38bdf8",
        "info.foreground": "#082f49",
        "warning.background": "#fbbf24",
        "warning.foreground": "#422006",
        "table.active.background": "#84cc1630",
        "table.active.border": "#84cc16",
        "table.even.background": "#14181b",
        "table.head.background": "#171b1f",
        "table.head.foreground": "#98a1a8",
        "table.hover.background": "#1b2126",
        "table.row.border": "#2a2e32cc",
        "tiles.background": "#151b1f",
        "title_bar.background": "#121618",
        "title_bar.border": "#2a2e32"
      }
    }
  ]
}"##;

static BUNDLED_THEMES: LazyLock<Vec<ThemeConfig>> = LazyLock::new(|| {
    serde_json::from_str::<ThemeSet>(CURATED_THEMES_JSON)
        .expect("curated themes should parse")
        .themes
});

pub(crate) fn ensure_theme_registry(
    storage: &StoragePaths,
    cx: &mut App,
) -> Result<PathBuf, String> {
    let themes_dir = storage.root.join(THEMES_DIR_NAME);
    std::fs::create_dir_all(&themes_dir)
        .map_err(|err| format!("Create themes dir failed: {err}"))?;

    let curated_path = themes_dir.join(CURATED_THEMES_FILE_NAME);
    if !curated_path.exists() {
        std::fs::write(&curated_path, CURATED_THEMES_JSON)
            .map_err(|err| format!("Write curated themes failed: {err}"))?;
    }

    ThemeRegistry::watch_dir(themes_dir.clone(), cx, |_cx| {})
        .map_err(|err| format!("Watch themes dir failed: {err}"))?;

    Ok(themes_dir)
}

pub(crate) fn available_themes(mode: ThemeMode, cx: &App) -> Vec<Rc<ThemeConfig>> {
    let mut themes = HashMap::<String, Rc<ThemeConfig>>::new();

    for theme in BUNDLED_THEMES.iter().filter(|theme| theme.mode == mode) {
        themes.insert(theme.name.to_lowercase(), Rc::new(theme.clone()));
    }

    for theme in ThemeRegistry::global(cx)
        .themes()
        .values()
        .filter(|theme| theme.mode == mode)
    {
        themes.insert(theme.name.to_lowercase(), theme.clone());
    }

    let mut themes = themes.into_values().collect::<Vec<_>>();
    themes.sort_by(|a, b| {
        b.is_default
            .cmp(&a.is_default)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    themes
}

pub(crate) fn resolve_theme_config(
    mode: ThemeMode,
    preferred_name: Option<&str>,
    cx: &App,
) -> Rc<ThemeConfig> {
    if let Some(name) = preferred_name {
        if let Some(theme) = available_themes(mode, cx)
            .into_iter()
            .find(|theme| theme.name.eq_ignore_ascii_case(name))
        {
            return theme;
        }
    }

    if mode.is_dark() {
        ThemeRegistry::global(cx).default_dark_theme().clone()
    } else {
        ThemeRegistry::global(cx).default_light_theme().clone()
    }
}

pub(crate) fn resolved_theme_name(
    mode: ThemeMode,
    preferred_name: Option<&str>,
    cx: &App,
) -> SharedString {
    resolve_theme_config(mode, preferred_name, cx).name.clone()
}

pub(crate) fn apply_theme_preferences(
    mode: ThemeMode,
    light_name: Option<&str>,
    dark_name: Option<&str>,
    window: Option<&mut Window>,
    cx: &mut App,
) {
    if !cx.has_global::<Theme>() {
        Theme::change(mode, None, cx);
    }

    let light_theme = resolve_theme_config(ThemeMode::Light, light_name, cx);
    let dark_theme = resolve_theme_config(ThemeMode::Dark, dark_name, cx);

    {
        let theme = Theme::global_mut(cx);
        theme.light_theme = light_theme;
        theme.dark_theme = dark_theme;
    }

    Theme::change(mode, window, cx);
}
