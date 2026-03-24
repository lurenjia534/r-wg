use std::net::IpAddr;

use super::{RoutePlan, RoutePlanExplainResult, RoutePlanGroup, RoutePlanItem};

impl RoutePlan {
    pub fn explain(&self, search_query: &str) -> RoutePlanExplainResult {
        build_explain(&self.inventory_groups, search_query)
    }
}

fn build_explain(groups: &[RoutePlanGroup], search_query: &str) -> RoutePlanExplainResult {
    let query = search_query.trim().to_string();
    if query.is_empty() {
        return RoutePlanExplainResult {
            query,
            headline: "Explain a target".to_string(),
            summary: "Search for an IP, CIDR, or endpoint hostname to see how the current plan resolves it.".to_string(),
            steps: vec![
                "IP queries pick the most specific planned route or bypass rule.".to_string(),
                "Hostname queries match configured peer endpoint hosts and call out when runtime resolution is still needed.".to_string(),
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
                super::util::cidr_contains(target.addr, target.cidr, ip).then_some((item, target.cidr))
            })
            .max_by_key(|(_, cidr)| *cidr)
            .map(|(item, _)| item);

        if let Some(item) = best {
            return RoutePlanExplainResult {
                query: query.clone(),
                headline: format!("{ip} matches {}", item.title),
                summary: item.subtitle.clone(),
                steps: explain_steps_from_item(item, Some(ip)),
                risk: item.inspector.risk_assessment.clone(),
                matched_item_id: Some(item.id.clone()),
            };
        }

        return RoutePlanExplainResult {
            query: query.clone(),
            headline: format!("{ip} has no planned match"),
            summary: "The current config does not advertise a concrete route for this destination.".to_string(),
            steps: vec![
                "No AllowedIPs, DNS host route, or endpoint bypass rule contains this IP.".to_string(),
                "If the expectation differs, inspect the inventory for missing prefixes or family filters.".to_string(),
            ],
            risk: vec![
                "Unmatched traffic follows the platform default routing policy, not the tunnel plan."
                    .to_string(),
            ],
            matched_item_id: None,
        };
    }

    if let Some(item) = items.iter().find(|item| {
        item.endpoint_host
            .as_ref()
            .map(|host| host.eq_ignore_ascii_case(&query))
            .unwrap_or(false)
    }) {
        return RoutePlanExplainResult {
            query: query.clone(),
            headline: format!("{query} is a configured endpoint"),
            summary: item.subtitle.clone(),
            steps: explain_steps_from_item(item, None),
            risk: item.inspector.risk_assessment.clone(),
            matched_item_id: Some(item.id.clone()),
        };
    }

    RoutePlanExplainResult {
        query: query.clone(),
        headline: format!("No direct explanation for {query}"),
        summary: "Only configured endpoint hosts can be explained without runtime DNS resolution.".to_string(),
        steps: vec![
            "Route Map can explain raw IPs, CIDRs, and configured endpoint hostnames.".to_string(),
            "Generic domains need a resolved IP before they can be matched against the current route plan.".to_string(),
        ],
        risk: Vec::new(),
        matched_item_id: None,
    }
}

fn explain_steps_from_item(item: &RoutePlanItem, ip: Option<IpAddr>) -> Vec<String> {
    let mut steps = Vec::new();
    if let Some(ip) = ip {
        steps.push(format!("{ip} matched the most specific visible route."));
    }
    for step in &item.graph_steps {
        match step.note.as_ref() {
            Some(note) => steps.push(format!("{} -> {} ({note})", step.label, step.value)),
            None => steps.push(format!("{} -> {}", step.label, step.value)),
        }
    }
    steps
}
