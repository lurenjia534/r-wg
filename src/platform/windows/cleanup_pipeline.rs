use super::addresses::delete_unicast_address;
use super::dns::cleanup_dns;
use super::error::NetworkError;
use super::firewall::cleanup_dns_guard;
use super::metrics::restore_interface_metric;
use super::nrpt::cleanup_nrpt_guard;
use super::recovery::clear_persisted_apply_report;
use super::report::AppliedNetworkState;
use super::routes::delete_route;
use crate::log::events::{dns as log_dns, net as log_net};

/// Roll back Windows network configuration in reverse dependency order.
pub async fn cleanup_network_config(state: AppliedNetworkState) -> Result<(), NetworkError> {
    let AppliedNetworkState {
        tun_name,
        adapter,
        addresses,
        routes,
        bypass_routes,
        iface_metrics,
        dns,
        nrpt,
        dns_guard,
        mut recovery,
    } = state;

    if let Some(guard) = recovery.as_mut() {
        let _ = guard.mark_stopping();
    }

    log_net::cleanup_windows(
        &tun_name,
        addresses.len(),
        routes.len(),
        bypass_routes.len(),
    );

    for entry in bypass_routes.iter().rev() {
        if let Err(err) = delete_route(entry) {
            log_net::bypass_route_del_failed(&err);
        }
    }

    for entry in routes.iter().rev() {
        if let Err(err) = delete_route(entry) {
            log_net::route_del_failed(&err);
        }
    }

    for address in &addresses {
        if let Err(err) = delete_unicast_address(adapter, address) {
            log_net::address_del_failed(&err);
        }
    }

    for iface in iface_metrics.iter().rev() {
        if let Err(err) = restore_interface_metric(adapter, *iface) {
            log_net::interface_metric_restore_failed(&err);
        }
    }

    if let Some(dns) = dns {
        log_dns::revert_start();
        if let Err(err) = cleanup_dns(dns) {
            log_dns::revert_failed(&err);
        }
    }

    if let Some(nrpt) = nrpt {
        if let Err(err) = cleanup_nrpt_guard(nrpt) {
            log_net::nrpt_cleanup_failed(&err);
        }
    }

    if let Some(guard) = dns_guard {
        if let Err(err) = cleanup_dns_guard(guard) {
            log_net::dns_guard_cleanup_failed(&err);
        }
    }

    if let Some(guard) = recovery {
        guard.clear()?;
    }
    clear_persisted_apply_report()?;

    Ok(())
}
