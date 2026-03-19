use gpui::*;
use gpui_component::{tag::Tag, Sizable as _};

use super::super::state::{BackendDiagnostic, BackendHealth};

pub(crate) fn backend_status_badge(diagnostic: &BackendDiagnostic) -> Tag {
    backend_status_tag(diagnostic, diagnostic.badge_label())
}

pub(crate) fn backend_status_tag(
    diagnostic: &BackendDiagnostic,
    label: impl Into<SharedString>,
) -> Tag {
    let label = label.into();
    match diagnostic.health {
        BackendHealth::Running => Tag::success().small().rounded_full().child(label),
        BackendHealth::Checking | BackendHealth::Working { .. } => {
            Tag::info().small().rounded_full().child(label)
        }
        BackendHealth::AccessDenied
        | BackendHealth::VersionMismatch { .. }
        | BackendHealth::Unreachable => Tag::danger().small().rounded_full().child(label),
        BackendHealth::Installed | BackendHealth::NotInstalled => {
            Tag::warning().small().rounded_full().child(label)
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        BackendHealth::Unsupported => Tag::secondary().small().rounded_full().child(label),
        BackendHealth::Unknown => Tag::secondary().small().rounded_full().child(label),
    }
}
