//! 适配器地址的增删与清理逻辑（IPv4/IPv6 单播地址）。
//!
//! 目标：只保留本次配置需要的地址，移除历史残留地址，避免路由决策异常。

use std::collections::HashSet;
use std::net::IpAddr;

use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_NOT_FOUND, NO_ERROR, WIN32_ERROR};
use windows::Win32::NetworkManagement::IpHelper::{
    CreateUnicastIpAddressEntry, DeleteUnicastIpAddressEntry, GetAdaptersAddresses,
    InitializeUnicastIpAddressEntry, GAA_FLAG_INCLUDE_PREFIX, GAA_FLAG_SKIP_ANYCAST,
    GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST, IP_ADAPTER_ADDRESSES_LH,
    IP_ADAPTER_UNICAST_ADDRESS_LH, MIB_UNICASTIPADDRESS_ROW,
};
use windows::Win32::Networking::WinSock::AF_UNSPEC;

use crate::backend::wg::config::InterfaceAddress;

use super::adapter::AdapterInfo;
use super::sockaddr::{ip_from_socket_address, sockaddr_inet_from_ip};
use super::{is_already_exists, NetworkError};
use crate::log::events::net as log_net;

pub(super) fn add_unicast_address(
    adapter: AdapterInfo,
    address: &InterfaceAddress,
) -> Result<(), NetworkError> {
    // CreateUnicastIpAddressEntry 按 ifIndex/LUID 绑定地址。
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

pub(super) fn delete_unicast_address(
    adapter: AdapterInfo,
    address: &InterfaceAddress,
) -> Result<(), NetworkError> {
    // 删除指定地址；若不存在则视为成功。
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

pub(super) fn cleanup_stale_unicast_addresses(
    adapter: AdapterInfo,
    desired: &[InterfaceAddress],
) -> Result<(), NetworkError> {
    // 与当前配置对比，删除“非本次配置”的地址。
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
        log_net::address_remove(ip, prefix);
        let address = InterfaceAddress {
            addr: ip,
            cidr: prefix,
        };
        delete_unicast_address(adapter, &address)?;
        removed += 1;
    }
    if removed > 0 {
        log_net::stale_address_cleanup_removed(removed);
    }
    Ok(())
}

fn build_unicast_row(adapter: AdapterInfo, address: &InterfaceAddress) -> MIB_UNICASTIPADDRESS_ROW {
    // 构造 IP Helper 结构体，包含 ifIndex/LUID 与前缀长度。
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

fn is_link_local(addr: IpAddr) -> bool {
    // 保留 link-local 地址，避免影响系统自带链路地址。
    match addr {
        IpAddr::V4(addr) => addr.is_link_local(),
        IpAddr::V6(addr) => addr.is_unicast_link_local(),
    }
}

fn list_unicast_addresses(adapter: AdapterInfo) -> Result<Vec<(IpAddr, u8)>, NetworkError> {
    // 遍历适配器现有单播地址，用于后续差异化清理。
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
                let mut unicast: *mut IP_ADAPTER_UNICAST_ADDRESS_LH = current.FirstUnicastAddress;
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
