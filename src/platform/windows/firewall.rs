//! Windows DNS 防泄露规则，基于 WFP dynamic session。
//!
//! 设计思路：
//! - 仅在全隧道场景启用；
//! - 读取“非隧道网卡”的 DNS 服务器地址；
//! - 在 dynamic WFP session 里添加出站阻断 filter；
//! - 正常断开时显式关闭 session，异常退出时由 WFP 自动删除对象。

use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr};

use serde::{Deserialize, Serialize};
use windows::core::{GUID, HRESULT, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    FWP_E_ALREADY_EXISTS, FWP_E_FILTER_NOT_FOUND, HANDLE, NO_ERROR, WIN32_ERROR,
};
use windows::Win32::NetworkManagement::IpHelper::{
    GetAdaptersAddresses, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_MULTICAST, IP_ADAPTER_ADDRESSES_LH,
    IP_ADAPTER_DNS_SERVER_ADDRESS_XP,
};
use windows::Win32::NetworkManagement::WindowsFilteringPlatform::{
    FwpmEngineClose0, FwpmEngineOpen0, FwpmFilterAdd0, FwpmFilterDeleteById0, FwpmSubLayerAdd0,
    FwpmTransactionAbort0, FwpmTransactionBegin0, FwpmTransactionCommit0, FWPM_ACTION0,
    FWPM_CONDITION_IP_PROTOCOL, FWPM_CONDITION_IP_REMOTE_ADDRESS, FWPM_CONDITION_IP_REMOTE_PORT,
    FWPM_DISPLAY_DATA0, FWPM_FILTER0, FWPM_FILTER_CONDITION0, FWPM_FILTER_FLAG_CLEAR_ACTION_RIGHT,
    FWPM_LAYER_ALE_AUTH_CONNECT_V4, FWPM_LAYER_ALE_AUTH_CONNECT_V6, FWPM_SESSION0,
    FWPM_SESSION_FLAG_DYNAMIC, FWPM_SUBLAYER0, FWP_ACTION_BLOCK, FWP_CONDITION_VALUE0,
    FWP_CONDITION_VALUE0_0, FWP_EMPTY, FWP_MATCH_EQUAL, FWP_UINT16, FWP_UINT8,
    FWP_V4_ADDR_AND_MASK, FWP_V4_ADDR_MASK, FWP_V6_ADDR_AND_MASK, FWP_V6_ADDR_MASK, FWP_VALUE0,
};
use windows::Win32::Networking::WinSock::{AF_UNSPEC, IPPROTO_TCP, IPPROTO_UDP};
use windows::Win32::System::Rpc::RPC_C_AUTHN_WINNT;

use super::adapter::AdapterInfo;
use super::sockaddr::ip_from_socket_address;
use super::NetworkError;
use crate::log::events::net as log_net;

const DNS_GUARD_SUBLAYER_KEY: GUID = GUID::from_u128(0x2e9240ca_7531_4d1b_b2c1_1f1092bcd336);
const DNS_GUARD_SUBLAYER_WEIGHT: u16 = 0x8000;
const DNS_GUARD_PORT: u16 = 53;

