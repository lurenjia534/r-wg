//! Windows 妤犵偛鍟胯ぐ瀵哥磾閹寸姷鎹曢梺鏉跨Ф閻ゅ棝宕楅妷銉ョ稉闁?//!
//! 闁煎崬鐭侀惌妤呮晬?//! - TUN 闁规亽鍎辫ぐ娑㈠捶閺夋寧绲诲☉鎾虫唉閻箖鎮介柆宥呭赋缂傚喚鍣槐?//! - Endpoint 缂備焦娲濈换鍐崉椤栨粍鏆犻柨娑樼墦娴尖晠宕楀鍛伎闂傚憞鍥﹀闂傚啰绮弻鍥箵閳╁啫顤侀柨娑橆檧缁?//! - DNS 闂佹澘绉堕悿鍡樼▔鎼粹剝绀€婵犲﹥鐔槐娆撳箰婢跺澶嶉柛娆欑悼妤犲洨鎷嬮崜褏鏋傞柨娑橆槶閳?
mod adapter;
mod addresses;
mod dns;
mod firewall;
mod metrics;
mod nrpt;
mod routes;
mod sockaddr;

use std::fmt;

use windows::core::PWSTR;
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, ERROR_OBJECT_ALREADY_EXISTS, WIN32_ERROR};
use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6};

use crate::backend::wg::config::{InterfaceAddress, InterfaceConfig, PeerConfig, RouteTable};
use crate::log::events::{dns as log_dns, net as log_net};

use adapter::AdapterInfo;
use addresses::{add_unicast_address, cleanup_stale_unicast_addresses, delete_unicast_address};
use dns::{apply_dns, cleanup_dns, DnsState};
use firewall::{apply_dns_guard, cleanup_dns_guard, DnsGuardState};
use metrics::{restore_interface_metric, set_interface_metric, InterfaceMetricState};
use nrpt::{apply_nrpt_guard, cleanup_nrpt_guard, NrptState};
use routes::{
    add_route, best_route_to, collect_allowed_routes, delete_route, detect_full_tunnel,
    resolve_endpoint_ips, RouteEntry,
};

/// Tunnel interface metric; lower is preferred.
const TUNNEL_METRIC: u32 = 0;

pub struct AppliedNetworkState {
    /// 闁哄牜鍓氶鍏兼償閺冨倹鏆忛柣銊ュ鐢挳宕ｉ敐鍛€崇紒澶婂簻缁辨瑩鎮介妸銈囪壘闁哄啨鍎辩换鏃€绋夋惔銏㈩伕闁荤偛妫寸槐姘跺Υ?
    tun_name: String,
    /// 闂侇偄鍊块崢銈夊闯閵娿倓绻嗛柟顓у灲缁辨獙fIndex/LUID/GUID闁挎稑顦埀?
    adapter: AdapterInfo,
    /// 闁哄牜鍓氶濂稿礃濞嗗繐寮抽柣銊ュ濠€鎾锤閳ь剟宕氬Δ鍕┾偓鍐Υ?
    addresses: Vec<InterfaceAddress>,
    /// 闁哄牜鍓氶濂稿礃濞嗗繐寮抽柣銊ュ閻箖鎮介崡鐐茬仚閻炴侗鐓夌槐姗漧lowedIPs闁挎稑顦埀?
    routes: Vec<RouteEntry>,
    /// Endpoint 缂備焦娲濈换鍐崉椤栨粍鏆犻柕?
    bypass_routes: Vec<RouteEntry>,
    /// 闁规亽鍎辫ぐ?metric 闁汇劌瀚敮顐ｆ叏鐎ｎ剙笑闁诡兛绶ょ槐婵嬫偨閵娿倗鑹鹃柟顓滃灩椤︽煡濡?
    iface_metrics: Vec<InterfaceMetricState>,
    /// DNS 濞ｅ浂鍠楅弫濂告偐閼哥鍋撴笟濠勭闁活潿鍔嬬花顒勫炊閻愬娉婇柨娑橆槶閳?
    dns: Option<DnsState>,
    nrpt: Option<NrptState>,
    /// DNS 闂傚啫寮剁涵鐘绘閺屻儲些闁诲浚鍋勯。鍓ф喆閸曨偄鐏熼柣妯垮煐閳ь兛绶ょ槐娆撳礂閵娾晜鐦堥梺顒佹尰濡炲倿宕ラ婊勬殢闁挎稑顦埀?
    dns_guard: Option<DnsGuardState>,
}

