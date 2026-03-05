// Network config event logs (scope = net).
use std::fmt;
use std::net::IpAddr;

use crate::backend::wg::config::RouteTable;
use crate::{log_debug, log_info};

pub fn apply_linux(
    tun_name: &str,
    mtu: Option<u16>,
    addr_count: usize,
    table: Option<RouteTable>,
    dns_servers: usize,
    dns_search: usize,
) {
    log_info!(
        "net",
        "apply: tun={} mtu={:?} addr_count={} route_table={:?} dns_servers={} dns_search={}",
        tun_name,
        mtu,
        addr_count,
        table,
        dns_servers,
        dns_search
    );
}

pub fn apply_windows(tun_name: &str, addr_count: usize, dns_servers: usize, dns_search: usize) {
    log_info!(
        "net",
        "apply: tun={} addr_count={} dns_servers={} dns_search={}",
        tun_name,
        addr_count,
        dns_servers,
        dns_search
    );
}

pub fn link_index(link_index: u32) {
    log_info!("net", "link index: {link_index}");
}

pub fn stale_default_route_cleanup_failed(err: &impl fmt::Display) {
    log_info!("net", "stale default route cleanup failed: {err}");
}

pub fn address_add(addr: IpAddr, cidr: u8) {
    log_info!("net", "address: {}/{}", addr, cidr);
}

pub fn address_add_windows(addr: IpAddr, cidr: u8) {
    log_info!("net", "address add: {}/{}", addr, cidr);
}

pub fn address_remove(addr: IpAddr, cidr: u8) {
    log_info!("net", "address remove: {}/{}", addr, cidr);
}

pub fn stale_address_cleanup_removed(removed: usize) {
    log_info!("net", "stale address cleanup removed={removed}");
}

pub fn route_add(addr: IpAddr, cidr: u8, table: Option<u32>) {
    log_info!("net", "route: {}/{} table={:?}", addr, cidr, table);
}

pub fn route_add_windows(
    dest: IpAddr,
    prefix: u8,
    next_hop: Option<IpAddr>,
    if_index: u32,
    metric: u32,
) {
    log_info!(
        "net",
        "route add: {}/{} via {:?} if_index={} metric={}",
        dest,
        prefix,
        next_hop,
        if_index,
        metric
    );
}

pub fn dns_route_add_windows(dest: IpAddr, prefix: u8, if_index: u32, metric: u32) {
    log_info!(
        "net",
        "dns route add: {}/{} if_index={} metric={}",
        dest,
        prefix,
        if_index,
        metric
    );
}
pub fn bypass_route_add(dest: IpAddr, next_hop: Option<IpAddr>, if_index: u32) {
    log_info!(
        "net",
        "bypass route add: {} via {:?} if_index={}",
        dest,
        next_hop,
        if_index
    );
}

pub fn bypass_route_failed(ip: IpAddr, err: &impl fmt::Display) {
    log_info!("net", "bypass route failed for {ip}: {err}");
}

pub fn skip_default_route_v4() {
    log_info!(
        "net",
        "full-tunnel guard: missing IPv4 endpoint bypass route; aborting apply to avoid leak"
    );
}

pub fn skip_default_route_v6() {
    log_info!(
        "net",
        "full-tunnel guard: missing IPv6 endpoint bypass route; aborting apply to avoid leak"
    );
}

pub fn dns_guard_apply(blocked_server_count: usize) {
    log_info!(
        "net",
        "dns guard applied: blocked non-tunnel dns servers={} for outbound dns (53)",
        blocked_server_count
    );
}

pub fn dns_guard_cleanup_failed(err: &impl fmt::Display) {
    log_info!("net", "dns guard cleanup failed: {err}");
}

pub fn nrpt_apply(dns_server_count: usize, rule_count: usize) {
    log_info!(
        "net",
        "nrpt applied: dns_servers={} rules={}",
        dns_server_count,
        rule_count
    );
}

pub fn nrpt_cleanup_failed(err: &impl fmt::Display) {
    log_info!("net", "nrpt cleanup failed: {err}");
}
pub fn bypass_route_add_failed(dest: IpAddr, err: &impl fmt::Display) {
    log_info!("net", "bypass route add failed for {}: {err}", dest);
}

pub fn cleanup_linux(
    tun_name: &str,
    addr_count: usize,
    route_count: usize,
    table: Option<RouteTable>,
    dns_present: bool,
) {
    log_info!(
        "net",
        "cleanup: tun={} addr_count={} route_count={} table={:?} dns={}",
        tun_name,
        addr_count,
        route_count,
        table,
        dns_present
    );
}

pub fn cleanup_windows(tun_name: &str, addr_count: usize, route_count: usize, bypass_count: usize) {
    log_info!(
        "net",
        "cleanup: tun={} addr_count={} route_count={} bypass_count={}",
        tun_name,
        addr_count,
        route_count,
        bypass_count
    );
}

