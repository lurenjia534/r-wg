
// Windows 平台网络配置实现。
// 目标：根据 WG 配置为 Wintun 适配器设置地址、路由、接口 metric 等，
// 同时处理绕过路由（bypass route）以保证 Endpoint 可达。
use std::collections::HashSet;
use std::ffi::CStr;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use tokio::time::sleep;
use windows::core::{GUID, PSTR, PWSTR};
use windows::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, ERROR_BUFFER_OVERFLOW, ERROR_FILE_NOT_FOUND, ERROR_NOT_FOUND,
    ERROR_OBJECT_ALREADY_EXISTS, BOOLEAN, NO_ERROR, WIN32_ERROR,
};
use windows::Win32::NetworkManagement::IpHelper::{
    CreateIpForwardEntry2, CreateUnicastIpAddressEntry, DeleteIpForwardEntry2,
    DeleteUnicastIpAddressEntry, FreeInterfaceDnsSettings, GetAdaptersAddresses,
    GetBestRoute2, GetInterfaceDnsSettings, GetIpInterfaceEntry, InitializeIpForwardEntry,
    InitializeIpInterfaceEntry, InitializeUnicastIpAddressEntry, SetInterfaceDnsSettings,
    SetIpInterfaceEntry, DNS_INTERFACE_SETTINGS, DNS_INTERFACE_SETTINGS_VERSION1,
    DNS_SETTING_NAMESERVER, DNS_SETTING_SEARCHLIST, IP_ADAPTER_ADDRESSES_LH,
    IP_ADAPTER_UNICAST_ADDRESS_LH, IP_ADDRESS_PREFIX, MIB_IPFORWARD_ROW2, MIB_IPINTERFACE_ROW,
    MIB_UNICASTIPADDRESS_ROW, GAA_FLAG_INCLUDE_PREFIX, GAA_FLAG_SKIP_ANYCAST,
    GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST,
};
use windows::Win32::NetworkManagement::Ndis::NET_LUID_LH;
use windows::Win32::Networking::WinSock::{
    ADDRESS_FAMILY, AF_INET, AF_INET6, AF_UNSPEC, IN6_ADDR, IN6_ADDR_0, IN_ADDR, IN_ADDR_0,
    MIB_IPPROTO_NETMGMT, SOCKADDR_IN, SOCKADDR_IN6, SOCKADDR_INET, SOCKET_ADDRESS,
};

use crate::backend::wg::config::{
    AllowedIp, InterfaceAddress, InterfaceConfig, PeerConfig, RouteTable,
};
use crate::log;

// 适配器创建后可能有短暂的系统延迟，允许重试查询。
const ADAPTER_RETRY_COUNT: usize = 10;
const ADAPTER_RETRY_DELAY: Duration = Duration::from_millis(200);
// 默认隧道接口与路由的 metric（值越小优先级越高）。
const TUNNEL_METRIC: u32 = 5;

#[derive(Clone, Copy)]
struct AdapterInfo {
    // 接口索引（ifIndex）用于路由/地址绑定。
    if_index: u32,
    // NET_LUID 用于部分 IP Helper API。
    luid: NET_LUID_LH,
    guid: GUID,
    dns_guid_fallback: Option<GUID>,
}

#[derive(Clone)]
struct RouteEntry {
    // 路由目的地址与前缀长度。
    dest: IpAddr,
    prefix: u8,
    // 下一跳（bypass route 需要；默认路由可为 None 表示 on-link）。
    next_hop: Option<IpAddr>,
    // 绑定到具体接口。
    if_index: u32,
    luid: NET_LUID_LH,
}

#[derive(Clone, Copy)]
struct InterfaceMetricState {
    // 保存修改前的接口 metric 状态，便于恢复。
    family: ADDRESS_FAMILY,
    use_auto: BOOLEAN,
    metric: u32,
}

#[derive(Clone)]
struct DnsState {
    guid: GUID,
    touched_nameserver: bool,
    touched_search: bool,
    original_nameserver: Option<String>,
    original_searchlist: Option<String>,
}

pub struct AppliedNetworkState {
    // 保存本次应用的状态，便于 cleanup 时回滚。
    tun_name: String,
    adapter: AdapterInfo,
    addresses: Vec<InterfaceAddress>,
    routes: Vec<RouteEntry>,
    bypass_routes: Vec<RouteEntry>,
    iface_metrics: Vec<InterfaceMetricState>,
    dns: Option<DnsState>,
}

