use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

mod cache;
mod clipboard;
mod dialogs;
mod endpoint_family;
mod naming;
mod selection;
mod storage;
mod workspace;

use super::super::state::{
    ConfigDraftState, EditorOperation, PendingDraftAction, SidebarItem, TunnelConfig, WgApp,
};
pub(crate) use endpoint_family::endpoint_family_hint_from_config;
use gpui::{AppContext, Context, SharedString, Window};

const CONFIG_TEXT_CACHE_LIMIT: usize = 32;

pub(crate) fn text_hash(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}
