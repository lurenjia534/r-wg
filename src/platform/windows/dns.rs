//! Windows DNS 配置与回滚。
//!
//! 使用 Get/SetInterfaceDnsSettings 按接口设置 DNS 服务器与 SearchList。
//! 当接口没有 DNS 记录时，GetInterfaceDnsSettings 可能返回 ERROR_FILE_NOT_FOUND，
//! 视作“空配置”而非致命错误。

use std::net::IpAddr;

use windows::core::{GUID, PWSTR};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, NO_ERROR};
use windows::Win32::NetworkManagement::IpHelper::{
    FreeInterfaceDnsSettings, GetInterfaceDnsSettings, SetInterfaceDnsSettings,
    DNS_INTERFACE_SETTINGS, DNS_INTERFACE_SETTINGS_VERSION1, DNS_SETTING_NAMESERVER,
    DNS_SETTING_SEARCHLIST,
};

use super::adapter::AdapterInfo;
use super::{log_net, pwstr_to_string, NetworkError};

#[derive(Clone)]
pub(super) struct DnsState {
    /// 实际使用的接口 GUID（可能为主 GUID 或回退 GUID）。
    guid: GUID,
    /// 是否修改过 NameServer 字段。
    touched_nameserver: bool,
    /// 是否修改过 SearchList 字段。
    touched_search: bool,
    /// 修改前的 NameServer 内容（原样保存，用于回滚）。
    original_nameserver: Option<String>,
    /// 修改前的 SearchList 内容（原样保存，用于回滚）。
    original_searchlist: Option<String>,
}

pub(super) fn apply_dns(
    adapter: AdapterInfo,
    servers: &[IpAddr],
    search: &[String],
) -> Result<DnsState, NetworkError> {
    // 首选 AdapterName GUID；失败时回退 NetworkGuid。
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

pub(super) fn cleanup_dns(state: DnsState) -> Result<(), NetworkError> {
    // 仅回滚本次触碰过的字段，避免误改用户其他 DNS 设置。
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
        // 以下字段不在本次变更范围内，保持为空以避免侧影响。
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

fn apply_dns_with_guid(
    guid: GUID,
    servers: &[IpAddr],
    search: &[String],
) -> Result<DnsState, NetworkError> {
    // 先读取原始设置，供失败回滚或停止时恢复。
    let (original_nameserver, original_searchlist) = read_interface_dns_settings(guid)?;

    // 清洗 search 列表，剔除空项。
    let search_items: Vec<String> = search
        .iter()
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .map(|item| item.to_string())
        .collect();

    let touched_nameserver = !servers.is_empty();
    let touched_search = !search_items.is_empty();

    if !touched_nameserver && !touched_search {
        // 没有任何实际改动，直接返回空操作状态。
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
        // DNS 服务器按逗号分隔，兼容 Windows 接口格式。
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
        // SearchList 同样用逗号拼接。
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

fn read_interface_dns_settings(
    guid: GUID,
) -> Result<(Option<String>, Option<String>), NetworkError> {
    // 读取当前接口 DNS 设置，失败时由调用方决定是否回退。
    let mut settings = DNS_INTERFACE_SETTINGS::default();
    settings.Version = DNS_INTERFACE_SETTINGS_VERSION1;

    let result = unsafe { GetInterfaceDnsSettings(guid, &mut settings) };
    if result == ERROR_FILE_NOT_FOUND {
        // 某些接口没有 DNS 记录，视为“空配置”。
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

    // GetInterfaceDnsSettings 可能分配内存，需要显式释放。
    unsafe {
        FreeInterfaceDnsSettings(&mut settings);
    }

    Ok((nameserver, searchlist))
}

fn normalize_pwstr(ptr: PWSTR) -> Option<String> {
    // 将空字符串统一归一为 None，避免误写入空值。
    let value = pwstr_to_string(ptr);
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn encode_utf16_z(value: &str) -> Vec<u16> {
    // Windows API 需要 NUL 结尾的 UTF-16 字符串。
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn pwstr_from_buf(buf: &Option<Vec<u16>>) -> PWSTR {
    // 从可选缓冲区生成 PWSTR，None 表示不写该字段。
    match buf {
        Some(value) => PWSTR(value.as_ptr() as *mut u16),
        None => PWSTR(std::ptr::null_mut()),
    }
}
