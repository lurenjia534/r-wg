//! Windows DNS 配置与回滚。
//!
//! 设计目标：
//! - 使用 `SetInterfaceDnsSettings` 直接按接口 GUID 写入 DNS；
//! - 同时兼容主 GUID 与回退 GUID（不同驱动/系统下可能不一致）；
//! - 在写入 DNS 时启用 `DisableUnconstrainedQueries`，降低 Windows 回退到其他网卡 DNS 的概率。

use std::net::IpAddr;

use windows::core::{GUID, PWSTR};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, NO_ERROR};
use windows::Win32::NetworkManagement::IpHelper::{
    FreeInterfaceDnsSettings, GetInterfaceDnsSettings, SetInterfaceDnsSettings,
    DNS_INTERFACE_SETTINGS, DNS_INTERFACE_SETTINGS3, DNS_INTERFACE_SETTINGS_VERSION3,
    DNS_SETTING_DISABLE_UNCONSTRAINED_QUERIES, DNS_SETTING_NAMESERVER, DNS_SETTING_SEARCHLIST,
};

use super::adapter::AdapterInfo;
use super::{pwstr_to_string, NetworkError};
use crate::log::events::dns as log_dns;

/// 单个 GUID 的 DNS 变更快照。
#[derive(Clone)]
struct DnsStateEntry {
    /// 变更目标接口 GUID。
    guid: GUID,
    /// 本次是否写入过 NameServer。
    touched_nameserver: bool,
    /// 本次是否写入过 SearchList。
    touched_search: bool,
    /// 本次是否写入过 DisableUnconstrainedQueries。
    touched_disable_unconstrained_queries: bool,
    /// 写入前 NameServer 原值（用于回滚）。
    original_nameserver: Option<String>,
    /// 写入前 SearchList 原值（用于回滚）。
    original_searchlist: Option<String>,
    /// 写入前 DisableUnconstrainedQueries 原值（用于回滚）。
    original_disable_unconstrained_queries: u32,
}

/// 本次 DNS 应用操作的完整状态。
#[derive(Clone)]
pub(super) struct DnsState {
    /// 可能包含 1~2 个 GUID 的写入记录；清理时按逆序回滚。
    entries: Vec<DnsStateEntry>,
}

/// 应用 DNS 到 Windows 接口。
///
/// 策略：
/// 1. 优先写主 GUID；
/// 2. 主 GUID 失败时使用 fallback GUID 兜底；
/// 3. 主 GUID 成功且 fallback GUID 不同，则再做一次 best-effort 补写。
pub(super) fn apply_dns(
    adapter: AdapterInfo,
    servers: &[IpAddr],
    search: &[String],
) -> Result<DnsState, NetworkError> {
    let mut entries = Vec::new();

    match apply_dns_with_guid(adapter.guid, servers, search) {
        Ok(state) => entries.push(state),
        Err(primary_err) => {
            if let Some(fallback) = adapter.dns_guid_fallback {
                if fallback != adapter.guid {
                    log_dns::apply_retry_fallback_guid();
                    let fallback_state = apply_dns_with_guid(fallback, servers, search)?;
                    entries.push(fallback_state);
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
            if let Ok(state) = apply_dns_with_guid(fallback, servers, search) {
                entries.push(state);
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
) -> Result<DnsStateEntry, NetworkError> {
    let (original_nameserver, original_searchlist, original_disable_unconstrained_queries) =
        read_interface_dns_settings(guid)?;

    let search_items: Vec<String> = search
        .iter()
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .map(|item| item.to_string())
        .collect();

    let touched_nameserver = !servers.is_empty();
    let touched_search = !search_items.is_empty();
    // 只要我们接管了 DNS（NameServer/Search 任一项），就开启禁用跨接口查询。
    let touched_disable_unconstrained_queries = touched_nameserver || touched_search;

    if !touched_nameserver && !touched_search && !touched_disable_unconstrained_queries {
        return Ok(DnsStateEntry {
            guid,
            touched_nameserver,
            touched_search,
            touched_disable_unconstrained_queries,
            original_nameserver,
            original_searchlist,
            original_disable_unconstrained_queries,
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

    // 安全护栏：如果要求禁用跨接口查询，必须确认系统已实际生效。
    if touched_disable_unconstrained_queries {
        let (_, _, effective_disable_unconstrained_queries) = read_interface_dns_settings(guid)?;
        if effective_disable_unconstrained_queries == 0 {
            return Err(NetworkError::UnsafeRouting(
                "DisableUnconstrainedQueries did not persist; refusing to continue to avoid DNS leak"
                    .to_string(),
            ));
        }
    }

    Ok(DnsStateEntry {
        guid,
        touched_nameserver,
        touched_search,
        touched_disable_unconstrained_queries,
        original_nameserver,
        original_searchlist,
        original_disable_unconstrained_queries,
    })
}

/// 读取接口当前 DNS 设置。
///
/// `ERROR_FILE_NOT_FOUND` 视为“尚无记录”，按空值处理。
fn read_interface_dns_settings(
    guid: GUID,
) -> Result<(Option<String>, Option<String>, u32), NetworkError> {
    let mut settings = DNS_INTERFACE_SETTINGS3::default();
    settings.Version = DNS_INTERFACE_SETTINGS_VERSION3;

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

    Ok((nameserver, searchlist, disable_unconstrained_queries))
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