#[derive(Debug)]
pub enum NetworkError {
    // 适配器未找到（通常是 Wintun 尚未创建或名称不匹配）。
    AdapterNotFound(String),
    // Endpoint 解析失败。
    EndpointResolve(String),
    // 系统 I/O 错误。
    Io(std::io::Error),
    // Win32 API 调用失败，带上下文与错误码。
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
    // 入口：按 WG 配置为指定 tun 适配器写入地址、路由与 metric。
    log_net(format!(
        "apply: tun={tun_name} addr_count={} dns_servers={} dns_search={}",
        interface.addresses.len(),
        interface.dns_servers.len(),
        interface.dns_search.len()
    ));

    if let Some(RouteTable::Id(id)) = interface.table {
        log_net(format!("route table id ignored on windows: {id}"));
    }
    if interface.fwmark.is_some() {
        log_net("fwmark ignored on windows".to_string());
    }
    let adapter = find_adapter_with_retry(tun_name).await?;

    let mut state = AppliedNetworkState {
        tun_name: tun_name.to_string(),
        adapter,
        addresses: Vec::new(),
        routes: Vec::new(),
        bypass_routes: Vec::new(),
        iface_metrics: Vec::new(),
        dns: None,
    };

    cleanup_stale_unicast_addresses(adapter, &interface.addresses)?;

    for address in &interface.addresses {
        log_net(format!("address add: {}/{}", address.addr, address.cidr));
        if let Err(err) = add_unicast_address(adapter, address) {
            let _ = cleanup_network_config(state).await;
            return Err(err);
        }
        state.addresses.push(address.clone());
    }

    let routes = collect_allowed_routes(peers);
    let (full_v4, full_v6) = detect_full_tunnel(&routes);

    if interface.table != Some(RouteTable::Off) {
        // 默认隧道需要更低的 interface metric 才能抢占系统默认路由。
        if full_v4 {
            match set_interface_metric(adapter, AF_INET, TUNNEL_METRIC) {
                Ok(metric_state) => {
                    log_net(format!("interface metric set: v4 metric={}", TUNNEL_METRIC));
                    state.iface_metrics.push(metric_state);
                }
                Err(err) => log_net(format!("interface metric set failed (v4): {err}")),
            }
        }
        if full_v6 {
            match set_interface_metric(adapter, AF_INET6, TUNNEL_METRIC) {
                Ok(metric_state) => {
                    log_net(format!("interface metric set: v6 metric={}", TUNNEL_METRIC));
                    state.iface_metrics.push(metric_state);
                }
                Err(err) => log_net(format!("interface metric set failed (v6): {err}")),
            }
        }
    }

    let mut bypass_routes = Vec::new();
    let mut endpoint_v4 = 0usize;
    let mut endpoint_v6 = 0usize;
    let mut bypass_v4 = 0usize;
    let mut bypass_v6 = 0usize;
    if interface.table != Some(RouteTable::Off) && (full_v4 || full_v6) {
        // full-tunnel 时必须为 Endpoint 添加绕过路由，否则握手无法出网。
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
                    log_net(format!(
                        "bypass route add: {} via {:?} if_index={}",
                        route.dest, route.next_hop, route.if_index
                    ));
                    if ip.is_ipv4() {
                        bypass_v4 += 1;
                    } else {
                        bypass_v6 += 1;
                    }
                    bypass_routes.push(route);
                }
                Err(err) => log_net(format!("bypass route failed for {ip}: {err}")),
            }
        }
    }

    let allow_v4_default = !(full_v4 && endpoint_v4 > 0 && bypass_v4 == 0);
    let allow_v6_default = !(full_v6 && endpoint_v6 > 0 && bypass_v6 == 0);
    if !allow_v4_default {
        // 如果无法为 Endpoint 添加绕过路由，则跳过默认路由，避免断网。
        log_net("skip IPv4 default route: no bypass route for endpoint".to_string());
    }
    if !allow_v6_default {
        log_net("skip IPv6 default route: no bypass route for endpoint".to_string());
    }

    if interface.table != Some(RouteTable::Off) {
        for route in routes {
            if is_default_route(&route) {
                if route.addr.is_ipv4() && !allow_v4_default {
                    continue;
                }
                if route.addr.is_ipv6() && !allow_v6_default {
                    continue;
                }
            }
            let entry = RouteEntry {
                dest: route.addr,
                prefix: route.cidr,
                next_hop: None,
                if_index: adapter.if_index,
                luid: adapter.luid,
            };
            log_net(format!(
                "route add: {}/{} via {:?} if_index={} metric={}",
                entry.dest, entry.prefix, entry.next_hop, entry.if_index, TUNNEL_METRIC
            ));
            if let Err(err) = add_route(&entry) {
                let _ = cleanup_network_config(state).await;
                return Err(err);
            }
            state.routes.push(entry);
        }

        for entry in bypass_routes {
            match add_route(&entry) {
                Ok(()) => state.bypass_routes.push(entry),
                Err(err) => log_net(format!(
                    "bypass route add failed for {}: {err}",
                    entry.dest
                )),
            }
        }
    }

    if !interface.dns_servers.is_empty() || !interface.dns_search.is_empty() {
        log_net(format!(
            "dns: servers={} search={}",
            interface.dns_servers.len(),
            interface.dns_search.len()
        ));
        match apply_dns(adapter, &interface.dns_servers, &interface.dns_search) {
            Ok(dns_state) => state.dns = Some(dns_state),
            Err(err) => {
                log_net(format!("dns apply failed: {err}"));
                let _ = cleanup_network_config(state).await;
                return Err(err);
            }
        }
    }

    Ok(state)
}

