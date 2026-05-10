//! Windows DNS 配置与回滚。
//!
//! 设计目标：
//! - 使用 `SetInterfaceDnsSettings` 按接口 GUID 写入 DNS；
//! - 同时兼容主 GUID 与 fallback GUID；
//! - 写入 DNS 时开启 `DisableUnconstrainedQueries`，降低跨接口 DNS 回退概率；
//! - 写后回读校验关键字段，避免“看似成功、实际未生效”的隐患。

use std::net::IpAddr;

use serde::{Deserialize, Serialize};
use windows::core::{GUID, PWSTR};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, NO_ERROR};
use windows::Win32::NetworkManagement::IpHelper::{
    FreeInterfaceDnsSettings, GetInterfaceDnsSettings, SetInterfaceDnsSettings,
    DNS_INTERFACE_SETTINGS, DNS_INTERFACE_SETTINGS3, DNS_INTERFACE_SETTINGS_VERSION3,
    DNS_SETTING_DISABLE_UNCONSTRAINED_QUERIES, DNS_SETTING_IPV6, DNS_SETTING_NAMESERVER,
    DNS_SETTING_SEARCHLIST,
};

use super::adapter::{guid_to_string, AdapterInfo};
use super::{pwstr_to_string, NetworkError};
use crate::log::events::dns as log_dns;

/// 单个 GUID 的 DNS 变更快照。
#[derive(Clone)]
struct DnsStateEntry {
    /// 变更目标接口 GUID。
    guid: GUID,
    /// 本条记录针对的 DNS 地址族。
    family: DnsFamily,
    /// 本次是否修改了 NameServer。
    touched_nameserver: bool,
    /// 本次是否修改了 SearchList。
    touched_search: bool,
    /// 本次是否修改了 DisableUnconstrainedQueries。
    touched_disable_unconstrained_queries: bool,
    /// 修改前 NameServer（用于回滚）。
    original_nameserver: Option<String>,
    /// 修改前 SearchList（用于回滚）。
    original_searchlist: Option<String>,
    /// 修改前 DisableUnconstrainedQueries（用于回滚）。
    original_disable_unconstrained_queries: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DnsFamily {
    Ipv4,
    Ipv6,
}

impl DnsFamily {
    fn is_ipv6(self) -> bool {
        matches!(self, Self::Ipv6)
    }