#[derive(Debug)]
pub enum NetworkError {
    AdapterNotFound(String),
    EndpointResolve(String),
    UnsafeRouting(String),
    Io(std::io::Error),
    Win32 {
        context: &'static str,
        code: WIN32_ERROR,
    },
}

impl fmt::Display for NetworkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetworkError::AdapterNotFound(name) => {
                write!(f, "adapter not found: {name}")
            }
            NetworkError::EndpointResolve(message) => {
                write!(f, "endpoint resolve failed: {message}")
            }
            NetworkError::UnsafeRouting(message) => {
                write!(f, "unsafe routing configuration: {message}")
            }
            NetworkError::Io(err) => write!(f, "io error: {err}"),
            NetworkError::Win32 { context, code } => {
                let err = std::io::Error::from_raw_os_error(code.0 as i32);
                write!(f, "{context}: {err} (code={})", code.0)
            }
        }
    }
}

impl std::error::Error for NetworkError {}

impl From<std::io::Error> for NetworkError {
    fn from(err: std::io::Error) -> Self {
        NetworkError::Io(err)
    }
}

pub async fn apply_network_config(
    tun_name: &str,
    interface: &InterfaceConfig,
    peers: &[PeerConfig],
) -> Result<AppliedNetworkState, NetworkError> {
    // 1) 閻犱焦婢樼紞宥夊春閻戞ɑ鎷遍柛娆忓€归弳鐔兼晬鐏炶偐鈹掑ù婊冩唉閻︽牠寮婧惧亾?
    log_net::apply_windows(
        tun_name,
        interface.addresses.len(),
        interface.dns_servers.len(),
        interface.dns_search.len(),
    );

    if let Some(RouteTable::Id(id)) = interface.table {
        log_net::route_table_id_ignored(id);
    }
    if interface.fwmark.is_some() {
        log_net::fwmark_ignored();
    }
    // 2) 閻熸瑱绲鹃悗浠嬬嵁鐠鸿櫣鏆板ù锝呯Ф濞蹭即寮介崶顑藉亾閸岀偛甯抽柛锝冨妸閳?
    let adapter = adapter::find_adapter_with_retry(tun_name).await?;

    let mut state = AppliedNetworkState {
        tun_name: tun_name.to_string(),
        adapter,
        addresses: Vec::new(),
        routes: Vec::new(),
        bypass_routes: Vec::new(),
        iface_metrics: Vec::new(),
        dns: None,
        nrpt: None,
        dns_guard: None,
    };

    // 3) 婵炴挸鎳愰幃濠囧储閸℃钑夋繛鍫濐儑閺嗏偓闁革附婢樺鍐晬瀹€鍕級闁稿繐绉存總鏍传瀹ュ牏鐔呴柣銏犲船閸犲懐绮甸弽銉㈠亾?
    cleanup_stale_unicast_addresses(adapter, &interface.addresses)?;

    // 4) 闁告劖鐟ラ崣鍡涘箳閵夈儱缍撻柛锔芥緲濞煎啴鏁嶉崷姹竩4/IPv6闁挎稑顦埀?
    for address in &interface.addresses {
        log_net::address_add_windows(address.addr, address.cidr);
        if let Err(err) = add_unicast_address(adapter, address) {
            let _ = cleanup_network_config(state).await;
            return Err(err);
        }
        state.addresses.push(address.clone());
    }

    // 5) 婵懓娲﹂埀?AllowedIPs闁挎稑鑻懟鐔煎礆閵堝棙鐒介柡鍕靛灠閹焦绋夐崫鍕伎闂傚憞鍥﹀闁?
    let routes = collect_allowed_routes(peers);
    let (full_v4, full_v6) = detect_full_tunnel(&routes);

    // 6) 闁稿繈鍔戝В鈧梺顒佹尰濡炲倿姊藉鍕У闁规亽鍎辫ぐ?metric闁挎稑濂旀禍鎺楀箮閵忕姴绐楀娑欘焾椤撹崵鎹勯婊勬殸濞村吋锚閸樻稓鐥閳?
    if interface.table != Some(RouteTable::Off) {
        if full_v4 {
            match set_interface_metric(adapter, AF_INET, TUNNEL_METRIC) {
                Ok(metric_state) => {
                    log_net::interface_metric_set_v4(TUNNEL_METRIC);
                    state.iface_metrics.push(metric_state);
                }
                Err(err) => {
                    log_net::interface_metric_set_failed_v4(&err);
                    let _ = cleanup_network_config(state).await;
                    return Err(NetworkError::UnsafeRouting(format!(
                        "failed to set IPv4 interface metric for full tunnel: {err}"
                    )));
                }
            }
        }
        if full_v6 {
            match set_interface_metric(adapter, AF_INET6, TUNNEL_METRIC) {
                Ok(metric_state) => {
                    log_net::interface_metric_set_v6(TUNNEL_METRIC);
                    state.iface_metrics.push(metric_state);
                }
                Err(err) => {
                    log_net::interface_metric_set_failed_v6(&err);
                    let _ = cleanup_network_config(state).await;
                    return Err(NetworkError::UnsafeRouting(format!(
                        "failed to set IPv6 interface metric for full tunnel: {err}"
                    )));
                }
            }
        }
    }

    let mut bypass_routes = Vec::new();
    let mut endpoint_v4 = 0usize;
    let mut endpoint_v6 = 0usize;
    let mut bypass_v4 = 0usize;
    let mut bypass_v6 = 0usize;
    // 7) 闁稿繈鍔戝В鈧梺顒佹尭濠р偓闁哄拋鍨粭鍛存晬鐏炶壈绀?Endpoint 闁汇垻鍠愰崹姘辩磼閺囷紕绠栭悹渚灣閺侀亶濡?
    if interface.table != Some(RouteTable::Off) && (full_v4 || full_v6) {
        let endpoint_ips = resolve_endpoint_ips(peers).await?;
        for ip in endpoint_ips {
            if ip.is_ipv4() {
                endpoint_v4 += 1;
                if !full_v4 {
                    continue;
                }
            } else {
                endpoint_v6 += 1;
                if !full_v6 {
                    continue;
                }
            }
            match best_route_to(ip) {
                Ok(route) => {
                    log_net::bypass_route_add(route.dest, route.next_hop, route.if_index);
                    if ip.is_ipv4() {
                        bypass_v4 += 1;
                    } else {
                        bypass_v6 += 1;
                    }
                    bypass_routes.push(route);
                }
                Err(err) => log_net::bypass_route_failed(ip, &err),
            }
        }
    }

    let missing_v4_bypass = full_v4 && endpoint_v4 > 0 && bypass_v4 == 0;
    let missing_v6_bypass = full_v6 && endpoint_v6 > 0 && bypass_v6 == 0;
    if missing_v4_bypass {
        log_net::skip_default_route_v4();
    }
    if missing_v6_bypass {
        log_net::skip_default_route_v6();
    }
    if missing_v4_bypass || missing_v6_bypass {
        let family = match (missing_v4_bypass, missing_v6_bypass) {
            (true, true) => "IPv4+IPv6",
            (true, false) => "IPv4",
            (false, true) => "IPv6",
            (false, false) => unreachable!(),
        };
        let _ = cleanup_network_config(state).await;
        return Err(NetworkError::UnsafeRouting(format!(
            "full-tunnel {family} endpoint bypass route missing; refusing to continue to avoid traffic/DNS leak"
        )));
    }

    // 8) 闁稿繐鐗嗛崯鎾诲礂?Endpoint bypass 閻犱警鍨抽弫閬嶆晬鐏炶棄鏅欓柛?AllowedIPs 閻犱警鍨抽弫閬嶆晬瀹€鍕級闁稿繐绉堕悡顓㈠汲閸屾稓銈﹂梺鎻掔箰婵參骞愭担绋跨厒闂傚憞鍥﹀闁?
    if interface.table != Some(RouteTable::Off) {
        for entry in bypass_routes {
            if let Err(err) = add_route(&entry) {
                let _ = cleanup_network_config(state).await;
                return Err(err);
            }
            state.bypass_routes.push(entry);
        }

        for route in routes {
            let entry = RouteEntry {
                dest: route.addr,
                prefix: route.cidr,
                next_hop: None,
                if_index: adapter.if_index,
                luid: adapter.luid,
            };
            log_net::route_add_windows(
                entry.dest,
                entry.prefix,
                entry.next_hop,
                entry.if_index,
                TUNNEL_METRIC,
            );
            if let Err(err) = add_route(&entry) {
                let _ = cleanup_network_config(state).await;
                return Err(err);
            }
            state.routes.push(entry);
        }
    }

    // 8.5) 闁稿繈鍔戝В鈧梺顒佹尫缁楀懏绋夊ú顏勫赋缂?DNS 闁哄牆绉存慨鐔煎闯閵婏箑娼戦柛鏃傚С鐎靛矂寮垫ウ璺ㄧ唴闁汇垺鍞荤槐婵嬫焼閸喖甯抽悶姘煎亞闁绱掗悢鎯板幀闁哄洦娼欓崣鎸庢媴閹惧湱鐔呴柣銏ｉ哺婵娀宕￠悩顔瑰亾?
    if interface.table != Some(RouteTable::Off) && !interface.dns_servers.is_empty() {
        for dns_server in &interface.dns_servers {
            let applies = (dns_server.is_ipv4() && full_v4) || (dns_server.is_ipv6() && full_v6);
            if !applies {
                continue;
            }

            let entry = RouteEntry {
                dest: *dns_server,
                prefix: if dns_server.is_ipv4() { 32 } else { 128 },
                next_hop: None,
                if_index: adapter.if_index,
                luid: adapter.luid,
            };
            log_net::dns_route_add_windows(entry.dest, entry.prefix, entry.if_index, TUNNEL_METRIC);
            if let Err(err) = add_route(&entry) {
                let _ = cleanup_network_config(state).await;
                return Err(err);
            }
            state.routes.push(entry);
        }
    }

    // 9) 闁告劖鐟ラ崣?DNS 閻犱礁澧介悿鍡涙晬鐏炲浜奸悹鎰╁劥椤绋夋ウ鍨瘈闁告稐绮欓弫濠勬嫚椤栨俺瀚欓柛銉у仦缁挳濡?
    if !interface.dns_servers.is_empty() || !interface.dns_search.is_empty() {
        log_dns::apply_summary(interface.dns_servers.len(), interface.dns_search.len());
        match apply_dns(adapter, &interface.dns_servers, &interface.dns_search) {
            Ok(dns_state) => state.dns = Some(dns_state),
            Err(err) => {
                log_dns::apply_failed(&err);
                let _ = cleanup_network_config(state).await;
                return Err(err);
            }
        }
    }

    if !interface.dns_servers.is_empty()
        && interface.table != Some(RouteTable::Off)
        && (full_v4 || full_v6)
    {
        match apply_nrpt_guard(adapter, &interface.dns_servers) {
            Ok(nrpt_state) => state.nrpt = nrpt_state,
            Err(err) => {
                let _ = cleanup_network_config(state).await;
                return Err(err);
            }
        }

        match apply_dns_guard(adapter, full_v4, full_v6, &interface.dns_servers) {
            Ok(guard_state) => state.dns_guard = guard_state,
            Err(err) => {
                let _ = cleanup_network_config(state).await;
                return Err(err);
            }
        }
    }

    Ok(state)
}

