use std::fs;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::Duration;

use tokio::process::Command;

use super::command::resolve_command;
use super::types::{DnsBackendKind, ResolvConfInfo};
use crate::log::events::dns as log_dns;

pub(super) static DNS_BACKEND_ORDER_CACHE: OnceLock<Vec<DnsBackendKind>> = OnceLock::new();

// 读取 resolv.conf 的符号链接与内容，用于推断系统 DNS 管理器。
pub(super) fn read_resolv_conf_info() -> ResolvConfInfo {
    let path = PathBuf::from("/etc/resolv.conf");
    let metadata = fs::symlink_metadata(&path);
    let is_symlink = metadata
        .as_ref()
        .map(|meta| meta.file_type().is_symlink())
        .unwrap_or(false);
    let target = if is_symlink {
        fs::read_link(&path).ok()
    } else {
        None
    };
    let contents = fs::read_to_string(&path).ok();
    ResolvConfInfo {
        path,
        is_symlink,
        target,
        contents,
    }
}

pub(super) fn log_resolv_conf_info(info: &ResolvConfInfo) {
    if info.is_symlink {
        if let Some(target) = &info.target {
            log_dns::resolv_conf_symlink(Some(target.as_path()));
        } else {
            log_dns::resolv_conf_symlink(None);
        }
    } else {
        log_dns::resolv_conf_regular();
    }
}

pub(super) async fn dns_backend_order(info: &ResolvConfInfo) -> Vec<DnsBackendKind> {
    if let Some(cached) = DNS_BACKEND_ORDER_CACHE.get() {
        return cached.clone();
    }
    // 运行时探测可用后端，避免仅凭 resolv.conf 推断导致误选。
    let preferred = detect_preferred_backend(info);
    let (resolved_ok, nm_ok) = tokio::join!(probe_resolved_backend(), probe_network_manager());
    let resolvconf_ok = probe_resolvconf_backend();
    let order = dns_backend_order_from_probes(info, preferred, resolved_ok, resolvconf_ok, nm_ok);
    let _ = DNS_BACKEND_ORDER_CACHE.set(order.clone());
    order
}

pub(super) fn dns_backend_order_from_probes(
    info: &ResolvConfInfo,
    preferred: Option<DnsBackendKind>,
    resolved_ok: bool,
    resolvconf_ok: bool,
    nm_ok: bool,
) -> Vec<DnsBackendKind> {
    // 默认顺序：系统后端优先，resolv.conf 作为最终兜底。
    let mut order = Vec::new();
    if resolved_ok {
        order.push(DnsBackendKind::Resolved);
    }
    if resolvconf_ok {
        order.push(DnsBackendKind::Resolvconf);
    }
    if nm_ok {
        order.push(DnsBackendKind::NetworkManager);
    }
    if !info.is_symlink {
        order.push(DnsBackendKind::ResolvConf);
    }

    if let Some(preferred) = preferred {
        if order.contains(&preferred) {
            order.retain(|backend| *backend != preferred);
            order.insert(0, preferred);
        }
    }

    order
}

/// 探测 systemd-resolved 是否可用（resolvectl status 成功即视为可用）。
pub(super) async fn probe_resolved_backend() -> bool {
    let Some(resolvectl) = resolve_command("resolvectl") else {
        return false;
    };
    let args = vec!["status".to_string()];
    probe_command_status(&resolvectl, &args).await
}

/// 探测 resolvconf 是否存在（仅检查二进制）。
pub(super) fn probe_resolvconf_backend() -> bool {
    resolve_command("resolvconf").is_some()
}

/// 探测 NetworkManager 是否处于运行态。
pub(super) async fn probe_network_manager() -> bool {
    let Some(nmcli) = resolve_command("nmcli") else {
        return false;
    };
    let args = vec![
        "-t".to_string(),
        "-f".to_string(),
        "RUNNING".to_string(),
        "general".to_string(),
    ];
    let output = probe_command_output(&nmcli, &args).await;
    matches!(output.as_deref(), Some(value) if value.trim().eq_ignore_ascii_case("running"))
}

/// 探测命令是否能在超时内成功返回。
async fn probe_command_status(program: &std::path::Path, args: &[String]) -> bool {
    let mut cmd = Command::new(program);
    cmd.args(args).stdout(Stdio::null()).stderr(Stdio::null());
    match tokio::time::timeout(Duration::from_millis(800), cmd.status()).await {
        Ok(Ok(status)) => status.success(),
        _ => false,
    }
}

/// 探测命令输出（超时或失败返回 None）。
async fn probe_command_output(program: &std::path::Path, args: &[String]) -> Option<String> {
    let mut cmd = Command::new(program);
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::null());
    let output = match tokio::time::timeout(Duration::from_millis(800), cmd.output()).await {
        Ok(Ok(output)) => output,
        _ => return None,
    };
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

pub(super) fn detect_preferred_backend(info: &ResolvConfInfo) -> Option<DnsBackendKind> {
    // 通过 symlink 目标与内容标识判断当前实际生效的 DNS 管理器。
    let target = info
        .target
        .as_ref()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_default();
    let contents = info.contents.as_deref().unwrap_or("");

    if target.contains("systemd/resolve")
        || contents.contains("systemd-resolved")
        || contents.contains("Stub resolver")
    {
        return Some(DnsBackendKind::Resolved);
    }

    if target.contains("resolvconf") || contents.contains("resolvconf") {
        return Some(DnsBackendKind::Resolvconf);
    }

    if contents.contains("NetworkManager") {
        return Some(DnsBackendKind::NetworkManager);
    }

    if !info.is_symlink {
        return Some(DnsBackendKind::ResolvConf);
    }

    None
}
