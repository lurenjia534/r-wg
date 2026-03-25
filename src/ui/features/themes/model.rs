use std::rc::Rc;

use gpui::SharedString;
use gpui_component::theme::{ThemeConfig, ThemeMode};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AppearancePolicy {
    #[default]
    System,
    Light,
    Dark,
}

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

    pub(super) fn sort_rank(self) -> u8 {
        match self {
            Self::Approved => 0,
            Self::Experimental => 1,
        }
    }
}

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

    pub(super) fn sort_rank(&self) -> (u8, u8) {
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
