use gpui::SharedString;

use crate::ui::state::RouteFamilyFilter;

use super::data::{RouteMapInventoryGroup, RouteMapInventoryItem};

pub(crate) fn apply_filters(
    groups: &mut [RouteMapInventoryGroup],
    family_filter: RouteFamilyFilter,
    search_query: &str,
) {
    for group in groups {
        group
            .items
            .retain(|item| item_matches(item, family_filter, search_query));
        group.summary = group.items.len().to_string().into();
    }
}

pub(crate) fn resolve_selected_item(
    groups: &[RouteMapInventoryGroup],
    requested_id: Option<&SharedString>,
    explain_match_id: Option<&SharedString>,
) -> Option<RouteMapInventoryItem> {
    requested_id
        .and_then(|id| find_item(groups, id.as_ref()))
        .or_else(|| explain_match_id.and_then(|id| find_item(groups, id.as_ref())))
        .or_else(|| {
            groups
                .iter()
                .flat_map(|group| group.items.iter())
                .next()
                .cloned()
        })
}

fn item_matches(
    item: &RouteMapInventoryItem,
    family_filter: RouteFamilyFilter,
    search_query: &str,
) -> bool {
    let family_match = match family_filter {
        RouteFamilyFilter::All => true,
        _ => item.family.is_none() || item.family == Some(family_filter),
    };
    if !family_match {
        return false;
    }

    let query = search_query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }

    let mut haystack = vec![
        item.title.to_string(),
        item.subtitle.to_string(),
        item.status.label().to_string(),
    ];
    haystack.extend(item.chips.iter().map(|chip| chip.label.to_string()));
    if let Some(route_row) = &item.route_row {
        haystack.push(route_row.destination.to_string());
        haystack.push(route_row.kind.to_string());
        haystack.push(route_row.endpoint.to_string());
        haystack.push(route_row.note.to_string());
    }
    if let Some(endpoint_host) = item.endpoint_host.as_ref() {
        haystack.push(endpoint_host.to_string());
    }

    haystack
        .into_iter()
        .any(|value| value.to_lowercase().contains(&query))
}

fn find_item(groups: &[RouteMapInventoryGroup], id: &str) -> Option<RouteMapInventoryItem> {
    groups
        .iter()
        .flat_map(|group| group.items.iter())
        .find(|item| item.id.as_ref() == id)
        .cloned()
}