    fn flag(self) -> u64 {
        if self.is_ipv6() {
            DNS_SETTING_IPV6 as u64
        } else {
            0
        }
    }
}

/// 一次 DNS 应用操作的完整状态。
#[derive(Clone)]
pub(super) struct DnsState {
    /// 可能包含 1~2 个 GUID 的写入记录；回滚按逆序执行。
    entries: Vec<DnsStateEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct DnsStateSnapshot {
    entries: Vec<DnsStateEntrySnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DnsStateEntrySnapshot {
    guid: String,
    #[serde(default)]
    ipv6: bool,
    touched_nameserver: bool,
    touched_search: bool,
    #[serde(default)]
    touched_disable_unconstrained_queries: bool,
    original_nameserver: Option<String>,
    original_searchlist: Option<String>,
    #[serde(default)]
    original_disable_unconstrained_queries: u32,
}

impl DnsState {
    pub(super) fn snapshot(&self) -> DnsStateSnapshot {
        DnsStateSnapshot {
            entries: self
                .entries
                .iter()
                .map(|entry| DnsStateEntrySnapshot {
                    guid: guid_to_string(entry.guid),
                    ipv6: entry.family.is_ipv6(),
                    touched_nameserver: entry.touched_nameserver,
                    touched_search: entry.touched_search,
                    touched_disable_unconstrained_queries: entry
                        .touched_disable_unconstrained_queries,
                    original_nameserver: entry.original_nameserver.clone(),
                    original_searchlist: entry.original_searchlist.clone(),
                    original_disable_unconstrained_queries: entry
                        .original_disable_unconstrained_queries,
                })
                .collect(),
        }
    }
}

impl DnsStateSnapshot {
    pub(super) fn to_state(&self) -> Result<DnsState, std::io::Error> {
        let mut entries = Vec::with_capacity(self.entries.len());
        for entry in &self.entries {
            let guid = GUID::try_from(entry.guid.as_str()).map_err(|err| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, err.to_string())
            })?;
            entries.push(DnsStateEntry {
                guid,
                family: if entry.ipv6 {
                    DnsFamily::Ipv6
                } else {
                    DnsFamily::Ipv4
                },
                touched_nameserver: entry.touched_nameserver,
                touched_search: entry.touched_search,
                touched_disable_unconstrained_queries: entry.touched_disable_unconstrained_queries,
                original_nameserver: entry.original_nameserver.clone(),
                original_searchlist: entry.original_searchlist.clone(),
                original_disable_unconstrained_queries: entry
                    .original_disable_unconstrained_queries,
            });
        }
        Ok(DnsState { entries })
    }
}

/// 应用 DNS 到 Windows 接口。
///
/// 策略：
/// 1. 优先写主 GUID；
/// 2. 主 GUID 失败时尝试 fallback GUID；
/// 3. 主 GUID 成功且 fallback 不同，额外 best-effort 补写 fallback。
pub(super) fn apply_dns(
    adapter: AdapterInfo,
    servers: &[IpAddr],
    search: &[String],
) -> Result<DnsState, NetworkError> {
    let mut entries = Vec::new();

    match apply_dns_with_guid(adapter.guid, servers, search) {
        Ok(states) => entries.extend(states),
        Err(primary_err) => {
            if let Some(fallback) = adapter.dns_guid_fallback {
                if fallback != adapter.guid {
                    log_dns::apply_retry_fallback_guid();
                    let fallback_states = apply_dns_with_guid(fallback, servers, search)?;
                    entries.extend(fallback_states);
                } else {
                    return Err(primary_err);
                }
            } else {
                return Err(primary_err);
            }
        }
    }

    if let Some(fallback) = adapter.dns_guid_fallback {
        if fallback != adapter.guid && !entries.iter().any(|entry| entry.guid == fallback) {
            // 主 GUID 已成功时，fallback 仅做增强，不影响主流程成功结果。
            log_dns::apply_retry_fallback_guid();
            if let Ok(states) = apply_dns_with_guid(fallback, servers, search) {
                entries.extend(states);
            }
        }
    }

    Ok(DnsState { entries })
}

/// 回滚 DNS 配置（尽力回滚，返回第一条错误）。
pub(super) fn cleanup_dns(state: DnsState) -> Result<(), NetworkError> {
    let mut first_error: Option<NetworkError> = None;

    for entry in state.entries.iter().rev() {
        if let Err(err) = cleanup_dns_entry(entry) {
            if first_error.is_none() {
                first_error = Some(err);
            }
        }
    }

    if let Some(err) = first_error {
        Err(err)
    } else {
        Ok(())
    }
}

/// 回滚单个 GUID 的 DNS 字段。
fn cleanup_dns_entry(state: &DnsStateEntry) -> Result<(), NetworkError> {
    if !state.touched_nameserver
        && !state.touched_search
        && !state.touched_disable_unconstrained_queries
    {
        return Ok(());
    }

    let mut flags = state.family.flag();

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

    let mut disable_unconstrained_queries = 0u32;
    if state.touched_disable_unconstrained_queries {
        flags |= DNS_SETTING_DISABLE_UNCONSTRAINED_QUERIES as u64;
        disable_unconstrained_queries = state.original_disable_unconstrained_queries;
    }

    if flags == 0 {
        return Ok(());
    }

    let settings = DNS_INTERFACE_SETTINGS3 {
        Version: DNS_INTERFACE_SETTINGS_VERSION3,
        Flags: flags,
        Domain: PWSTR(std::ptr::null_mut()),
        NameServer: pwstr_from_buf(&nameserver_buf),
        SearchList: pwstr_from_buf(&search_buf),
        RegistrationEnabled: 0,
        RegisterAdapterName: 0,
        EnableLLMNR: 0,
        QueryAdapterName: 0,
        ProfileNameServer: PWSTR(std::ptr::null_mut()),
        DisableUnconstrainedQueries: disable_unconstrained_queries,
        SupplementalSearchList: PWSTR(std::ptr::null_mut()),
        cServerProperties: 0,
        ServerProperties: std::ptr::null_mut(),
        cProfileServerProperties: 0,
        ProfileServerProperties: std::ptr::null_mut(),
    };

    let result = unsafe {
        SetInterfaceDnsSettings(
            state.guid,
            &settings as *const DNS_INTERFACE_SETTINGS3 as *const DNS_INTERFACE_SETTINGS,
        )
    };
    if result != NO_ERROR {
        return Err(NetworkError::Win32 {
            context: "SetInterfaceDnsSettings(revert)",
            code: result,
        });
    }

    Ok(())
}

/// 向单个 GUID 写入 DNS，并返回回滚快照。
fn apply_dns_with_guid(
    guid: GUID,
    servers: &[IpAddr],
    search: &[String],
) -> Result<Vec<DnsStateEntry>, NetworkError> {
    let v4_servers: Vec<IpAddr> = servers.iter().copied().filter(IpAddr::is_ipv4).collect();
    let v6_servers: Vec<IpAddr> = servers.iter().copied().filter(IpAddr::is_ipv6).collect();

    let mut entries = Vec::new();
    if !v4_servers.is_empty() || !search.is_empty() {
        entries.push(apply_dns_family_with_guid(
            guid,
            DnsFamily::Ipv4,
            &v4_servers,
            search,
        )?);
    }
    if !v6_servers.is_empty() {
        match apply_dns_family_with_guid(guid, DnsFamily::Ipv6, &v6_servers, &[]) {
            Ok(entry) => entries.push(entry),
            Err(err) => {
                for entry in entries.iter().rev() {
                    let _ = cleanup_dns_entry(entry);
                }
                return Err(err);
            }
        }
    }
    Ok(entries)
}

fn apply_dns_family_with_guid(
    guid: GUID,
    family: DnsFamily,
    servers: &[IpAddr],
    search: &[String],
) -> Result<DnsStateEntry, NetworkError> {
    let (original_nameserver, original_searchlist, original_disable_unconstrained_queries) =
        read_interface_dns_settings(guid, family)?;

    let search_items: Vec<String> = search
        .iter()
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .map(|item| item.to_string())
        .collect();

    let touched_nameserver = !servers.is_empty();
    let touched_search = !search_items.is_empty();
    // 只要我们接管了 DNS（NameServer/Search 任一项），就启用禁用跨接口查询。
    let touched_disable_unconstrained_queries = touched_nameserver || touched_search;

    if !touched_nameserver && !touched_search && !touched_disable_unconstrained_queries {
        return Ok(DnsStateEntry {
            guid,
            family,
            touched_nameserver,
            touched_search,
            touched_disable_unconstrained_queries,
            original_nameserver,
            original_searchlist,
            original_disable_unconstrained_queries,
        });
    }

    let mut flags = family.flag();

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

    let mut disable_unconstrained_queries = 0u32;
    if touched_disable_unconstrained_queries {
        flags |= DNS_SETTING_DISABLE_UNCONSTRAINED_QUERIES as u64;
        disable_unconstrained_queries = 1;
    }

    let settings = DNS_INTERFACE_SETTINGS3 {
        Version: DNS_INTERFACE_SETTINGS_VERSION3,
        Flags: flags,
        Domain: PWSTR(std::ptr::null_mut()),
        NameServer: pwstr_from_buf(&nameserver_buf),
        SearchList: pwstr_from_buf(&search_buf),
        RegistrationEnabled: 0,
        RegisterAdapterName: 0,
        EnableLLMNR: 0,
        QueryAdapterName: 0,
        ProfileNameServer: PWSTR(std::ptr::null_mut()),
        DisableUnconstrainedQueries: disable_unconstrained_queries,
        SupplementalSearchList: PWSTR(std::ptr::null_mut()),
        cServerProperties: 0,
        ServerProperties: std::ptr::null_mut(),
        cProfileServerProperties: 0,
        ProfileServerProperties: std::ptr::null_mut(),
    };

    let result = unsafe {
        SetInterfaceDnsSettings(
            guid,
            &settings as *const DNS_INTERFACE_SETTINGS3 as *const DNS_INTERFACE_SETTINGS,
        )
    };
    if result != NO_ERROR {
        return Err(NetworkError::Win32 {
            context: "SetInterfaceDnsSettings",
            code: result,
        });
    }

    let entry = DnsStateEntry {
        guid,
        family,
        touched_nameserver,
        touched_search,
        touched_disable_unconstrained_queries,
        original_nameserver,
        original_searchlist,
        original_disable_unconstrained_queries,
    };

    if let Err(err) = verify_dns_family(guid, family, servers, &search_items) {
        let _ = cleanup_dns_entry(&entry);
        return Err(err);
    }

    Ok(entry)
}

/// 读取接口当前 DNS 设置。
///
/// `ERROR_FILE_NOT_FOUND` 视为“尚无记录”，按空值处理。
fn read_interface_dns_settings(
    guid: GUID,
    family: DnsFamily,
) -> Result<(Option<String>, Option<String>, u32), NetworkError> {
    let mut settings = DNS_INTERFACE_SETTINGS3::default();
    settings.Version = DNS_INTERFACE_SETTINGS_VERSION3;
    // IPv6 rollback is only safe if the API returns IPv6-family settings here.
    // The family validator below makes this fail closed if Windows ignores the flag.
    settings.Flags = family.flag();

    let result = unsafe {
        GetInterfaceDnsSettings(
            guid,
            &mut settings as *mut DNS_INTERFACE_SETTINGS3 as *mut DNS_INTERFACE_SETTINGS,
        )
    };
    if result == ERROR_FILE_NOT_FOUND {
        log_dns::settings_not_found();
        return Ok((None, None, 0));
    }
    if result != NO_ERROR {
        return Err(NetworkError::Win32 {
            context: "GetInterfaceDnsSettings",
            code: result,
        });
    }

    let nameserver = normalize_pwstr(settings.NameServer);
    let searchlist = normalize_pwstr(settings.SearchList);
    let disable_unconstrained_queries = settings.DisableUnconstrainedQueries;

    unsafe {
        FreeInterfaceDnsSettings(
            &mut settings as *mut DNS_INTERFACE_SETTINGS3 as *mut DNS_INTERFACE_SETTINGS,
        );
    }

    validate_nameserver_family(family, nameserver.as_deref(), "GetInterfaceDnsSettings")?;

    Ok((nameserver, searchlist, disable_unconstrained_queries))
}

fn verify_dns_family(
    guid: GUID,
    family: DnsFamily,
    expected_servers: &[IpAddr],
    expected_search: &[String],
) -> Result<(), NetworkError> {
    let (effective_nameserver, effective_searchlist, effective_disable_unconstrained_queries) =
        read_interface_dns_settings(guid, family)?;

    if !expected_servers.is_empty() {
        let expected = expected_servers
            .iter()
            .map(|ip| ip.to_string())
            .collect::<Vec<_>>()
            .join(",");
        if effective_nameserver.as_deref() != Some(expected.as_str()) {
            return Err(NetworkError::UnsafeRouting(format!(
                "{family:?} DNS NameServer did not persist; expected {expected:?}, got {effective_nameserver:?}"
            )));
        }
    }

    if !expected_search.is_empty() {
        let expected = expected_search.join(",");
        if effective_searchlist.as_deref() != Some(expected.as_str()) {
            return Err(NetworkError::UnsafeRouting(format!(
                "{family:?} DNS SearchList did not persist; expected {expected:?}, got {effective_searchlist:?}"
            )));
        }
    }

    if effective_disable_unconstrained_queries == 0 {
        return Err(NetworkError::UnsafeRouting(
            "DisableUnconstrainedQueries did not persist; refusing to continue to avoid DNS leak"
                .to_string(),
        ));
    }

    Ok(())
}

fn validate_nameserver_family(
    family: DnsFamily,
    nameserver: Option<&str>,
    context: &'static str,
) -> Result<(), NetworkError> {
    let Some(nameserver) = nameserver else {
        return Ok(());
    };

    for item in nameserver
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        let ip: IpAddr = item.parse().map_err(|_| {
            NetworkError::UnsafeRouting(format!(
                "{context} returned an unparsable {family:?} DNS server: {item}"
            ))
        })?;
        if family.is_ipv6() != ip.is_ipv6() {
            return Err(NetworkError::UnsafeRouting(format!(
                "{context} returned {ip} while reading {family:?} DNS settings"
            )));
        }
    }

    Ok(())
}

/// 把 Windows PWSTR 规范化为 `Option<String>`。
fn normalize_pwstr(ptr: PWSTR) -> Option<String> {
    let value = pwstr_to_string(ptr);
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// UTF-16 零结尾编码（用于 Win32 API）。
fn encode_utf16_z(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 将可选 UTF-16 缓冲区转换为 PWSTR。
fn pwstr_from_buf(buf: &Option<Vec<u16>>) -> PWSTR {
    match buf {
        Some(value) => PWSTR(value.as_ptr() as *mut u16),
        None => PWSTR(std::ptr::null_mut()),
    }
}
