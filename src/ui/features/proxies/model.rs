use std::collections::BTreeSet;

use crate::ui::state::{
    ConfigSource, EndpointFamily, ProxyRunningFilter, TunnelConfig, WgApp,
};

#[derive(Clone)]
struct ProxyNameParts {
    country: Option<String>,
    city: Option<String>,
    protocol: Option<String>,
    sequence: Option<String>,
}

#[derive(Clone)]
pub(super) struct ProxyRowData {
    pub(super) id: u64,
    pub(super) name: String,
    pub(super) name_lower: String,
    pub(super) country: Option<String>,
    pub(super) city: Option<String>,
    pub(super) protocol: Option<String>,
    pub(super) sequence: Option<String>,
    pub(super) endpoint_family: EndpointFamily,
    pub(super) is_running: bool,
    pub(super) source_kind: &'static str,
}

impl ProxyRowData {
    pub(super) fn country_label(&self) -> &str {
        self.country.as_deref().unwrap_or("—")
    }

    pub(super) fn city_label(&self) -> &str {
        self.city.as_deref().unwrap_or("—")
    }

    pub(super) fn protocol_label(&self) -> &str {
        self.protocol.as_deref().unwrap_or("—")
    }

    pub(super) fn sequence_label(&self) -> &str {
        self.sequence.as_deref().unwrap_or("—")
    }

    pub(super) fn location_label(&self) -> String {
        match (self.country.as_deref(), self.city.as_deref()) {
            (Some(country), Some(city)) => format!("{country} / {city}"),
            (Some(country), None) => country.to_string(),
            (None, Some(city)) => city.to_string(),
            (None, None) => "—".to_string(),
        }
    }
}

pub(super) struct ProxiesViewModel {
    pub(super) rows: Vec<ProxyRowData>,
    pub(super) filtered_rows: Vec<ProxyRowData>,
    pub(super) countries: Vec<String>,
    pub(super) cities: Vec<String>,
    pub(super) protocols: Vec<String>,
    pub(super) selected_row: Option<ProxyRowData>,
    pub(super) selected_visible: bool,
}

pub(super) fn build_proxies_view_model(app: &WgApp, query: &str) -> ProxiesViewModel {
    let rows = app
        .configs
        .iter()
        .map(|config| proxy_row_data(config, app.runtime.running_id == Some(config.id)))
        .collect::<Vec<_>>();
    let countries = rows
        .iter()
        .filter_map(|row| row.country.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let cities = rows
        .iter()
        .filter(|row| {
            app.selection
                .proxy_country_filter
                .as_deref()
                .is_none_or(|country| row.country.as_deref() == Some(country))
        })
        .filter_map(|row| row.city.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let protocols = rows
        .iter()
        .filter(|row| {
            app.selection
                .proxy_country_filter
                .as_deref()
                .is_none_or(|country| row.country.as_deref() == Some(country))
                && app
                    .selection
                    .proxy_city_filter
                    .as_deref()
                    .is_none_or(|city| row.city.as_deref() == Some(city))
        })
        .filter_map(|row| row.protocol.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let filtered_rows = rows
        .iter()
        .filter(|row| {
            (query.is_empty() || row.name_lower.contains(query))
                && app
                    .selection
                    .proxy_country_filter
                    .as_deref()
                    .is_none_or(|country| row.country.as_deref() == Some(country))
                && app
                    .selection
                    .proxy_city_filter
                    .as_deref()
                    .is_none_or(|city| row.city.as_deref() == Some(city))
                && app
                    .selection
                    .proxy_protocol_filter
                    .as_deref()
                    .is_none_or(|protocol| row.protocol.as_deref() == Some(protocol))
                && match app.selection.proxy_running_filter {
                    ProxyRunningFilter::All => true,
                    ProxyRunningFilter::Running => row.is_running,
                    ProxyRunningFilter::Idle => !row.is_running,
                }
        })
        .cloned()
        .collect::<Vec<_>>();
    let selected_row = app
        .selection
        .selected_id
        .and_then(|selected_id| rows.iter().find(|row| row.id == selected_id).cloned());
    let selected_visible = app
        .selection
        .selected_id
        .is_some_and(|selected_id| filtered_rows.iter().any(|row| row.id == selected_id));

    ProxiesViewModel {
        rows,
        filtered_rows,
        countries,
        cities,
        protocols,
        selected_row,
        selected_visible,
    }
}

fn proxy_row_data(config: &TunnelConfig, is_running: bool) -> ProxyRowData {
    let parts = parse_proxy_name(&config.name);
    ProxyRowData {
        id: config.id,
        name: config.name.clone(),
        name_lower: config.name_lower.clone(),
        country: parts.country,
        city: parts.city,
        protocol: parts.protocol,
        sequence: parts.sequence,
        endpoint_family: config.endpoint_family,
        is_running,
        source_kind: match config.source {
            ConfigSource::File { .. } => "File",
            ConfigSource::Paste => "Paste",
        },
    }
}

fn parse_proxy_name(name: &str) -> ProxyNameParts {
    let segments = name
        .split('-')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    ProxyNameParts {
        country: segments.first().map(|segment| segment.to_ascii_uppercase()),
        city: segments.get(1).map(|segment| segment.to_ascii_uppercase()),
        protocol: segments.get(2).map(|segment| segment.to_ascii_uppercase()),
        sequence: segments.last().map(|segment| segment.to_string()),
    }
}
