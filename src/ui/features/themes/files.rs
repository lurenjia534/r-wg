use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use gpui::App;
use gpui_component::theme::{ThemeConfig, ThemeRegistry, ThemeSet};
use serde_json::{Map, Value};

use crate::ui::format::sanitize_file_stem;
use crate::ui::persistence::StoragePaths;

use super::catalog::{theme_name_counts, theme_name_key};
use super::resolver::unique_theme_name;

const THEMES_DIR_NAME: &str = "themes";
const LEGACY_CURATED_THEMES_FILE_NAME: &str = "r-wg-curated.json";
const THEME_SCHEMA_URL: &str =
    "https://github.com/longbridge/gpui-component/raw/refs/heads/main/.theme-schema.json";

pub(super) struct BundledThemeFile {
    pub(super) file_name: &'static str,
    pub(super) contents: &'static str,
}

const BUNDLED_THEME_FILES: &[BundledThemeFile] = &[
    BundledThemeFile {
        file_name: "ayu.json",
        contents: include_str!("../../../../assets/themes/ayu.json"),
    },
    BundledThemeFile {
        file_name: "catppuccin.json",
        contents: include_str!("../../../../assets/themes/catppuccin.json"),
    },
    BundledThemeFile {
        file_name: "flexoki.json",
        contents: include_str!("../../../../assets/themes/flexoki.json"),
    },
    BundledThemeFile {
        file_name: "gruvbox.json",
        contents: include_str!("../../../../assets/themes/gruvbox.json"),
    },
    BundledThemeFile {
        file_name: "solarized.json",
        contents: include_str!("../../../../assets/themes/solarized.json"),
    },
    BundledThemeFile {
        file_name: "tokyonight.json",
        contents: include_str!("../../../../assets/themes/tokyonight.json"),
    },
];

static BUNDLED_THEME_FILE_NAMES: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    BUNDLED_THEME_FILES
        .iter()
        .map(|file| file.file_name)
        .collect()
});

pub(crate) fn ensure_theme_registry(
    storage: &StoragePaths,
    cx: &mut App,
) -> Result<PathBuf, String> {
    let themes_dir = ensure_themes_dir(storage)?;

    ThemeRegistry::watch_dir(themes_dir.clone(), cx, |_cx| {})
        .map_err(|err| format!("Watch themes dir failed: {err}"))?;

    Ok(themes_dir)
}

pub(crate) fn themes_dir(storage: &StoragePaths) -> PathBuf {
    storage.root.join(THEMES_DIR_NAME)
}

pub(crate) fn ensure_themes_dir(storage: &StoragePaths) -> Result<PathBuf, String> {
    let themes_dir = themes_dir(storage);
    std::fs::create_dir_all(&themes_dir)
        .map_err(|err| format!("Create themes dir failed: {err}"))?;

    let legacy_curated_path = themes_dir.join(LEGACY_CURATED_THEMES_FILE_NAME);
    if let Err(err) = remove_if_exists(&legacy_curated_path) {
        return Err(format!("Remove legacy curated themes failed: {err}"));
    }

    sync_bundled_theme_files(&themes_dir)?;
    Ok(themes_dir)
}

pub(crate) fn restore_curated_themes(storage: &StoragePaths) -> Result<PathBuf, String> {
    ensure_themes_dir(storage)
}

pub(crate) fn import_theme_file(
    source_path: &Path,
    storage: &StoragePaths,
    names_in_use: &mut HashSet<String>,
) -> Result<ImportedThemeSet, String> {
    let contents = std::fs::read_to_string(source_path)
        .map_err(|err| format!("Read theme {} failed: {err}", source_path.display()))?;
    let theme_set = serde_json::from_str::<ThemeSet>(&contents)
        .map_err(|err| format!("Parse theme {} failed: {err}", source_path.display()))?;
    if theme_set.themes.is_empty() {
        return Err(format!(
            "Theme file {} has no themes",
            source_path.display()
        ));
    }

    let base_name = source_path
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("custom-theme");
    let theme_set = sanitize_theme_set_with_inventory(theme_set, names_in_use);
    let path = write_theme_set(storage, base_name, &theme_set)?;
    Ok(ImportedThemeSet { path, theme_set })
}

