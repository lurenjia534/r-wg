mod catalog;
mod controller;
mod files;
mod lint;
mod model;
mod resolver;
mod view;

pub(crate) use catalog::available_themes;
pub(crate) use files::{
    build_theme_template, ensure_theme_registry, ensure_themes_dir, import_theme_file,
    restore_curated_themes, sanitize_theme_set_with_inventory, theme_name_inventory,
    write_theme_set,
};
pub(crate) use lint::{
    lint_theme_config, lint_theme_set, ThemeLintCounts, ThemeLintItem, ThemeLintSeverity,
};
pub(crate) use model::{AppearancePolicy, ThemeCatalogEntry};
pub(crate) use resolver::{
    apply_resolved_theme_preferences, resolve_theme_config, resolve_theme_preference,
};
pub(crate) use view::theme_settings_group;
