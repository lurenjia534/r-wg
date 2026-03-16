//! Windows DNS 防泄露防火墙规则。
//!
//! 设计思路：
//! - 仅在全隧道场景启用；
//! - 读取“非隧道网卡”的 DNS 服务器地址；
//! - 下发两条出站阻断规则（UDP/TCP 53），仅阻断到这些非隧道 DNS 的请求；
//! - 不阻断隧道 DNS 服务器，避免误伤正常解析。

use std::collections::BTreeSet;
use std::net::IpAddr;
use std::os::windows::process::CommandExt;
use std::process::Command;

use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, NO_ERROR, WIN32_ERROR};
use windows::Win32::NetworkManagement::IpHelper::{
    GetAdaptersAddresses, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_MULTICAST, IP_ADAPTER_ADDRESSES_LH,
    IP_ADAPTER_DNS_SERVER_ADDRESS_XP,
};
use windows::Win32::Networking::WinSock::AF_UNSPEC;

use super::adapter::AdapterInfo;
use super::sockaddr::ip_from_socket_address;
use super::NetworkError;
use crate::log::events::net as log_net;

/// Windows CreateProcess 标志：让控制台子进程在后台运行而不弹出黑窗。
/// DNS Guard 通过 `netsh` 下发规则时会频繁调用外部命令，这里隐藏窗口避免干扰 UI。
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// DNS Guard 的可回滚状态。
#[derive(Clone)]
pub(super) struct DnsGuardState {
    /// 创建过的规则名，断开隧道时按此删除。
    rule_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct DnsGuardStateSnapshot {
    rule_names: Vec<String>,
}

impl DnsGuardState {
    pub(super) fn snapshot(&self) -> DnsGuardStateSnapshot {
        DnsGuardStateSnapshot {
            rule_names: self.rule_names.clone(),
        }
    }
}

impl DnsGuardStateSnapshot {
    pub(super) fn to_state(&self) -> DnsGuardState {
        DnsGuardState {
            rule_names: self.rule_names.clone(),
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

    let remote_ip_arg = blocked_dns_servers
        .iter()
        .map(|ip| ip.to_string())
        .collect::<Vec<_>>()
        .join(",");

    let rule_base = format!("r-wg-dns-guard-if{}", adapter.if_index);
    let rule_names = vec![format!("{rule_base}-udp"), format!("{rule_base}-tcp")];

    // 先清旧规则，避免重连后同名冲突。
    for name in &rule_names {
        delete_rule(name)?;
    }

    add_dns_block_rule(&rule_names[0], "UDP", &remote_ip_arg)?;
    if let Err(err) = add_dns_block_rule(&rule_names[1], "TCP", &remote_ip_arg) {
        // 第二条失败时，回滚第一条，避免半成功状态。
        let _ = delete_rule(&rule_names[0]);
        return Err(err);
    }

    log_net::dns_guard_apply(blocked_dns_servers.len());

    Ok(Some(DnsGuardState { rule_names }))
}

/// 清理 DNS Guard 规则。
pub(super) fn cleanup_dns_guard(state: DnsGuardState) -> Result<(), NetworkError> {
    let mut first_error: Option<NetworkError> = None;
    for name in state.rule_names {
        if let Err(err) = delete_rule(&name) {
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

/// 通过 netsh 增加一条出站 DNS 阻断规则。
fn add_dns_block_rule(name: &str, protocol: &str, remote_ips: &str) -> Result<(), NetworkError> {
    let args = vec![
        "advfirewall".to_string(),
        "firewall".to_string(),
        "add".to_string(),
        "rule".to_string(),
        format!("name={name}"),
        "dir=out".to_string(),
        "action=block".to_string(),
        format!("protocol={protocol}"),
        "remoteport=53".to_string(),
        format!("remoteip={remote_ips}"),
        "profile=any".to_string(),
    ];

    let output = Command::new("netsh")
        // 开关隧道时会触发规则增删；隐藏 netsh 黑窗可避免前台闪烁。
        .creation_flags(CREATE_NO_WINDOW)
        .args(&args)
        .output()
        .map_err(NetworkError::Io)?;

    if output.status.success() {
        return Ok(());
    }

    let detail = netsh_detail(&output.stdout, &output.stderr);
    Err(NetworkError::UnsafeRouting(format!(
        "failed to add DNS guard rule {name}: {detail}"
    )))
}

/// 删除指定规则名（不存在也不报错）。
fn delete_rule(name: &str) -> Result<(), NetworkError> {
    let args = [
        "advfirewall",
        "firewall",
        "delete",
        "rule",
        &format!("name={name}"),
    ];

    Command::new("netsh")
        // 删除规则同样走 netsh，保持无窗口行为一致。
        .creation_flags(CREATE_NO_WINDOW)
        .args(args)
        .output()
        .map(|_| ())
        .map_err(NetworkError::Io)
}

/// 从 netsh 输出中提取可读错误信息。
fn netsh_detail(stdout: &[u8], stderr: &[u8]) -> String {
    let stderr_text = String::from_utf8_lossy(stderr).trim().to_string();
    if !stderr_text.is_empty() {
        return stderr_text;
    }
    let stdout_text = String::from_utf8_lossy(stdout).trim().to_string();
    if !stdout_text.is_empty() {
        return stdout_text;
    }
    "netsh returned a failure status".to_string()
}
