use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::LazyLock;

use gpui::{App, SharedString, Window};
use gpui_component::theme::{Theme, ThemeConfig, ThemeMode, ThemeRegistry, ThemeSet};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::format::sanitize_file_stem;
use super::persistence::StoragePaths;

const THEMES_DIR_NAME: &str = "themes";
const LEGACY_CURATED_THEMES_FILE_NAME: &str = "r-wg-curated.json";
const THEME_SCHEMA_URL: &str =
    "https://github.com/longbridge/gpui-component/raw/refs/heads/main/.theme-schema.json";

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

static BUNDLED_THEME_FILE_NAMES: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    BUNDLED_THEME_FILES
        .iter()
        .map(|file| file.file_name)
        .collect()
});

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AppearancePolicy {
    #[default]
    System,
    Light,
    Dark,
}

impl AppearancePolicy {}

impl From<ThemeMode> for AppearancePolicy {
    fn from(value: ThemeMode) -> Self {
        match value {
            ThemeMode::Light => Self::Light,
            ThemeMode::Dark => Self::Dark,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) enum ThemeCollection {
    Builtin,
    Curated,
    Custom,
}

impl ThemeCollection {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Builtin => "Built-in",
            Self::Curated => "Curated",
            Self::Custom => "Custom",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) enum ThemeApproval {
    Approved,
    Experimental,
}

impl ThemeApproval {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Approved => "Approved",
            Self::Experimental => "Experimental",
        }
    }

    fn sort_rank(self) -> u8 {
        match self {
            Self::Approved => 0,
            Self::Experimental => 1,
        }
    }
}

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

#[derive(Clone)]
pub(crate) struct ThemeCatalogEntry {
    pub(crate) key: SharedString,
    pub(crate) name: SharedString,
    pub(crate) collection: ThemeCollection,
    pub(crate) approval: Option<ThemeApproval>,
    pub(crate) name_conflict: bool,
    pub(crate) tags: Vec<SharedString>,
    pub(crate) source_label: SharedString,
    pub(crate) config: Rc<ThemeConfig>,
}

impl ThemeCatalogEntry {
    pub(crate) fn badge_label(&self) -> Option<&'static str> {
        self.approval.map(ThemeApproval::label)
    }

    pub(crate) fn conflict_label(&self) -> Option<&'static str> {
        self.name_conflict.then_some("Name Conflict")
    }

    pub(crate) fn menu_group_label(&self) -> &'static str {
        match self.collection {
            ThemeCollection::Builtin => "Default",
            ThemeCollection::Curated => {
                if self.approval == Some(ThemeApproval::Approved) {
                    "Recommended"
                } else {
                    "More Themes"
                }
            }
            ThemeCollection::Custom => "Custom",
        }
    }

    fn sort_rank(&self) -> (u8, u8) {
        match self.collection {
            ThemeCollection::Builtin => (0, 0),
            ThemeCollection::Curated => (
                1,
                self.approval
                    .map(ThemeApproval::sort_rank)
                    .unwrap_or(ThemeApproval::Experimental.sort_rank()),
            ),
            ThemeCollection::Custom => (2, 0),
        }
    }

    pub(crate) fn menu_label(&self) -> SharedString {
        let mut parts = vec![self.name.to_string()];
        if let Some(label) = self.conflict_label() {
            parts.push(label.to_string());
        }
        if self.collection == ThemeCollection::Custom {
            parts.push(self.source_label.to_string());
        }
        parts.join(" · ").into()
    }
}

#[derive(Clone)]
pub(crate) struct ResolvedThemePreference {
    pub(crate) entry: ThemeCatalogEntry,
    pub(crate) migrated: bool,
    pub(crate) notice: Option<String>,
}

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

    // Migrate away from the older hand-written seed file now that upstream theme files are vendored.
    let legacy_curated_path = themes_dir.join(LEGACY_CURATED_THEMES_FILE_NAME);
    if let Err(err) = remove_if_exists(&legacy_curated_path) {
        return Err(format!("Remove legacy curated themes failed: {err}"));
    }

    sync_bundled_theme_files(&themes_dir)?;
    Ok(themes_dir)
}

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

pub(crate) fn theme_name_inventory(storage: Option<&StoragePaths>, cx: &App) -> HashSet<String> {
    theme_name_counts(storage, cx).into_keys().collect()
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
) -> SharedString {
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

pub(crate) fn sanitize_theme_set(theme_set: ThemeSet) -> ThemeSet {
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

pub(crate) fn sanitize_theme_set_with_inventory(
    theme_set: ThemeSet,
    names_in_use: &mut HashSet<String>,
) -> ThemeSet {
    let mut theme_set = sanitize_theme_set(theme_set);
    let duplicate_names = theme_set
        .themes
        .iter()
        .fold(HashMap::new(), |mut counts, theme| {
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
    for file in BUNDLED_THEME_FILES {
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
            || BUNDLED_THEME_FILE_NAMES.contains(file_name)
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

fn default_theme_entry(mode: ThemeMode, cx: &App) -> ThemeCatalogEntry {
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

fn theme_name_counts(storage: Option<&StoragePaths>, cx: &App) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for theme in collect_available_themes(mode, storage, cx) {
            *counts.entry(theme_name_key(&theme.name)).or_insert(0usize) += 1;
        }
    }
    counts
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
        "{}:{}#{}",
        match collection {
            ThemeCollection::Builtin => "builtin",
            ThemeCollection::Curated => "curated",
            ThemeCollection::Custom => "custom",
        },
        sanitize_key(file_name),
        format!("{}-{}", theme_mode_key(mode), sanitize_key(name))
    )
    .into()
}

fn sanitize_key(value: &str) -> String {
    let value = value.trim().to_lowercase();
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else if ch == '-' || ch == '_' || ch == '.' {
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

fn theme_mode_label(mode: ThemeMode) -> &'static str {
    match mode {
        ThemeMode::Light => "Light",
        ThemeMode::Dark => "Dark",
    }
}

fn theme_name_key(name: &str) -> String {
    name.trim().to_lowercase()
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