pub fn link_lookup_failed(err: &impl fmt::Display) {
    log_info!("net", "link lookup failed: {err}");
}

pub fn address_del(addr: IpAddr, cidr: u8) {
    log_info!("net", "address del: {}/{}", addr, cidr);
}

pub fn address_del_failed(err: &impl fmt::Display) {
    log_info!("net", "address del failed: {err}");
}

pub fn route_del(addr: IpAddr, cidr: u8, table: Option<u32>) {
    log_info!("net", "route del: {}/{} table={:?}", addr, cidr, table);
}

pub fn route_del_failed(err: &impl fmt::Display) {
    log_info!("net", "route del failed: {err}");
}

pub fn policy_rule_cleanup_failed(err: &impl fmt::Display) {
    log_info!("net", "policy rule cleanup failed: {err}");
}

pub fn stale_policy_rule_cleanup_failed(err: &impl fmt::Display) {
    log_info!("net", "stale policy rule cleanup failed: {err}");
}

pub fn interface_metric_set_v4(metric: u32) {
    log_info!("net", "interface metric set: v4 metric={}", metric);
}

pub fn interface_metric_set_failed_v4(err: &impl fmt::Display) {
    log_info!("net", "interface metric set failed (v4): {err}");
}

pub fn interface_metric_set_v6(metric: u32) {
    log_info!("net", "interface metric set: v6 metric={}", metric);
}

pub fn interface_metric_set_failed_v6(err: &impl fmt::Display) {
    log_info!("net", "interface metric set failed (v6): {err}");
}

pub fn interface_metric_restore_failed(err: &impl fmt::Display) {
    log_info!("net", "interface metric restore failed: {err}");
}

pub fn route_table_id_ignored(id: u32) {
    log_info!("net", "route table id ignored on windows: {id}");
}

pub fn fwmark_ignored() {
    log_info!("net", "fwmark ignored on windows");
}

pub fn bypass_route_del_failed(err: &impl fmt::Display) {
    log_info!("net", "bypass route del failed: {err}");
}

pub fn adapter_guid_parse_failed() {
    log_info!("net", "adapter guid parse failed, using NetworkGuid");
}

pub fn policy_rule_add_v6_fwmark(fwmark: u32, table: u32, pref: u32) {
    log_info!(
        "net",
        "policy rule add: v6 fwmark=0x{fwmark:x} table={table} pref={pref}"
    );
}

pub fn policy_rule_add_v6_not_fwmark(fwmark: u32, table: u32, pref: u32) {
    log_info!(
        "net",
        "policy rule add: v6 not fwmark=0x{fwmark:x} table={table} pref={pref}"
    );
}

pub fn policy_rule_add_v6_suppress(pref: u32) {
    log_info!("net", "policy rule add: v6 suppress main pref={pref}");
}

pub fn policy_rule_add_v4_fwmark(fwmark: u32, table: u32, pref: u32) {
    log_info!(
        "net",
        "policy rule add: v4 fwmark=0x{fwmark:x} table={table} pref={pref}"
    );
}

pub fn policy_rule_add_v4_not_fwmark(fwmark: u32, table: u32, pref: u32) {
    log_info!(
        "net",
        "policy rule add: v4 not fwmark=0x{fwmark:x} table={table} pref={pref}"
    );
}

pub fn policy_rule_add_v4_suppress(pref: u32) {
    log_info!("net", "policy rule add: v4 suppress main pref={pref}");
}

pub fn stale_default_route_del(iface: &str) {
    log_info!("net", "stale default route del: iface={iface}");
}

pub fn default_route_v4(iface: &str, gw: &str, metric: &str) {
    log_debug!(
        "net",
        "default route v4: iface={} gw={} metric={}",
        iface,
        gw,
        metric
    );
}

pub fn default_route_v4_not_found() {
    log_debug!("net", "default route v4: not found");
}

pub fn default_route_v4_read_failed(err: &impl fmt::Display) {
    log_debug!("net", "default route v4 read failed: {err}");
}

pub fn default_route_v6(iface: &str, gw: &str, metric: &str) {
    log_debug!(
        "net",
        "default route v6: iface={} gw={} metric={}",
        iface,
        gw,
        metric
    );
}

pub fn default_route_v6_not_found() {
    log_debug!("net", "default route v6: not found");
}

pub fn default_route_v6_read_failed(err: &impl fmt::Display) {
    log_debug!("net", "default route v6 read failed: {err}");
}

pub fn proc_status_read_failed(err: &impl fmt::Display) {
    log_debug!("net", "proc status read failed: {err}");
}

pub fn proc_status_parse_failed() {
    log_debug!("net", "proc status parse failed");
}

pub fn proc_status_capabilities(euid: u32, cap_eff: u64, has_net_admin: bool) {
    log_debug!(
        "net",
        "euid={} cap_eff=0x{:x} net_admin={}",
        euid,
        cap_eff,
        has_net_admin
    );
}
