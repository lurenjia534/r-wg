use std::time::Duration;

use r_wg::backend::wg::PeerStats;
use r_wg::core::config::{self, RouteTable};

pub struct PeerSummary {
    pub peer_count: usize,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub last_handshake: Option<Duration>,
}

pub fn summarize_peers(peers: &[PeerStats]) -> PeerSummary {
    let rx_bytes = peers.iter().map(|peer| peer.rx_bytes).sum();
    let tx_bytes = peers.iter().map(|peer| peer.tx_bytes).sum();
    let last_handshake = peers.iter().filter_map(|peer| peer.last_handshake).min();
    PeerSummary {
        peer_count: peers.len(),
        rx_bytes,
        tx_bytes,
        last_handshake,
    }
}

pub fn format_duration(duration: Duration) -> String {
    let secs = duration.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        let minutes = secs / 60;
        let seconds = secs % 60;
        format!("{minutes}m{seconds}s")
    } else {
        let hours = secs / 3600;
        let minutes = (secs % 3600) / 60;
        format!("{hours}h{minutes}m")
    }
}

pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * KB;
    const GB: f64 = 1024.0 * MB;

    let value = bytes as f64;
    if value >= GB {
        format!("{:.1}GiB", value / GB)
    } else if value >= MB {
        format!("{:.1}MiB", value / MB)
    } else if value >= KB {
        format!("{:.1}KiB", value / KB)
    } else {
        format!("{bytes}B")
    }
}

pub fn format_addresses(interface: &config::InterfaceConfig) -> String {
    if interface.addresses.is_empty() {
        return "None".to_string();
    }
    interface
        .addresses
        .iter()
        .map(|addr| format!("{}/{}", addr.addr, addr.cidr))
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn format_dns(interface: &config::InterfaceConfig) -> String {
    let mut parts = Vec::new();
    if !interface.dns_servers.is_empty() {
        parts.push(
            interface
                .dns_servers
                .iter()
                .map(|ip| ip.to_string())
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    if !interface.dns_search.is_empty() {
        parts.push(interface.dns_search.join(", "));
    }
    if parts.is_empty() {
        "None".to_string()
    } else {
        parts.join(" | ")
    }
}

pub fn format_route_table(table: Option<RouteTable>) -> String {
    match table {
        Some(RouteTable::Auto) => "auto".to_string(),
        Some(RouteTable::Off) => "off".to_string(),
        Some(RouteTable::Id(id)) => format!("id:{id}"),
        None => "main".to_string(),
    }
}

pub fn format_allowed_ips(peers: &[config::PeerConfig]) -> String {
    let mut items = Vec::new();
    for peer in peers {
        for allowed in &peer.allowed_ips {
            items.push(format!("{}/{}", allowed.addr, allowed.cidr));
        }
    }
    if items.is_empty() {
        "None".to_string()
    } else {
        items.join(", ")
    }
}

pub fn sanitize_file_stem(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch);
        } else if ch.is_ascii_whitespace() {
            out.push('_');
        }
    }
    if out.is_empty() {
        "tunnel".to_string()
    } else {
        out
    }
}
