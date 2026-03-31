use std::net::IpAddr;
use std::path::PathBuf;

use gpui::{AppContext, Context, SharedString};
use r_wg::core::config::{self, WireGuardConfig};

use crate::ui::state::{EndpointFamily, WgApp};

struct EndpointFamilyResolutionHint {
    base_family: EndpointFamily,
    pending_hosts: Vec<(String, u16)>,
}

pub(crate) fn endpoint_family_hint_from_config(cfg: &WireGuardConfig) -> EndpointFamily {
    endpoint_family_resolution_hint_from_config(cfg).base_family
}

pub(crate) async fn resolve_endpoint_family_from_text(text: String) -> EndpointFamily {
    let hint = endpoint_family_resolution_hint_from_text(&text);
    if hint.pending_hosts.is_empty() {
        return hint.base_family;
    }
    resolve_endpoint_family(hint.base_family, hint.pending_hosts).await
}

pub(crate) fn schedule_endpoint_family_refresh(
    app: &mut WgApp,
    config_id: u64,
    text: Option<SharedString>,
    storage_path: PathBuf,
    cx: &mut Context<WgApp>,
) {
    if app.selection.endpoint_family_loading.contains(&config_id) {
        return;
    }
    let Some(config) = app.configs.get_mut_by_id(config_id) else {
        return;
    };
    config.endpoint_family = EndpointFamily::Unknown;
    app.selection.endpoint_family_loading.insert(config_id);

    let config_library = app.config_library.clone();
    cx.spawn(async move |view, cx| {
        let refresh_task = cx.background_spawn(async move {
            let text = match text {
                Some(text) => Some(text.to_string()),
                None => config_library.read_config_text(&storage_path).ok(),
            };
            let text = text?;
            Some(resolve_endpoint_family_from_text(text).await)
        });
        let family = refresh_task.await;
        view.update(cx, |this, cx| {
            this.selection.endpoint_family_loading.remove(&config_id);
            let Some(family) = family else {
                return;
            };
            let Some(config) = this.configs.get_mut_by_id(config_id) else {
                return;
            };
            if config.endpoint_family != family {
                config.endpoint_family = family;
                let updated_config = config.clone();
                this.upsert_configs_workspace_library_row(&updated_config, cx);
                cx.notify();
            }
        })
        .ok();
    })
    .detach();
}

fn endpoint_family_resolution_hint_from_text(text: &str) -> EndpointFamilyResolutionHint {
    let parsed = config::parse_config(text);
    let Ok(parsed) = parsed else {
        return EndpointFamilyResolutionHint {
            base_family: EndpointFamily::Unknown,
            pending_hosts: Vec::new(),
        };
    };
    endpoint_family_resolution_hint_from_config(&parsed)
}

fn endpoint_family_resolution_hint_from_config(
    cfg: &WireGuardConfig,
) -> EndpointFamilyResolutionHint {
    let mut has_v4 = false;
    let mut has_v6 = false;
    let mut pending_hosts = Vec::new();

    for peer in &cfg.peers {
        let Some(endpoint) = &peer.endpoint else {
            continue;
        };
        let host = endpoint.host.trim();
        if host.is_empty() {
            continue;
        }
        if let Ok(addr) = host.parse::<IpAddr>() {
            if addr.is_ipv4() {
                has_v4 = true;
            } else {
                has_v6 = true;
            }
            continue;
        }
        if host.contains(':') {
            has_v6 = true;
            continue;
        }

        pending_hosts.push((host.to_string(), endpoint.port));
    }

    let base_family = endpoint_family_from_flags(has_v4, has_v6);
    if base_family == EndpointFamily::Dual {
        pending_hosts.clear();
    }

    EndpointFamilyResolutionHint {
        base_family,
        pending_hosts,
    }
}

async fn resolve_endpoint_family(
    base_family: EndpointFamily,
    pending_hosts: Vec<(String, u16)>,
) -> EndpointFamily {
    if base_family == EndpointFamily::Dual {
        return EndpointFamily::Dual;
    }

    let mut has_v4 = matches!(base_family, EndpointFamily::V4 | EndpointFamily::Dual);
    let mut has_v6 = matches!(base_family, EndpointFamily::V6 | EndpointFamily::Dual);

    for (host, port) in pending_hosts {
        if let Ok(addrs) = tokio::net::lookup_host((host.as_str(), port)).await {
            for addr in addrs {
                if addr.ip().is_ipv4() {
                    has_v4 = true;
                } else {
                    has_v6 = true;
                }
            }
        }
    }

    endpoint_family_from_flags(has_v4, has_v6)
}

fn endpoint_family_from_flags(has_v4: bool, has_v6: bool) -> EndpointFamily {
    match (has_v4, has_v6) {
        (true, true) => EndpointFamily::Dual,
        (true, false) => EndpointFamily::V4,
        (false, true) => EndpointFamily::V6,
        (false, false) => EndpointFamily::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::endpoint_family_hint_from_config;
    use crate::ui::state::EndpointFamily;
    use r_wg::core::config;

    #[test]
    fn sync_hint_uses_only_static_endpoint_shape() {
        let parsed = config::parse_config(
            "[Interface]\nPrivateKey = 0000000000000000000000000000000000000000000000000000000000000000\nAddress = 10.0.0.1/32\n\n[Peer]\nPublicKey = 1111111111111111111111111111111111111111111111111111111111111111\nAllowedIPs = 0.0.0.0/0\nEndpoint = example.com:51820\n",
        )
        .unwrap();

        assert!(endpoint_family_hint_from_config(&parsed) == EndpointFamily::Unknown);
    }

    #[test]
    fn sync_hint_marks_colon_hosts_as_v6_shape() {
        let parsed = config::parse_config(
            "[Interface]\nPrivateKey = 0000000000000000000000000000000000000000000000000000000000000000\nAddress = 10.0.0.1/32\n\n[Peer]\nPublicKey = 1111111111111111111111111111111111111111111111111111111111111111\nAllowedIPs = ::/0\nEndpoint = not-an-ipv6-literal:with-colon:51820\n",
        )
        .unwrap();

        assert!(endpoint_family_hint_from_config(&parsed) == EndpointFamily::V6);
    }
}