pub async fn cleanup_network_config(state: AppliedNetworkState) -> Result<(), NetworkError> {
    // 回滚：先删 bypass route，再删普通路由，再删地址，最后恢复 metric。
    log_net(format!(
        "cleanup: tun={} addr_count={} route_count={} bypass_count={}",
        state.tun_name,
        state.addresses.len(),
        state.routes.len(),
        state.bypass_routes.len()
    ));

    for entry in state.bypass_routes.iter().rev() {
        if let Err(err) = delete_route(entry) {
            log_net(format!("bypass route del failed: {err}"));
        }
    }

    for entry in state.routes.iter().rev() {
        if let Err(err) = delete_route(entry) {
            log_net(format!("route del failed: {err}"));
        }
    }

    for address in &state.addresses {
        if let Err(err) = delete_unicast_address(state.adapter, address) {
            log_net(format!("address del failed: {err}"));
        }
    }

    for iface in state.iface_metrics.iter().rev() {
        if let Err(err) = restore_interface_metric(state.adapter, *iface) {
            log_net(format!("interface metric restore failed: {err}"));
        }
    }

    if let Some(dns) = state.dns {
        log_net("dns revert".to_string());
        if let Err(err) = cleanup_dns(dns) {
            log_net(format!("dns revert failed: {err}"));
        }
    }

    Ok(())
}

fn log_net(message: String) {
    // 统一网络层日志出口。
    log::log("net", message);
}

fn apply_dns(
    adapter: AdapterInfo,
    servers: &[IpAddr],
    search: &[String],
) -> Result<DnsState, NetworkError> {
    let primary = apply_dns_with_guid(adapter.guid, servers, search);
    if primary.is_ok() {
        return primary;
    }
    if let Some(fallback) = adapter.dns_guid_fallback {
        log_net("dns apply retry with fallback guid".to_string());
        return apply_dns_with_guid(fallback, servers, search);
    }
    primary
}

fn apply_dns_with_guid(
    guid: GUID,
    servers: &[IpAddr],
    search: &[String],
) -> Result<DnsState, NetworkError> {
    let (original_nameserver, original_searchlist) = read_interface_dns_settings(guid)?;

    let search_items: Vec<String> = search
        .iter()
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .map(|item| item.to_string())
        .collect();

    let touched_nameserver = !servers.is_empty();
    let touched_search = !search_items.is_empty();

    if !touched_nameserver && !touched_search {
        return Ok(DnsState {
            guid,
            touched_nameserver,
            touched_search,
            original_nameserver,
            original_searchlist,
        });
    }

    let mut flags = 0u64;
    let mut nameserver_buf = None;
    if touched_nameserver {
        flags |= DNS_SETTING_NAMESERVER as u64;
        let joined = servers
            .iter()
            .map(|ip| ip.to_string())
            .collect::<Vec<_>>()
            .join(",");
        nameserver_buf = Some(encode_utf16_z(&joined));
    }

    let mut search_buf = None;
    if touched_search {
        flags |= DNS_SETTING_SEARCHLIST as u64;
        let joined = search_items.join(",");
        search_buf = Some(encode_utf16_z(&joined));
    }

    let settings = DNS_INTERFACE_SETTINGS {
        Version: DNS_INTERFACE_SETTINGS_VERSION1,
        Flags: flags,
        Domain: PWSTR(std::ptr::null_mut()),
        NameServer: pwstr_from_buf(&nameserver_buf),
        SearchList: pwstr_from_buf(&search_buf),
        RegistrationEnabled: 0,
        RegisterAdapterName: 0,
        EnableLLMNR: 0,
        QueryAdapterName: 0,
        ProfileNameServer: PWSTR(std::ptr::null_mut()),
    };

    let result = unsafe { SetInterfaceDnsSettings(guid, &settings) };
    if result != NO_ERROR {
        return Err(NetworkError::Win32 {
            context: "SetInterfaceDnsSettings",
            code: result,
        });
    }

    Ok(DnsState {
        guid,
        touched_nameserver,
        touched_search,
        original_nameserver,
        original_searchlist,
    })
}

