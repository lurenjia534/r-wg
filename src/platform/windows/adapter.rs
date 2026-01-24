//! Windows 适配器查找与标识信息提取。
//!
//! 这里的关键点是确定用于 DNS API 的接口 GUID：
//! - AdapterName（形如 `\\DEVICE\\TCPIP_{GUID}`）通常对应 DNS 接口 GUID。
//! - NetworkGuid 并不总是等同于 AdapterName GUID，因此需要解析并保留回退值。

use std::ffi::CStr;
use std::time::Duration;

use tokio::time::sleep;
use windows::core::{GUID, PSTR};
use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, NO_ERROR, WIN32_ERROR};
use windows::Win32::NetworkManagement::IpHelper::{
    GetAdaptersAddresses, IP_ADAPTER_ADDRESSES_LH, GAA_FLAG_SKIP_ANYCAST,
    GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST,
};
use windows::Win32::NetworkManagement::Ndis::NET_LUID_LH;
use windows::Win32::Networking::WinSock::AF_UNSPEC;

use super::{log_net, pwstr_to_string, NetworkError};

const ADAPTER_RETRY_COUNT: usize = 10;
const ADAPTER_RETRY_DELAY: Duration = Duration::from_millis(200);

#[derive(Clone, Copy)]
pub(super) struct AdapterInfo {
    /// ifIndex：用于路由/地址绑定（IP Helper API 的常用标识）。
    pub(super) if_index: u32,
    /// NET_LUID：部分 API 需要 LUID 而非 ifIndex。
    pub(super) luid: NET_LUID_LH,
    /// DNS 接口首选 GUID（优先来自 AdapterName）。
    pub(super) guid: GUID,
    /// DNS 接口回退 GUID（必要时使用 NetworkGuid）。
    pub(super) dns_guid_fallback: Option<GUID>,
}

pub(super) async fn find_adapter_with_retry(name: &str) -> Result<AdapterInfo, NetworkError> {
    // Wintun 创建后可能短暂不可见，做重试等待。
    for _ in 0..ADAPTER_RETRY_COUNT {
        if let Some(adapter) = find_adapter_by_name(name)? {
            return Ok(adapter);
        }
        sleep(ADAPTER_RETRY_DELAY).await;
    }
    Err(NetworkError::AdapterNotFound(name.to_string()))
}

fn find_adapter_by_name(name: &str) -> Result<Option<AdapterInfo>, NetworkError> {
    // 通过 GetAdaptersAddresses 遍历系统适配器，并用 FriendlyName 精确匹配。
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
                // AdapterName 通常为 "\\DEVICE\\TCPIP_{GUID}"，解析出 GUID 更可靠。
                let adapter_name = pstr_to_string(current.AdapterName);
                let guid = match extract_guid_from_adapter_name(&adapter_name) {
                    Some(guid) => guid,
                    None => {
                        log_net("adapter guid parse failed, using NetworkGuid".to_string());
                        current.NetworkGuid
                    }
                };
                // 当 AdapterName GUID 与 NetworkGuid 不一致时保留回退值。
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

fn pstr_to_string(ptr: PSTR) -> String {
    // 适配器名是 ANSI 字符串（PSTR），无需 UTF-16 处理。
    if ptr.0.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(ptr.0 as *const i8).to_string_lossy().into_owned() }
}

fn extract_guid_from_adapter_name(name: &str) -> Option<GUID> {
    // 允许输入包含 {GUID} 或纯 GUID 的形式。
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
    // 只做格式校验：长度 36 且连字符位置固定。
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
