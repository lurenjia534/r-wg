use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::LazyLock;

use gpui::{App, SharedString, Window};
use gpui_component::theme::{Theme, ThemeConfig, ThemeMode, ThemeRegistry, ThemeSet};

use super::persistence::StoragePaths;

const THEMES_DIR_NAME: &str = "themes";
const LEGACY_CURATED_THEMES_FILE_NAME: &str = "r-wg-curated.json";

struct BundledThemeFile {
    file_name: &'static str,
    contents: &'static str,
}

const BUNDLED_THEME_FILES: &[BundledThemeFile] = &[
    BundledThemeFile {
        file_name: "ayu.json",
        contents: include_str!("../../assets/themes/ayu.json"),
    },
    BundledThemeFile {
        file_name: "catppuccin.json",
        contents: include_str!("../../assets/themes/catppuccin.json"),
    },
    BundledThemeFile {
        file_name: "flexoki.json",
        contents: include_str!("../../assets/themes/flexoki.json"),
    },
    BundledThemeFile {
        file_name: "gruvbox.json",
        contents: include_str!("../../assets/themes/gruvbox.json"),
    },
    BundledThemeFile {
        file_name: "solarized.json",
        contents: include_str!("../../assets/themes/solarized.json"),
    },
    BundledThemeFile {
        file_name: "tokyonight.json",
        contents: include_str!("../../assets/themes/tokyonight.json"),
    },
];

static BUNDLED_THEMES: LazyLock<Vec<ThemeConfig>> = LazyLock::new(|| {
    BUNDLED_THEME_FILES
        .iter()
        .flat_map(|file| parse_theme_file(file.file_name, file.contents).into_iter())
        .collect()
});

pub(crate) fn ensure_theme_registry(
    storage: &StoragePaths,
    cx: &mut App,
) -> Result<PathBuf, String> {
    let themes_dir = storage.root.join(THEMES_DIR_NAME);
    std::fs::create_dir_all(&themes_dir)
        .map_err(|err| format!("Create themes dir failed: {err}"))?;

    // Migrate away from the older hand-written seed file now that upstream theme files are vendored.
    let legacy_curated_path = themes_dir.join(LEGACY_CURATED_THEMES_FILE_NAME);
    if let Err(err) = remove_if_exists(&legacy_curated_path) {
        return Err(format!("Remove legacy curated themes failed: {err}"));
    }

    sync_bundled_theme_files(&themes_dir)?;

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

fn sync_bundled_theme_files(themes_dir: &Path) -> Result<(), String> {
    for file in BUNDLED_THEME_FILES {
        let path = themes_dir.join(file.file_name);
        let needs_write = match std::fs::read_to_string(&path) {
            Ok(existing) => existing != file.contents,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => true,
            Err(err) => {
                return Err(format!(
                    "Read bundled theme {} failed: {err}",
                    path.display()
                ))
            }
        };

        if needs_write {
            std::fs::write(&path, file.contents)
                .map_err(|err| format!("Write bundled theme {} failed: {err}", path.display()))?;
        }
    }
    Ok(())
}

fn remove_if_exists(path: &Path) -> Result<(), std::io::Error> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

fn parse_theme_file(file_name: &str, contents: &str) -> Vec<ThemeConfig> {
    serde_json::from_str::<ThemeSet>(contents)
        .unwrap_or_else(|err| panic!("bundled theme {file_name} should parse: {err}"))
        .themes
}