/// DNS Guard 的可回滚状态。
pub(super) struct DnsGuardState {
    filter_ids: Vec<u64>,
    session: Option<WfpSession>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct DnsGuardStateSnapshot {
    #[serde(default)]
    filter_ids: Vec<u64>,
}

impl DnsGuardState {
    pub(super) fn snapshot(&self) -> DnsGuardStateSnapshot {
        DnsGuardStateSnapshot {
            filter_ids: self.filter_ids.clone(),
        }
    }
}

impl DnsGuardStateSnapshot {
    pub(super) fn to_state(&self) -> DnsGuardState {
        DnsGuardState {
            filter_ids: self.filter_ids.clone(),
            session: None,
        }
    }
}

/// 应用 DNS Guard。
///
/// 返回 `Ok(None)` 表示当前场景无需下发规则（例如非全隧道或没有可阻断目标）。
pub(super) fn apply_dns_guard(
    adapter: AdapterInfo,
    full_v4: bool,
    full_v6: bool,
    tunnel_dns_servers: &[IpAddr],
) -> Result<Option<DnsGuardState>, NetworkError> {
    if !full_v4 && !full_v6 {
        return Ok(None);
    }

    let blocked_dns_servers =
        collect_non_tunnel_dns_servers(adapter, full_v4, full_v6, tunnel_dns_servers)?;
    if blocked_dns_servers.is_empty() {
        return Ok(None);
    }

    let session = WfpSession::open_dynamic()?;
    let transaction = WfpTransaction::begin(session.handle())?;
    ensure_dns_guard_sublayer(session.handle())?;

    let mut filter_ids = Vec::with_capacity(blocked_dns_servers.len() * 2);
    for ip in &blocked_dns_servers {
        filter_ids.push(add_dns_block_filter(
            session.handle(),
            *ip,
            IPPROTO_UDP.0 as u8,
        )?);
        filter_ids.push(add_dns_block_filter(
            session.handle(),
            *ip,
            IPPROTO_TCP.0 as u8,
        )?);
    }

    transaction.commit()?;
    log_net::dns_guard_apply(blocked_dns_servers.len());

    Ok(Some(DnsGuardState {
        filter_ids,
        session: Some(session),
    }))
}

/// 清理 DNS Guard 规则。
pub(super) fn cleanup_dns_guard(state: DnsGuardState) -> Result<(), NetworkError> {
    if state.filter_ids.is_empty() {
        return Ok(());
    }

    match state.session {
        Some(session) => {
            let delete_result = delete_filters(session.handle(), &state.filter_ids);
            let close_result = session.close();
            delete_result.and(close_result)
        }
        None => {
            let session = WfpSession::open_static()?;
            let delete_result = delete_filters(session.handle(), &state.filter_ids);
            let close_result = session.close();
            delete_result.and(close_result)
        }
    }
}

/// dynamic session 会在 owning process 退出时自动删对象，因此不需要前缀扫描。
pub(super) fn cleanup_stale_dns_guard_rules() -> Result<(), NetworkError> {
    Ok(())
}

/// 收集“非隧道网卡上的 DNS 服务器”并过滤出需要阻断的地址。
fn collect_non_tunnel_dns_servers(
    adapter: AdapterInfo,
    full_v4: bool,
    full_v6: bool,
    tunnel_dns_servers: &[IpAddr],
) -> Result<Vec<IpAddr>, NetworkError> {
    let mut size = 0u32;
    let family = AF_UNSPEC.0 as u32;
    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST;

    let mut result = unsafe { GetAdaptersAddresses(family, flags, None, None, &mut size) };
    if result != windows::Win32::Foundation::ERROR_BUFFER_OVERFLOW.0 && result != NO_ERROR.0 {
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

    let tunnel_dns: BTreeSet<IpAddr> = tunnel_dns_servers.iter().copied().collect();
    let mut blocked_dns = BTreeSet::new();
    let mut adapter_ptr = ptr;
    unsafe {
        while !adapter_ptr.is_null() {
            let current = &*adapter_ptr;
            let is_tunnel_adapter = current.Luid.Value == adapter.luid.Value;
            if !is_tunnel_adapter {
                let mut dns_ptr: *mut IP_ADAPTER_DNS_SERVER_ADDRESS_XP =
                    current.FirstDnsServerAddress;
                while !dns_ptr.is_null() {
                    let entry = &*dns_ptr;
                    if let Some(ip) = ip_from_socket_address(&entry.Address) {
                        if should_block_dns_server(ip, full_v4, full_v6)
                            && !tunnel_dns.contains(&ip)
                        {
                            blocked_dns.insert(ip);
                        }
                    }
                    dns_ptr = entry.Next;
                }
            }
            adapter_ptr = current.Next;
        }
    }

    Ok(blocked_dns.into_iter().collect())
}

/// 判断某个 DNS 服务器地址是否应加入阻断集合。
fn should_block_dns_server(ip: IpAddr, full_v4: bool, full_v6: bool) -> bool {
    match ip {
        IpAddr::V4(addr) => {
            full_v4
                && !addr.is_loopback()
                && !addr.is_unspecified()
                && !addr.is_multicast()
                && !addr.is_broadcast()
        }
        IpAddr::V6(addr) => {
            full_v6 && !addr.is_loopback() && !addr.is_unspecified() && !addr.is_multicast()
        }
    }
}

fn ensure_dns_guard_sublayer(handle: HANDLE) -> Result<(), NetworkError> {
    let mut name = encode_wide("r-wg DNS Guard");
    let mut description = encode_wide("Blocks outbound DNS to non-tunnel resolvers.");
    let sublayer = FWPM_SUBLAYER0 {
        subLayerKey: DNS_GUARD_SUBLAYER_KEY,
        displayData: FWPM_DISPLAY_DATA0 {
            name: PWSTR(name.as_mut_ptr()),
            description: PWSTR(description.as_mut_ptr()),
        },
        weight: DNS_GUARD_SUBLAYER_WEIGHT,
        ..Default::default()
    };

    let status = unsafe { FwpmSubLayerAdd0(handle, &sublayer, None) };
    if status == NO_ERROR.0 || status == FWP_E_ALREADY_EXISTS.0 as u32 {
        Ok(())
    } else {
        Err(wfp_error("failed to add WFP DNS guard sublayer", status))
    }
}

fn add_dns_block_filter(
    handle: HANDLE,
    remote_ip: IpAddr,
    protocol: u8,
) -> Result<u64, NetworkError> {
    let layer = match remote_ip {
        IpAddr::V4(_) => FWPM_LAYER_ALE_AUTH_CONNECT_V4,
        IpAddr::V6(_) => FWPM_LAYER_ALE_AUTH_CONNECT_V6,
    };

    let mut filter_name = encode_wide(&format!(
        "r-wg DNS Guard {} {}",
        protocol_name(protocol),
        remote_ip
    ));
    let mut filter_description = encode_wide("Blocks outbound DNS to a non-tunnel resolver.");

    let protocol_condition = FWPM_FILTER_CONDITION0 {
        fieldKey: FWPM_CONDITION_IP_PROTOCOL,
        matchType: FWP_MATCH_EQUAL,
        conditionValue: FWP_CONDITION_VALUE0 {
            r#type: FWP_UINT8,
            Anonymous: FWP_CONDITION_VALUE0_0 { uint8: protocol },
        },
    };
    let port_condition = FWPM_FILTER_CONDITION0 {
        fieldKey: FWPM_CONDITION_IP_REMOTE_PORT,
        matchType: FWP_MATCH_EQUAL,
        conditionValue: FWP_CONDITION_VALUE0 {
            r#type: FWP_UINT16,
            Anonymous: FWP_CONDITION_VALUE0_0 {
                uint16: DNS_GUARD_PORT,
            },
        },
    };

    let mut filter_id = 0u64;
    match remote_ip {
        IpAddr::V4(addr) => {
            let mut address = FWP_V4_ADDR_AND_MASK {
                addr: ipv4_to_wfp_u32(addr),
                mask: u32::MAX,
            };
            let address_condition = FWPM_FILTER_CONDITION0 {
                fieldKey: FWPM_CONDITION_IP_REMOTE_ADDRESS,
                matchType: FWP_MATCH_EQUAL,
                conditionValue: FWP_CONDITION_VALUE0 {
                    r#type: FWP_V4_ADDR_MASK,
                    Anonymous: FWP_CONDITION_VALUE0_0 {
                        v4AddrMask: &mut address,
                    },
                },
            };
            let mut conditions = [protocol_condition, port_condition, address_condition];
            let filter = FWPM_FILTER0 {
                displayData: FWPM_DISPLAY_DATA0 {
                    name: PWSTR(filter_name.as_mut_ptr()),
                    description: PWSTR(filter_description.as_mut_ptr()),
                },
                flags: FWPM_FILTER_FLAG_CLEAR_ACTION_RIGHT,
                layerKey: layer,
                subLayerKey: DNS_GUARD_SUBLAYER_KEY,
                weight: FWP_VALUE0 {
                    r#type: FWP_EMPTY,
                    ..Default::default()
                },
                numFilterConditions: conditions.len() as u32,
                filterCondition: conditions.as_mut_ptr(),
                action: FWPM_ACTION0 {
                    r#type: FWP_ACTION_BLOCK,
                    ..Default::default()
                },
                ..Default::default()
            };
            let status = unsafe { FwpmFilterAdd0(handle, &filter, None, Some(&mut filter_id)) };
            if status != NO_ERROR.0 {
                return Err(wfp_error("failed to add WFP IPv4 DNS guard filter", status));
            }
        }
        IpAddr::V6(addr) => {
            let mut address = FWP_V6_ADDR_AND_MASK {
                addr: addr.octets(),
                prefixLength: 128,
            };
            let address_condition = FWPM_FILTER_CONDITION0 {
                fieldKey: FWPM_CONDITION_IP_REMOTE_ADDRESS,
                matchType: FWP_MATCH_EQUAL,
                conditionValue: FWP_CONDITION_VALUE0 {
                    r#type: FWP_V6_ADDR_MASK,
                    Anonymous: FWP_CONDITION_VALUE0_0 {
                        v6AddrMask: &mut address,
                    },
                },
            };
            let mut conditions = [protocol_condition, port_condition, address_condition];
            let filter = FWPM_FILTER0 {
                displayData: FWPM_DISPLAY_DATA0 {
                    name: PWSTR(filter_name.as_mut_ptr()),
                    description: PWSTR(filter_description.as_mut_ptr()),
                },
                flags: FWPM_FILTER_FLAG_CLEAR_ACTION_RIGHT,
                layerKey: layer,
                subLayerKey: DNS_GUARD_SUBLAYER_KEY,
                weight: FWP_VALUE0 {
                    r#type: FWP_EMPTY,
                    ..Default::default()
                },
                numFilterConditions: conditions.len() as u32,
                filterCondition: conditions.as_mut_ptr(),
                action: FWPM_ACTION0 {
                    r#type: FWP_ACTION_BLOCK,
                    ..Default::default()
                },
                ..Default::default()
            };
            let status = unsafe { FwpmFilterAdd0(handle, &filter, None, Some(&mut filter_id)) };
            if status != NO_ERROR.0 {
                return Err(wfp_error("failed to add WFP IPv6 DNS guard filter", status));
            }
        }
    }

    Ok(filter_id)
}

fn delete_filters(handle: HANDLE, filter_ids: &[u64]) -> Result<(), NetworkError> {
    let mut first_error = None;
    for &filter_id in filter_ids.iter().rev() {
        let status = unsafe { FwpmFilterDeleteById0(handle, filter_id) };
        if status != NO_ERROR.0
            && status != FWP_E_FILTER_NOT_FOUND.0 as u32
            && first_error.is_none()
        {
            first_error = Some(wfp_error("failed to delete WFP DNS guard filter", status));
        }
    }
    if let Some(err) = first_error {
        Err(err)
    } else {
        Ok(())
    }
}

fn ipv4_to_wfp_u32(addr: Ipv4Addr) -> u32 {
    u32::from_be_bytes(addr.octets())
}

fn protocol_name(protocol: u8) -> &'static str {
    match protocol {
        value if value == IPPROTO_UDP.0 as u8 => "UDP",
        value if value == IPPROTO_TCP.0 as u8 => "TCP",
        _ => "IP",
    }
}

fn encode_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn wfp_error(context: &'static str, status: u32) -> NetworkError {
    let detail = windows::core::Error::from_hresult(HRESULT(status as i32)).to_string();
    NetworkError::UnsafeRouting(format!("{context}: {detail} (code={status:#x})"))
}

struct WfpSession {
    handle: HANDLE,
}

impl WfpSession {
    fn open_dynamic() -> Result<Self, NetworkError> {
        Self::open(true)
    }