pub async fn cleanup_network_config(state: AppliedNetworkState) -> Result<(), NetworkError> {
    // 闁搞儳鍋炵划瀛樸亜閸濆嫮纰嶉柨娑欒壘閸樻稓鎹勯婊勬殸闁靛棔绀侀幃妤呭捶閺夋寧绲婚柕鍡曠閸熲偓 metric/DNS闁挎稑鐭傛导鈺呭礂瀹ュ棛鏆欓柣锝嗙懃婵傛牠宕鍥厙缂備胶鍠撶紞澶岀磼濠у簱鍋?
    log_net::cleanup_windows(
        &state.tun_name,
        state.addresses.len(),
        state.routes.len(),
        state.bypass_routes.len(),
    );

    // 闁稿繐鐗嗛崹?bypass routes闁挎稑鐭傛导鈺呭礂瀹ュ懏鍊电紓渚囧弮缁垳鎷嬮妶鍫㈢唴闁汇垼椴哥粩濠氭偠閸℃顨涢柛婵嗙Т閸ゎ參宕ｉ敐鍐ｅ亾?
    for entry in state.bypass_routes.iter().rev() {
        if let Err(err) = delete_route(entry) {
            log_net::bypass_route_del_failed(&err);
        }
    }

    // 闁告劕绉撮崹褰掑疾椤曗偓閳ь剚淇洪惌楣冩偨娓氬簱鍋?
    for entry in state.routes.iter().rev() {
        if let Err(err) = delete_route(entry) {
            log_net::route_del_failed(&err);
        }
    }

    // 闁告帞濞€濞呭酣骞掗妷銉ョ稉闁革附婢樺鍐Υ?
    for address in &state.addresses {
        if let Err(err) = delete_unicast_address(state.adapter, address) {
            log_net::address_del_failed(&err);
        }
    }

    // 闁诡厹鍨归ˇ鏌ュ箳閵夈儱缍?metric闁?
    for iface in state.iface_metrics.iter().rev() {
        if let Err(err) = restore_interface_metric(state.adapter, *iface) {
            log_net::interface_metric_restore_failed(&err);
        }
    }

    // 闁搞儳鍋炵划?DNS 閻犱礁澧介悿鍡涘Υ?
    if let Some(dns) = state.dns {
        log_dns::revert_start();
        if let Err(err) = cleanup_dns(dns) {
            log_dns::revert_failed(&err);
        }
    }

    if let Some(nrpt) = state.nrpt {
        if let Err(err) = cleanup_nrpt_guard(nrpt) {
            log_net::nrpt_cleanup_failed(&err);
        }
    }

    if let Some(guard) = state.dns_guard {
        if let Err(err) = cleanup_dns_guard(guard) {
            log_net::dns_guard_cleanup_failed(&err);
        }
    }

    Ok(())
}

fn pwstr_to_string(ptr: PWSTR) -> String {
    // 閻?Windows 閻庣妫勯悺褏绮敂鑳洬闁圭娲幏鈩冩姜椤掍礁搴婂☉?Rust String闁?
    if ptr.0.is_null() {
        return String::new();
    }
    unsafe {
        let mut len = 0usize;
        let mut cursor = ptr.0;
        while *cursor != 0 {
            len += 1;
            cursor = cursor.add(1);
        }
        let slice = std::slice::from_raw_parts(ptr.0, len);
        String::from_utf16_lossy(slice)
    }
}

fn is_already_exists(code: WIN32_ERROR) -> bool {
    // Windows 閻庨潧鍘滈埀顒佺矊閸戯紕鈧稒锚濠€顏堝灳濠靛鏅╅悹鍥跺灡濠€浣瑰緞濮橆偊鍤嬮弶鈺傛煥濞叉牠宕愮粭琛″亾?
    code == ERROR_OBJECT_ALREADY_EXISTS || code == ERROR_ALREADY_EXISTS
}
