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

use gpui::{AppContext, Context, SharedString, Window};
use super::super::state::{
    ConfigDraftState, EditorOperation, PendingDraftAction, SidebarItem, TunnelConfig, WgApp,
};
pub(crate) use endpoint_family::{
    endpoint_family_hint_from_config,
};
pub(crate) use naming::reserve_unique_name;

const CONFIG_TEXT_CACHE_LIMIT: usize = 32;

pub(crate) fn text_hash(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

pub(crate) fn format_delete_status(deleted_names: &[String], skipped_running: usize) -> String {
    let deleted_count = deleted_names.len();
    if deleted_count == 0 && skipped_running > 0 {
        if skipped_running == 1 {
            return "Skipped 1 running config".to_string();
        }
        return format!("Skipped {skipped_running} running configs");
    }
    if deleted_count == 1 && skipped_running == 0 {
        return format!("Deleted {}", deleted_names[0]);
    }
    let config_word = if deleted_count == 1 {
        "config"
    } else {
        "configs"
    };
    if skipped_running > 0 {
        return format!("Deleted {deleted_count} {config_word}, skipped {skipped_running} running");
    }
    format!("Deleted {deleted_count} {config_word}")
}