    fn open_static() -> Result<Self, NetworkError> {
        Self::open(false)
    }

    fn open(dynamic: bool) -> Result<Self, NetworkError> {
        let mut name = encode_wide("r-wg DNS Guard Session");
        let mut description = encode_wide("Dynamic WFP session for r-wg DNS guard filters.");
        let session = FWPM_SESSION0 {
            displayData: FWPM_DISPLAY_DATA0 {
                name: PWSTR(name.as_mut_ptr()),
                description: PWSTR(description.as_mut_ptr()),
            },
            flags: if dynamic {
                FWPM_SESSION_FLAG_DYNAMIC
            } else {
                0
            },
            ..Default::default()
        };

        let mut handle = HANDLE::default();
        let status = unsafe {
            FwpmEngineOpen0(
                PCWSTR::null(),
                RPC_C_AUTHN_WINNT,
                None,
                Some(&session),
                &mut handle,
            )
        };
        if status != NO_ERROR.0 {
            return Err(wfp_error("failed to open WFP engine", status));
        }

        Ok(Self { handle })
    }

    fn handle(&self) -> HANDLE {
        self.handle
    }

    fn close(mut self) -> Result<(), NetworkError> {
        let result = close_engine_handle(self.handle);
        self.handle = HANDLE::default();
        result
    }
}

impl Drop for WfpSession {
    fn drop(&mut self) {
        let _ = close_engine_handle(self.handle);
        self.handle = HANDLE::default();
    }
}

fn close_engine_handle(handle: HANDLE) -> Result<(), NetworkError> {
    if handle.is_invalid() {
        return Ok(());
    }
    let status = unsafe { FwpmEngineClose0(handle) };
    if status == NO_ERROR.0 {
        Ok(())
    } else {
        Err(wfp_error("failed to close WFP engine", status))
    }
}

struct WfpTransaction {
    handle: HANDLE,
    active: bool,
}

impl WfpTransaction {
    fn begin(handle: HANDLE) -> Result<Self, NetworkError> {
        let status = unsafe { FwpmTransactionBegin0(handle, 0) };
        if status != NO_ERROR.0 {
            return Err(wfp_error("failed to begin WFP transaction", status));
        }
        Ok(Self {
            handle,
            active: true,
        })
    }

    fn commit(mut self) -> Result<(), NetworkError> {
        let status = unsafe { FwpmTransactionCommit0(self.handle) };
        self.active = false;
        if status == NO_ERROR.0 {
            Ok(())
        } else {
            Err(wfp_error("failed to commit WFP transaction", status))
        }
    }
}

impl Drop for WfpTransaction {
    fn drop(&mut self) {
        if self.active {
            let _ = unsafe { FwpmTransactionAbort0(self.handle) };
            self.active = false;
        }
    }
}
