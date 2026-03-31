use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;

mod cache;
mod clipboard;
mod dialogs;
mod draft;
mod endpoint_family;
mod inputs;
mod naming;
mod selection;
mod storage;
mod workspace;

use gpui::{
    div, AppContext, ClipboardItem, Context, Entity, IntoElement, ParentElement, SharedString,
    Styled, Window,
};
use gpui_component::{
    button::{Button, ButtonVariant, ButtonVariants as _},
    dialog::DialogButtonProps,
    input::{InputEvent, InputState, TabSize},
    ActiveTheme as _, WindowExt,
};
use r_wg::core::config;

use super::super::persistence;
use super::super::state::{
    ConfigDraftState, ConfigSource, ConfigsPrimaryPane, ConfigsWorkspace, DraftValidationState,
    EditorOperation, EndpointFamily, LoadedConfigState, PendingDraftAction, SidebarItem,
    TunnelConfig, WgApp,
};
pub(crate) use endpoint_family::{
    endpoint_family_hint_from_config, resolve_endpoint_family_from_text,
};
pub(crate) use naming::reserve_unique_name;

const CONFIG_TEXT_CACHE_LIMIT: usize = 32;
const DRAFT_VALIDATION_DEBOUNCE: Duration = Duration::from_millis(180);

#[derive(Clone, Copy)]
enum DeletePolicy {
    BlockRunning,
    SkipRunning,
}

pub(crate) fn text_hash(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

fn format_delete_status(deleted_names: &[String], skipped_running: usize) -> String {
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
