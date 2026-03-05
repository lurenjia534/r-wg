use std::collections::BTreeSet;
use std::net::IpAddr;
use std::process::Command;

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

#[derive(Clone)]
pub(super) struct DnsGuardState {
    rule_names: Vec<String>,
}

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

    for name in &rule_names {
        delete_rule(name)?;
    }

    add_dns_block_rule(&rule_names[0], "UDP", &remote_ip_arg)?;
    if let Err(err) = add_dns_block_rule(&rule_names[1], "TCP", &remote_ip_arg) {
        let _ = delete_rule(&rule_names[0]);
        return Err(err);
    }

    log_net::dns_guard_apply(blocked_dns_servers.len());

    Ok(Some(DnsGuardState { rule_names }))
}

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

fn delete_rule(name: &str) -> Result<(), NetworkError> {
    let args = [
        "advfirewall",
        "firewall",
        "delete",
        "rule",
        &format!("name={name}"),
    ];

    Command::new("netsh")
        .args(args)
        .output()
        .map(|_| ())
        .map_err(NetworkError::Io)
}

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
