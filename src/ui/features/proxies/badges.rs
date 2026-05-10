use gpui::ParentElement as _;
use gpui_component::tag::Tag;
use gpui_component::Sizable as _;

use crate::ui::state::EndpointFamily;

pub(super) fn endpoint_family_tag(family: EndpointFamily) -> Option<Tag> {
    Some(match family {
        EndpointFamily::V4 => Tag::secondary().small().child("IPv4"),
        EndpointFamily::V6 => Tag::info().small().child("IPv6"),
        EndpointFamily::Dual => Tag::warning().small().child("Dual"),
        EndpointFamily::Unknown => return None,
    })
}
