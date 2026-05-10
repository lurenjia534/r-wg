use std::net::IpAddr;

use gpui::SharedString;

use super::data::{RouteMapExplainResult, RouteMapInventoryGroup, RouteMapInventoryItem};

pub(super) fn build_plan_explain(
    groups: &[RouteMapInventoryGroup],
    search_query: &str,
) -> RouteMapExplainResult {
    let query = search_query.trim().to_string();
    if query.is_empty() {
        return RouteMapExplainResult {
            query: query.into(),
            headline: "Explain a target".into(),
            summary:
                "Search for an IP, CIDR, or endpoint hostname to see how the current plan resolves it."
                    .into(),
            steps: vec![
                "IP queries pick the most specific planned route or bypass rule.".into(),
                "Hostname queries match configured peer endpoint hosts and call out when runtime resolution is still needed.".into(),
            ],
            risk: Vec::new(),
            matched_item_id: None,
        };
    }

    let items = groups
        .iter()
        .flat_map(|group| group.items.iter())
        .collect::<Vec<_>>();

    if let Ok(ip) = query.parse::<IpAddr>() {
        let best = items
            .iter()
            .filter_map(|item| {
                let target = item.match_target.as_ref()?;
                cidr_contains(target.addr, target.cidr, ip).then_some((item, target.cidr))
            })
            .max_by_key(|(_, cidr)| *cidr)
            .map(|(item, _)| item);

        if let Some(item) = best {
            return RouteMapExplainResult {
                query: query.clone().into(),
                headline: format!("{ip} matches {}", item.title).into(),
                summary: item.subtitle.clone(),
                steps: explain_steps_from_item(item, Some(ip)),
                risk: item.inspector.risk_assessment.clone(),
                matched_item_id: Some(item.id.clone()),
            };
        }

        return RouteMapExplainResult {
            query: query.clone().into(),
            headline: format!("{ip} has no planned match").into(),
            summary:
                "The current config does not advertise a concrete route for this destination."
                    .into(),
            steps: vec![
                "No AllowedIPs, DNS host route, or endpoint bypass rule contains this IP.".into(),
                "If the expectation differs, inspect the inventory for missing prefixes or family filters.".into(),
            ],
            risk: vec![
                "Unmatched traffic follows the platform default routing policy, not the tunnel plan.".into(),
            ],
            matched_item_id: None,
        };
    }

    if let Some(item) = items.iter().find(|item| {
        item.endpoint_host
            .as_ref()
            .map(|host| host.as_ref().eq_ignore_ascii_case(&query))
            .unwrap_or(false)
    }) {
        return RouteMapExplainResult {
            query: query.clone().into(),
            headline: format!("{query} is a configured endpoint").into(),
            summary: item.subtitle.clone(),
            steps: explain_steps_from_item(item, None),
            risk: item.inspector.risk_assessment.clone(),
            matched_item_id: Some(item.id.clone()),
        };
    }

    RouteMapExplainResult {
        query: query.clone().into(),
        headline: format!("No direct explanation for {query}").into(),
        summary: "Only configured endpoint hosts can be explained without runtime DNS resolution."
            .into(),
        steps: vec![
            "Route Map can explain raw IPs, CIDRs, and configured endpoint hostnames.".into(),
            "Generic domains need a resolved IP before they can be matched against the current route plan.".into(),
        ],
        risk: Vec::new(),
        matched_item_id: None,
    }
}

fn explain_steps_from_item(item: &RouteMapInventoryItem, ip: Option<IpAddr>) -> Vec<SharedString> {
    let mut steps = Vec::new();
    if let Some(ip) = ip {
        steps.push(format!("{ip} matched the most specific visible route.").into());
    }
    for step in &item.graph_steps {
        match step.note.as_ref() {
            Some(note) => steps.push(format!("{} -> {} ({note})", step.label, step.value).into()),
            None => steps.push(format!("{} -> {}", step.label, step.value).into()),
        }
    }
    steps
}

fn cidr_contains(network: IpAddr, cidr: u8, value: IpAddr) -> bool {
    match (network, value) {
        (IpAddr::V4(network), IpAddr::V4(value)) => {
            let mask = if cidr == 0 {
                0
            } else {
                u32::MAX << (32 - cidr)
            };
            (u32::from(network) & mask) == (u32::from(value) & mask)
        }
        (IpAddr::V6(network), IpAddr::V6(value)) => {
            let mask = if cidr == 0 {
                0
            } else {
                u128::MAX << (128 - cidr)
            };
            (u128::from_be_bytes(network.octets()) & mask)
                == (u128::from_be_bytes(value.octets()) & mask)
        }
        _ => false,
    }
}