fn cleanup_dns(state: DnsState) -> Result<(), NetworkError> {
    if !state.touched_nameserver && !state.touched_search {
        return Ok(());
    }

    let mut flags = 0u64;
    let mut nameserver_buf = None;
    if state.touched_nameserver {
        flags |= DNS_SETTING_NAMESERVER as u64;
        if let Some(value) = state.original_nameserver.as_ref() {
            nameserver_buf = Some(encode_utf16_z(value));
        }
    }

    let mut search_buf = None;
    if state.touched_search {
        flags |= DNS_SETTING_SEARCHLIST as u64;
        if let Some(value) = state.original_searchlist.as_ref() {
            search_buf = Some(encode_utf16_z(value));
        }
    }

    if flags == 0 {
        return Ok(());
    }

    let settings = DNS_INTERFACE_SETTINGS {
        Version: DNS_INTERFACE_SETTINGS_VERSION1,
        Flags: flags,
        Domain: PWSTR(std::ptr::null_mut()),
        NameServer: pwstr_from_buf(&nameserver_buf),
        SearchList: pwstr_from_buf(&search_buf),
        RegistrationEnabled: 0,
        RegisterAdapterName: 0,
        EnableLLMNR: 0,
        QueryAdapterName: 0,
        ProfileNameServer: PWSTR(std::ptr::null_mut()),
    };

    let result = unsafe { SetInterfaceDnsSettings(state.guid, &settings) };
    if result != NO_ERROR {
        return Err(NetworkError::Win32 {
            context: "SetInterfaceDnsSettings(revert)",
            code: result,
        });
    }

    Ok(())
}

fn read_interface_dns_settings(
    guid: GUID,
) -> Result<(Option<String>, Option<String>), NetworkError> {
    let mut settings = DNS_INTERFACE_SETTINGS::default();
    settings.Version = DNS_INTERFACE_SETTINGS_VERSION1;

    let result = unsafe { GetInterfaceDnsSettings(guid, &mut settings) };
    if result == ERROR_FILE_NOT_FOUND {
        log_net("dns settings not found for interface, assuming empty".to_string());
        return Ok((None, None));
    }
    if result != NO_ERROR {
        return Err(NetworkError::Win32 {
            context: "GetInterfaceDnsSettings",
            code: result,
        });
    }

    let nameserver = normalize_pwstr(settings.NameServer);
    let searchlist = normalize_pwstr(settings.SearchList);

    unsafe {
        FreeInterfaceDnsSettings(&mut settings);
    }

    Ok((nameserver, searchlist))
}

