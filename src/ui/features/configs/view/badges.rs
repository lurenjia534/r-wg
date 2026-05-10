use gpui::ParentElement;
use gpui_component::tag::Tag;
use gpui_component::Sizable as _;

use crate::ui::state::{ConfigSource, EndpointFamily};

pub(super) fn source_tag(source: &ConfigSource) -> Tag {
    match source {
        ConfigSource::File { .. } => Tag::secondary().small().child("Imported"),
        ConfigSource::Paste => Tag::secondary().small().child("Saved"),
    }
}

pub(super) fn endpoint_family_tag(family: EndpointFamily) -> Tag {
    match family {
        EndpointFamily::V4 => Tag::secondary().small().child("IPv4"),
        EndpointFamily::V6 => Tag::secondary().small().child("IPv6"),
        EndpointFamily::Dual => Tag::secondary().small().child("Dual"),
        EndpointFamily::Unknown => Tag::secondary().small().child("Unknown"),
    }
}