pub(crate) fn write_theme_set(
    storage: &StoragePaths,
    file_stem: &str,
    theme_set: &ThemeSet,
) -> Result<PathBuf, String> {
    let themes_dir = ensure_themes_dir(storage)?;
    let path = unique_theme_file_path(&themes_dir, file_stem);
    let data = encode_theme_set(theme_set)?;
    std::fs::write(&path, data)
        .map_err(|err| format!("Write theme {} failed: {err}", path.display()))?;
    Ok(path)
}

pub(crate) fn build_theme_template(
    light_theme: &ThemeConfig,
    dark_theme: &ThemeConfig,
) -> ThemeSet {
    let mut light_theme = sanitize_theme_config(light_theme.clone());
    let mut dark_theme = sanitize_theme_config(dark_theme.clone());
    light_theme.name = "Custom Light".into();
    dark_theme.name = "Custom Dark".into();

    ThemeSet {
        name: "r-wg Custom Theme".into(),
        author: Some("r-wg".into()),
        url: None,
        themes: vec![light_theme, dark_theme],
    }
}

pub(crate) struct ImportedThemeSet {
    pub(crate) path: PathBuf,
    pub(crate) theme_set: ThemeSet,
}

pub(crate) fn theme_name_inventory(storage: Option<&StoragePaths>, cx: &App) -> HashSet<String> {
    theme_name_counts(storage, cx).into_keys().collect()
}

pub(crate) fn sanitize_theme_set_with_inventory(
    theme_set: ThemeSet,
    names_in_use: &mut HashSet<String>,
) -> ThemeSet {
    let mut theme_set = sanitize_theme_set(theme_set);
    let duplicate_names =
        theme_set
            .themes
            .iter()
            .fold(std::collections::HashMap::new(), |mut counts, theme| {
                *counts.entry(theme_name_key(&theme.name)).or_insert(0usize) += 1;
                counts
            });
    for theme in &mut theme_set.themes {
        let prefer_mode_variant = duplicate_names
            .get(&theme_name_key(&theme.name))
            .copied()
            .unwrap_or_default()
            > 1;
        theme.name = unique_theme_name(&theme.name, theme.mode, prefer_mode_variant, names_in_use);
    }
    theme_set
}

pub(super) fn bundled_theme_files() -> &'static [BundledThemeFile] {
    BUNDLED_THEME_FILES
}

pub(super) fn is_bundled_theme_file(file_name: &str) -> bool {
    BUNDLED_THEME_FILE_NAMES.contains(file_name)
}

fn sanitize_theme_set(theme_set: ThemeSet) -> ThemeSet {
    ThemeSet {
        name: theme_set.name,
        author: theme_set.author,
        url: theme_set.url,
        themes: theme_set
            .themes
            .into_iter()
            .map(sanitize_theme_config)
            .collect(),
    }
}

fn sanitize_theme_config(mut config: ThemeConfig) -> ThemeConfig {
    config.is_default = false;
    config.font_size = None;
    config.font_family = None;
    config.mono_font_family = None;
    config.mono_font_size = None;
    config.radius = None;
    config.radius_lg = None;
    config.shadow = None;
    config
}

fn encode_theme_set(theme_set: &ThemeSet) -> Result<Vec<u8>, String> {
    let mut value =
        serde_json::to_value(theme_set).map_err(|err| format!("Serialize theme failed: {err}"))?;
    if let Value::Object(ref mut object) = value {
        object.insert(
            "$schema".to_string(),
            Value::String(THEME_SCHEMA_URL.to_string()),
        );
    } else {
        let mut object = Map::new();
        object.insert(
            "$schema".to_string(),
            Value::String(THEME_SCHEMA_URL.to_string()),
        );
        object.insert("themes".to_string(), value);
        value = Value::Object(object);
    }

    serde_json::to_vec_pretty(&value).map_err(|err| format!("Encode theme failed: {err}"))
}

fn unique_theme_file_path(themes_dir: &Path, file_stem: &str) -> PathBuf {
    let file_stem = sanitize_file_stem(file_stem);
    let direct = themes_dir.join(format!("{file_stem}.json"));
    if !direct.exists() {
        return direct;
    }

    for index in 2..10_000 {
        let candidate = themes_dir.join(format!("{file_stem}-{index}.json"));
        if !candidate.exists() {
            return candidate;
        }
    }

    themes_dir.join(format!("{file_stem}-copy.json"))
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