fn normalize_pwstr(ptr: PWSTR) -> Option<String> {
    let value = pwstr_to_string(ptr);
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn encode_utf16_z(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn pwstr_from_buf(buf: &Option<Vec<u16>>) -> PWSTR {
    match buf {
        Some(value) => PWSTR(value.as_ptr() as *mut u16),
        None => PWSTR(std::ptr::null_mut()),
    }
}

async fn find_adapter_with_retry(name: &str) -> Result<AdapterInfo, NetworkError> {
    // Wintun 创建后不一定立刻出现在系统列表，重试等待。
    for _ in 0..ADAPTER_RETRY_COUNT {
        if let Some(adapter) = find_adapter_by_name(name)? {
            return Ok(adapter);
        }
        sleep(ADAPTER_RETRY_DELAY).await;
    }
    Err(NetworkError::AdapterNotFound(name.to_string()))
}

fn find_adapter_by_name(name: &str) -> Result<Option<AdapterInfo>, NetworkError> {
    // 通过 GetAdaptersAddresses 遍历系统网卡，匹配 FriendlyName。
    let mut size = 0u32;
    let family = AF_UNSPEC.0 as u32;
    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;

    let mut result = unsafe { GetAdaptersAddresses(family, flags, None, None, &mut size) };
    if result != ERROR_BUFFER_OVERFLOW.0 && result != NO_ERROR.0 {
        return Err(NetworkError::Win32 {
            context: "GetAdaptersAddresses(size)",
            code: WIN32_ERROR(result),
        });
    }

    let mut buffer = vec![0u8; size as usize];
    let ptr = buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;
    result = unsafe { GetAdaptersAddresses(family, flags, None, Some(ptr), &mut size) };
    if result != NO_ERROR.0 {
        return Err(NetworkError::Win32 {
            context: "GetAdaptersAddresses(data)",
            code: WIN32_ERROR(result),
        });
    }

    let mut adapter = ptr;
    unsafe {
        while !adapter.is_null() {
            let current = &*adapter;
            let friendly = pwstr_to_string(current.FriendlyName);
            if friendly.eq_ignore_ascii_case(name) {
                let adapter_name = pstr_to_string(current.AdapterName);
                let guid = match extract_guid_from_adapter_name(&adapter_name) {
                    Some(guid) => guid,
                    None => {
                        log_net("adapter guid parse failed, using NetworkGuid".to_string());
                        current.NetworkGuid
                    }
                };
                let dns_guid_fallback = if guid != current.NetworkGuid {
                    Some(current.NetworkGuid)
                } else {
                    None
                };
                return Ok(Some(AdapterInfo {
                    if_index: current.Anonymous1.Anonymous.IfIndex,
                    luid: current.Luid,
                    guid,
                    dns_guid_fallback,
                }));
            }
            adapter = current.Next;
        }
    }

    Ok(None)
}

fn pwstr_to_string(ptr: PWSTR) -> String {
    // 将 Windows 宽字符指针转换为 Rust String。
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

fn pstr_to_string(ptr: PSTR) -> String {
    // Convert Windows ANSI string pointer to Rust String.
    if ptr.0.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(ptr.0 as *const i8).to_string_lossy().into_owned() }
}

fn extract_guid_from_adapter_name(name: &str) -> Option<GUID> {
    let trimmed = name.trim();
    let candidate = if let Some(start) = trimmed.find('{') {
        let rest = &trimmed[start + 1..];
        if let Some(end) = rest.find('}') {
            &rest[..end]
        } else {
            trimmed
        }
    } else {
        trimmed
    };
    let candidate = candidate.trim_matches('{').trim_matches('}');
    if !is_guid_string(candidate) {
        return None;
    }
    Some(GUID::from(candidate))
}

fn is_guid_string(value: &str) -> bool {
    if value.len() != 36 {
        return false;
    }
    let bytes = value.as_bytes();
    for (idx, ch) in bytes.iter().enumerate() {
        match idx {
            8 | 13 | 18 | 23 => {
                if *ch != b'-' {
                    return false;
                }
            }
            _ => {
                if !ch.is_ascii_hexdigit() {
                    return false;
                }
            }
        }
    }
    true
}

fn add_unicast_address(
    adapter: AdapterInfo,
    address: &InterfaceAddress,
) -> Result<(), NetworkError> {
    // 为接口添加单播地址。
    let row = build_unicast_row(adapter, address);
    let result = unsafe { CreateUnicastIpAddressEntry(&row) };
    if result == NO_ERROR || is_already_exists(result) {
        Ok(())
    } else {
        Err(NetworkError::Win32 {
            context: "CreateUnicastIpAddressEntry",
            code: result,
        })
    }
}

fn delete_unicast_address(
    adapter: AdapterInfo,
    address: &InterfaceAddress,
) -> Result<(), NetworkError> {
    // 从接口删除单播地址（地址不存在视为成功）。
    let row = build_unicast_row(adapter, address);
    let result = unsafe { DeleteUnicastIpAddressEntry(&row) };
    if result == NO_ERROR || result == ERROR_NOT_FOUND {
        Ok(())
    } else {
        Err(NetworkError::Win32 {
            context: "DeleteUnicastIpAddressEntry",
            code: result,
        })
    }
}

fn build_unicast_row(
    adapter: AdapterInfo,
    address: &InterfaceAddress,
) -> MIB_UNICASTIPADDRESS_ROW {
    // 构造 MIB_UNICASTIPADDRESS_ROW。
    let mut row: MIB_UNICASTIPADDRESS_ROW = unsafe { std::mem::zeroed() };
    unsafe {
        InitializeUnicastIpAddressEntry(&mut row);
    }
    row.InterfaceIndex = adapter.if_index;
    row.InterfaceLuid = adapter.luid;
    row.Address = sockaddr_inet_from_ip(address.addr);
    row.OnLinkPrefixLength = address.cidr;
    row
}

fn add_route(entry: &RouteEntry) -> Result<(), NetworkError> {
    // 添加路由（CreateIpForwardEntry2）。
    let row = build_route_row(entry);
    let result = unsafe { CreateIpForwardEntry2(&row) };
    if result == NO_ERROR || is_already_exists(result) {
        Ok(())
    } else {
        Err(NetworkError::Win32 {
            context: "CreateIpForwardEntry2",
            code: result,
        })
    }
}

fn delete_route(entry: &RouteEntry) -> Result<(), NetworkError> {
    // 删除路由（DeleteIpForwardEntry2）。
    let row = build_route_row(entry);
    let result = unsafe { DeleteIpForwardEntry2(&row) };
    if result == NO_ERROR || result == ERROR_NOT_FOUND {
        Ok(())
    } else {
        Err(NetworkError::Win32 {
            context: "DeleteIpForwardEntry2",
            code: result,
        })
    }
}

fn build_route_row(entry: &RouteEntry) -> MIB_IPFORWARD_ROW2 {
    // 构造 MIB_IPFORWARD_ROW2，注意 NextHop 与 Metric。
    let mut row: MIB_IPFORWARD_ROW2 = unsafe { std::mem::zeroed() };
    unsafe {
        InitializeIpForwardEntry(&mut row);
    }
    row.InterfaceIndex = entry.if_index;
    row.InterfaceLuid = entry.luid;
    row.DestinationPrefix = IP_ADDRESS_PREFIX {
        Prefix: sockaddr_inet_from_ip(entry.dest),
        PrefixLength: entry.prefix,
    };
    let next_hop = entry.next_hop.unwrap_or_else(|| match entry.dest {
        IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
    });
    row.NextHop = sockaddr_inet_from_ip(next_hop);
    row.Metric = TUNNEL_METRIC;
    row.Protocol = MIB_IPPROTO_NETMGMT;
    row
}

fn sockaddr_inet_from_ip(addr: IpAddr) -> SOCKADDR_INET {
    // 将 IP 地址转换成 SOCKADDR_INET（供 IP Helper/WinSock 使用）。
    match addr {
        IpAddr::V4(addr) => sockaddr_inet_v4(addr),
        IpAddr::V6(addr) => sockaddr_inet_v6(addr),
    }
}

fn sockaddr_inet_v4(addr: Ipv4Addr) -> SOCKADDR_INET {
    // IPv4 写入 sin_addr 时，u32 需为“内存中的网络字节序”。
    // Windows 的 IN_ADDR.S_addr 期望“网络序字节的原样内存布局”，
    // 这里用 from_ne_bytes 保持字节顺序不被数值转换打乱。
    let mut sockaddr: SOCKADDR_IN = unsafe { std::mem::zeroed() };
    sockaddr.sin_family = AF_INET;
    let in_addr = IN_ADDR {
        S_un: IN_ADDR_0 {
            S_addr: u32::from_ne_bytes(addr.octets()),
        },
    };
    sockaddr.sin_addr = in_addr;
    SOCKADDR_INET { Ipv4: sockaddr }
}

fn sockaddr_inet_v6(addr: Ipv6Addr) -> SOCKADDR_INET {
    // IPv6 直接写入 16 字节数组即可。
    let mut sockaddr: SOCKADDR_IN6 = unsafe { std::mem::zeroed() };
    sockaddr.sin6_family = AF_INET6;
    sockaddr.sin6_addr = IN6_ADDR {
        u: IN6_ADDR_0 {
            Byte: addr.octets(),
        },
    };
    SOCKADDR_INET { Ipv6: sockaddr }
}

fn ip_from_sockaddr_inet(addr: &SOCKADDR_INET) -> Option<IpAddr> {
    // 从 SOCKADDR_INET 解析 IP（IPv4 需做字节序修正）。
    unsafe {
        if addr.si_family == AF_INET {
            let value = addr.Ipv4.sin_addr.S_un.S_addr;
            return Some(IpAddr::V4(Ipv4Addr::from(u32::from_be(value))));
        }
        if addr.si_family == AF_INET6 {
            let bytes = addr.Ipv6.sin6_addr.u.Byte;
            return Some(IpAddr::V6(Ipv6Addr::from(bytes)));
        }
    }
    None
}

fn ip_from_socket_address(addr: &SOCKET_ADDRESS) -> Option<IpAddr> {
    // 从 SOCKET_ADDRESS 解析 IP（用于枚举既有单播地址）。
    if addr.lpSockaddr.is_null() {
        return None;
    }
    unsafe {
        let sockaddr = &*addr.lpSockaddr;
        if sockaddr.sa_family == AF_INET {
            let sin = &*(addr.lpSockaddr as *const SOCKADDR_IN);
            let value = sin.sin_addr.S_un.S_addr;
            return Some(IpAddr::V4(Ipv4Addr::from(u32::from_be(value))));
        }
        if sockaddr.sa_family == AF_INET6 {
            let sin6 = &*(addr.lpSockaddr as *const SOCKADDR_IN6);
            let bytes = sin6.sin6_addr.u.Byte;
            return Some(IpAddr::V6(Ipv6Addr::from(bytes)));
        }
    }
    None
}

fn is_link_local(addr: IpAddr) -> bool {
    // 判断是否为 link-local（应保留，不应删除）。
    match addr {
        IpAddr::V4(addr) => addr.is_link_local(),
        IpAddr::V6(addr) => addr.is_unicast_link_local(),
    }
}

fn cleanup_stale_unicast_addresses(
    adapter: AdapterInfo,
    desired: &[InterfaceAddress],
) -> Result<(), NetworkError> {
    // 清理隧道接口上“非本次配置”的地址，避免残留地址导致路由异常。
    let desired_set: HashSet<(IpAddr, u8)> = desired
        .iter()
        .map(|address| (address.addr, address.cidr))
        .collect();
    let existing = list_unicast_addresses(adapter)?;
    let mut removed = 0usize;
    for (ip, prefix) in existing {
        if is_link_local(ip) {
            continue;
        }
        if desired_set.contains(&(ip, prefix)) {
            continue;
        }
        log_net(format!("address remove: {}/{}", ip, prefix));
        let address = InterfaceAddress { addr: ip, cidr: prefix };
        delete_unicast_address(adapter, &address)?;
        removed += 1;
    }
    if removed > 0 {
        log_net(format!("stale address cleanup removed={removed}"));
    }
    Ok(())
}

fn list_unicast_addresses(
    adapter: AdapterInfo,
) -> Result<Vec<(IpAddr, u8)>, NetworkError> {
    // 遍历指定适配器的单播地址（依赖 GetAdaptersAddresses）。
    let mut size = 0u32;
    let family = AF_UNSPEC.0 as u32;
    let flags = GAA_FLAG_SKIP_ANYCAST
        | GAA_FLAG_SKIP_MULTICAST
        | GAA_FLAG_SKIP_DNS_SERVER
        | GAA_FLAG_INCLUDE_PREFIX;

    let mut result = unsafe { GetAdaptersAddresses(family, flags, None, None, &mut size) };
    if result != ERROR_BUFFER_OVERFLOW.0 && result != NO_ERROR.0 {
        return Err(NetworkError::Win32 {
            context: "GetAdaptersAddresses(size)",
            code: WIN32_ERROR(result),
        });
    }

    let mut buffer = vec![0u8; size as usize];
    let ptr = buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;
    result = unsafe { GetAdaptersAddresses(family, flags, None, Some(ptr), &mut size) };
    if result != NO_ERROR.0 {
        return Err(NetworkError::Win32 {
            context: "GetAdaptersAddresses(data)",
            code: WIN32_ERROR(result),
        });
    }

    let mut list = Vec::new();
    let mut adapter_ptr = ptr;
    unsafe {
        while !adapter_ptr.is_null() {
            let current = &*adapter_ptr;
            if current.Anonymous1.Anonymous.IfIndex == adapter.if_index {
                let mut unicast: *mut IP_ADAPTER_UNICAST_ADDRESS_LH =
                    current.FirstUnicastAddress;
                while !unicast.is_null() {
                    let entry = &*unicast;
                    if let Some(ip) = ip_from_socket_address(&entry.Address) {
                        list.push((ip, entry.OnLinkPrefixLength));
                    }
                    unicast = entry.Next;
                }
                break;
            }
            adapter_ptr = current.Next;
        }
    }

    Ok(list)
}

fn collect_allowed_routes(peers: &[PeerConfig]) -> Vec<AllowedIp> {
    // 汇总 peers 中的 AllowedIPs，去重后返回。
    let mut seen = HashSet::new();
    let mut routes = Vec::new();
    for peer in peers {
        for allowed in &peer.allowed_ips {
            if seen.insert((allowed.addr, allowed.cidr)) {
                routes.push(AllowedIp {
                    addr: allowed.addr,
                    cidr: allowed.cidr,
                });
            }
        }
    }
    routes
}

fn detect_full_tunnel(routes: &[AllowedIp]) -> (bool, bool) {
    // 判断是否存在 0.0.0.0/0 或 ::/0（全隧道）。
    let mut v4 = false;
    let mut v6 = false;
    for route in routes {
        match route.addr {
            IpAddr::V4(addr) if addr.is_unspecified() && route.cidr == 0 => v4 = true,
            IpAddr::V6(addr) if addr.is_unspecified() && route.cidr == 0 => v6 = true,
            _ => {}
        }
    }
    (v4, v6)
}

fn is_default_route(route: &AllowedIp) -> bool {
    // 是否为默认路由（0.0.0.0/0 或 ::/0）。
    route.addr.is_unspecified() && route.cidr == 0
}

async fn resolve_endpoint_ips(peers: &[PeerConfig]) -> Result<Vec<IpAddr>, NetworkError> {
    // 解析 Endpoint 的所有 IP（支持域名与 IPv6）。
    let mut seen = HashSet::new();
    for peer in peers {
        let Some(endpoint) = &peer.endpoint else {
            continue;
        };
        let host = endpoint.host.trim();
        if host.is_empty() {
            continue;
        }
        let lookup = tokio::net::lookup_host((host, endpoint.port))
            .await
            .map_err(|_| {
                NetworkError::EndpointResolve(format!("failed to resolve {host}"))
            })?;
        for addr in lookup {
            seen.insert(addr.ip());
        }
    }
    Ok(seen.into_iter().collect())
}

fn best_route_to(ip: IpAddr) -> Result<RouteEntry, NetworkError> {
    // 通过 GetBestRoute2 查询到目标 IP 的系统路由，
    // 用于构建“bypass route”，确保 Endpoint 仍走物理网卡。
    let dest = sockaddr_inet_from_ip(ip);
    let mut row: MIB_IPFORWARD_ROW2 = unsafe { std::mem::zeroed() };
    let mut best_source: SOCKADDR_INET = unsafe { std::mem::zeroed() };
    let result = unsafe {
        GetBestRoute2(
            None,
            0,
            None,
            &dest,
            0,
            &mut row,
            &mut best_source,
        )
    };
    if result != NO_ERROR {
        return Err(NetworkError::Win32 {
            context: "GetBestRoute2",
            code: result,
        });
    }

    let next_hop = ip_from_sockaddr_inet(&row.NextHop);
    Ok(RouteEntry {
        dest: ip,
        prefix: if ip.is_ipv4() { 32 } else { 128 },
        next_hop,
        if_index: row.InterfaceIndex,
        luid: row.InterfaceLuid,
    })
}

fn is_already_exists(code: WIN32_ERROR) -> bool {
    // Windows 对“已存在”的错误码有多个取值。
    code == ERROR_OBJECT_ALREADY_EXISTS || code == ERROR_ALREADY_EXISTS
}

fn set_interface_metric(
    adapter: AdapterInfo,
    family: ADDRESS_FAMILY,
    metric: u32,
) -> Result<InterfaceMetricState, NetworkError> {
    // 将接口 metric 设为固定值（关闭自动 metric），并返回旧值用于恢复。
    let mut row: MIB_IPINTERFACE_ROW = unsafe { std::mem::zeroed() };
    unsafe {
        InitializeIpInterfaceEntry(&mut row);
    }
    row.Family = family;
    row.InterfaceLuid = adapter.luid;
    row.InterfaceIndex = adapter.if_index;

    let result = unsafe { GetIpInterfaceEntry(&mut row) };
    if result != NO_ERROR {
        return Err(NetworkError::Win32 {
            context: "GetIpInterfaceEntry",
            code: result,
        });
    }

    let previous = InterfaceMetricState {
        family,
        use_auto: row.UseAutomaticMetric,
        metric: row.Metric,
    };

    row.UseAutomaticMetric = BOOLEAN(0);
    row.Metric = metric;

    let result = unsafe { SetIpInterfaceEntry(&mut row) };
    if result != NO_ERROR {
        return Err(NetworkError::Win32 {
            context: "SetIpInterfaceEntry",
            code: result,
        });
    }

    Ok(previous)
}

fn restore_interface_metric(
    adapter: AdapterInfo,
    state: InterfaceMetricState,
) -> Result<(), NetworkError> {
    // 恢复接口 metric 到修改前的值。
    let mut row: MIB_IPINTERFACE_ROW = unsafe { std::mem::zeroed() };
    unsafe {
        InitializeIpInterfaceEntry(&mut row);
    }
    row.Family = state.family;
    row.InterfaceLuid = adapter.luid;
    row.InterfaceIndex = adapter.if_index;

    let result = unsafe { GetIpInterfaceEntry(&mut row) };
    if result != NO_ERROR {
        return Err(NetworkError::Win32 {
            context: "GetIpInterfaceEntry",
            code: result,
        });
    }

    row.UseAutomaticMetric = state.use_auto;
    row.Metric = state.metric;

    let result = unsafe { SetIpInterfaceEntry(&mut row) };
    if result != NO_ERROR {
        return Err(NetworkError::Win32 {
            context: "SetIpInterfaceEntry",
            code: result,
        });
    }

    Ok(())
}
