//! DNS 配置与回滚逻辑。
//!
//! 设计目标：
//! - 优先使用系统原生后端（systemd-resolved），再尝试兼容后端（resolvconf）。
//! - 只处理“对接口生效”的 DNS 设置，不直接修改全局 resolv.conf。
//! - 一旦设置失败，向上抛错，由上层决定是否允许“带警告启动”。

use std::net::IpAddr;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

use super::logging::log_net;
use super::NetworkError;

#[derive(Debug)]
pub(super) struct DnsState {
    backend: DnsBackend,
}

#[derive(Debug)]
enum DnsBackend {
    Resolved,
    Resolvconf,
    NetworkManager { connections: Vec<NmConnectionState> },
    ResolvConf { path: PathBuf, original: String },
}

#[derive(Debug, Clone)]
struct NmConnectionState {
    name: String,
    device: String,
    ipv4_dns: String,
    ipv4_ignore_auto: String,
    ipv4_search: String,
    ipv4_priority: String,
    ipv6_dns: String,
    ipv6_ignore_auto: String,
    ipv6_search: String,
    ipv6_priority: String,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum DnsBackendKind {
    Resolved,
    Resolvconf,
    NetworkManager,
    ResolvConf,
}

impl DnsBackendKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Resolved => "resolved",
            Self::Resolvconf => "resolvconf",
            Self::NetworkManager => "network-manager",
            Self::ResolvConf => "resolv.conf",
        }
    }
}

#[derive(Debug)]
struct ResolvConfInfo {
    path: PathBuf,
    is_symlink: bool,
    target: Option<PathBuf>,
    contents: Option<String>,
}

/// 应用 DNS 配置。
///
/// 优先使用 `resolvectl`，否则使用 `resolvconf`。
pub(super) async fn apply_dns(
    tun_name: &str,
    servers: &[IpAddr],
    search: &[String],
) -> Result<DnsState, NetworkError> {
    let info = read_resolv_conf_info();
    log_resolv_conf_info(&info);

    let backends = dns_backend_order(&info);
    log_net(format!(
        "dns backend order: {}",
        backends
            .iter()
            .map(|backend| backend.as_str())
            .collect::<Vec<_>>()
            .join(" -> ")
    ));

    let mut last_error: Option<NetworkError> = None;

    for backend in backends {
        match backend {
            DnsBackendKind::Resolved => {
                let Some(resolvectl) = resolve_command("resolvectl") else {
                    log_net("dns resolvectl not found".to_string());
                    continue;
                };
                log_net(format!("dns backend: resolvectl ({})", resolvectl.display()));
                match apply_resolved(&resolvectl, tun_name, servers, search).await {
                    Ok(()) => {
                        return Ok(DnsState {
                            backend: DnsBackend::Resolved,
                        });
                    }
                    Err(err) => {
                        log_net(format!("dns resolvectl failed: {err}"));
                        last_error = Some(err);
                    }
                }
            }
            DnsBackendKind::Resolvconf => {
                let Some(resolvconf) = resolve_command("resolvconf") else {
                    log_net("dns resolvconf not found".to_string());
                    continue;
                };
                log_net(format!("dns backend: resolvconf ({})", resolvconf.display()));
                match apply_resolvconf(&resolvconf, tun_name, servers, search).await {
                    Ok(()) => {
                        return Ok(DnsState {
                            backend: DnsBackend::Resolvconf,
                        });
                    }
                    Err(err) => {
                        log_net(format!("dns resolvconf failed: {err}"));
                        last_error = Some(err);
                    }
                }
            }
            DnsBackendKind::NetworkManager => {
                let Some(nmcli) = resolve_command("nmcli") else {
                    log_net("dns nmcli not found".to_string());
                    continue;
                };
                log_net(format!("dns backend: nmcli ({})", nmcli.display()));
                match apply_network_manager(&nmcli, servers, search).await {
                    Ok(state) => return Ok(state),
                    Err(err) => {
                        log_net(format!("dns nmcli failed: {err}"));
                        last_error = Some(err);
                    }
                }
            }
            DnsBackendKind::ResolvConf => match apply_resolv_conf_file(&info, servers, search) {
                Ok(state) => return Ok(state),
                Err(err) => {
                    log_net(format!("dns resolv.conf failed: {err}"));
                    last_error = Some(err);
                }
            },
        }
    }

    // 所有后端都失败时，返回最后一个错误或“不支持”错误。
    Err(last_error.unwrap_or(NetworkError::DnsNotSupported))
}

/// 清理 DNS 配置。
pub(super) async fn cleanup_dns(tun_name: &str, state: DnsState) -> Result<(), NetworkError> {
    match state.backend {
        DnsBackend::Resolved => {
            if let Some(resolvectl) = resolve_command("resolvectl") {
                // resolvectl revert 会撤销该接口的 DNS 配置。
                log_net(format!("dns revert: resolvectl ({})", resolvectl.display()));
                run_cmd(&resolvectl, &vec!["revert".to_string(), tun_name.to_string()]).await?
            }
        }
        DnsBackend::Resolvconf => {
            if let Some(resolvconf) = resolve_command("resolvconf") {
                // resolvconf -d 删除该接口的 DNS 记录。
                log_net(format!("dns revert: resolvconf ({})", resolvconf.display()));
                run_cmd(&resolvconf, &vec!["-d".to_string(), tun_name.to_string()]).await?
            }
        }
        DnsBackend::NetworkManager { connections } => {
            if let Some(nmcli) = resolve_command("nmcli") {
                for conn in connections {
                    log_net(format!(
                        "dns revert: nmcli connection={}",
                        conn.name
                    ));
                    if let Err(err) = restore_nm_connection(&nmcli, &conn).await {
                        log_net(format!("dns nmcli revert failed: {err}"));
                    }
                }
            }
        }
        DnsBackend::ResolvConf { path, original } => {
            log_net(format!("dns revert: resolv.conf ({})", path.display()));
            write_resolv_conf(&path, &original)?;
        }
    }
    Ok(())
}

/// 解析命令路径，优先使用 PATH，其次尝试常见系统目录。
fn resolve_command(program: &str) -> Option<PathBuf> {
    if program.contains('/') {
        let path = PathBuf::from(program);
        return path.is_file().then_some(path);
    }

    // PATH 中可执行优先，避免硬编码路径。
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join(program);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    // 一些系统工具可能不在 PATH，补一层常见目录兜底。
    for dir in ["/usr/sbin", "/sbin", "/usr/bin", "/bin"] {
        let candidate = Path::new(dir).join(program);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    None
}

/// 执行命令并检查返回码。
async fn run_cmd(program: &Path, args: &[String]) -> Result<(), NetworkError> {
    // 将实际执行的命令记录到日志，便于排查系统调用失败。
    log_command(program, args);
    let output = Command::new(program)
        .args(args)
        .output()
        .await?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(NetworkError::CommandFailed {
        command: format_command(program, args),
        status: output.status.code(),
        stderr,
    })
}

/// 执行命令并通过 stdin 写入内容。
async fn run_cmd_with_input(
    program: &Path,
    args: &[String],
    input: &str,
) -> Result<(), NetworkError> {
    // 需要向 stdin 写入时，必须用 spawn + piped 模式。
    log_command(program, args);
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        stdin.write_all(input.as_bytes()).await?;
    }

    let output = child.wait_with_output().await?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(NetworkError::CommandFailed {
        command: format_command(program, args),
        status: output.status.code(),
        stderr,
    })
}

fn log_command(program: &Path, args: &[String]) {
    log_net(format!("exec: {}", format_command(program, args)));
}

/// 组装可读的命令文本用于错误提示。
fn format_command(program: &Path, args: &[String]) -> String {
    let mut command = program.display().to_string();
    for arg in args {
        command.push(' ');
        command.push_str(arg);
    }
    command
}

async fn apply_resolved(
    resolvectl: &Path,
    tun_name: &str,
    servers: &[IpAddr],
    search: &[String],
) -> Result<(), NetworkError> {
    if !servers.is_empty() {
        let mut args = vec!["dns".to_string(), tun_name.to_string()];
        for server in servers {
            args.push(server.to_string());
        }
        run_cmd(resolvectl, &args).await?;
    }

    if !servers.is_empty() || !search.is_empty() {
        let mut domain_args = vec!["domain".to_string(), tun_name.to_string()];
        domain_args.extend(search.iter().cloned());
        if !servers.is_empty() && !search.iter().any(|domain| domain == "~.") {
            domain_args.push("~.".to_string());
        }
        run_cmd(resolvectl, &domain_args).await?;
    }

    Ok(())
}

async fn apply_resolvconf(
    resolvconf: &Path,
    tun_name: &str,
    servers: &[IpAddr],
    search: &[String],
) -> Result<(), NetworkError> {
    let mut content = String::new();
    for server in servers {
        content.push_str("nameserver ");
        content.push_str(&server.to_string());
        content.push('\n');
    }
    if !search.is_empty() {
        content.push_str("search ");
        content.push_str(&search.join(" "));
        content.push('\n');
    }

    let args = vec!["-a".to_string(), tun_name.to_string()];
    run_cmd_with_input(resolvconf, &args, &content).await
}

fn read_resolv_conf_info() -> ResolvConfInfo {
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

fn log_resolv_conf_info(info: &ResolvConfInfo) {
    if info.is_symlink {
        if let Some(target) = &info.target {
            log_net(format!(
                "dns resolv.conf: symlink -> {}",
                target.display()
            ));
        } else {
            log_net("dns resolv.conf: symlink (target unknown)".to_string());
        }
    } else {
        log_net("dns resolv.conf: regular file".to_string());
    }
}

fn dns_backend_order(info: &ResolvConfInfo) -> Vec<DnsBackendKind> {
    let mut order = vec![
        DnsBackendKind::Resolved,
        DnsBackendKind::Resolvconf,
        DnsBackendKind::NetworkManager,
    ];
    if let Some(preferred) = detect_preferred_backend(info) {
        if preferred != DnsBackendKind::ResolvConf {
            order.retain(|backend| *backend != preferred);
            order.insert(0, preferred);
        }
    }

    order.push(DnsBackendKind::ResolvConf);
    order
}

fn detect_preferred_backend(info: &ResolvConfInfo) -> Option<DnsBackendKind> {
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

async fn apply_network_manager(
    nmcli: &Path,
    servers: &[IpAddr],
    search: &[String],
) -> Result<DnsState, NetworkError> {
    let connections = nmcli_active_connections(nmcli).await?;
    if connections.is_empty() {
        return Err(NetworkError::DnsNotSupported);
    }

    let (v4_dns, v6_dns) = split_dns_servers(servers);
    let v4_dns = v4_dns.join(",");
    let v6_dns = v6_dns.join(",");
    let search_value = search.join(",");

    let mut touched = Vec::new();
    for conn in connections {
        let state = read_nm_connection_state(nmcli, &conn.name, &conn.device).await?;
        if let Err(err) =
            apply_nm_connection(
                nmcli,
                &conn.name,
                &conn.device,
                &v4_dns,
                &v6_dns,
                &search_value,
            )
            .await
        {
            for restored in touched.iter().rev() {
                let _ = restore_nm_connection(nmcli, restored).await;
            }
            return Err(err);
        }
        touched.push(state);
    }

    if let Err(err) = wait_for_resolv_conf_servers(Path::new("/etc/resolv.conf"), servers).await {
        for restored in touched.iter().rev() {
            let _ = restore_nm_connection(nmcli, restored).await;
        }
        return Err(err);
    }

    Ok(DnsState {
        backend: DnsBackend::NetworkManager { connections: touched },
    })
}

async fn nmcli_active_connections(nmcli: &Path) -> Result<Vec<NmConnection>, NetworkError> {
    let args = vec![
        "-t".to_string(),
        "-f".to_string(),
        "NAME,DEVICE,TYPE,STATE".to_string(),
        "connection".to_string(),
        "show".to_string(),
        "--active".to_string(),
    ];
    let output = run_cmd_capture(nmcli, &args).await?;
    let mut connections = Vec::new();
    for line in output.lines() {
        let mut parts = line.splitn(4, ':');
        let name = parts.next().unwrap_or("").trim();
        let device = parts.next().unwrap_or("").trim();
        let kind = parts.next().unwrap_or("").trim();
        let state = parts.next().unwrap_or("").trim();
        if name.is_empty() || device.is_empty() || device == "--" {
            continue;
        }
        if state != "activated" {
            continue;
        }
        let kind_lower = kind.to_ascii_lowercase();
        if kind_lower.contains("loopback")
            || kind_lower.contains("tun")
            || kind_lower.contains("wireguard")
            || kind_lower.contains("vpn")
        {
            continue;
        }
        connections.push(NmConnection {
            name: name.to_string(),
            device: device.to_string(),
        });
    }
    Ok(connections)
}

struct NmConnection {
    name: String,
    device: String,
}

async fn read_nm_connection_state(
    nmcli: &Path,
    name: &str,
    device: &str,
) -> Result<NmConnectionState, NetworkError> {
    let ipv4_dns = nmcli_get(nmcli, name, "ipv4.dns").await?;
    let ipv4_ignore_auto = normalize_nmcli_bool(nmcli_get(nmcli, name, "ipv4.ignore-auto-dns").await?);
    let ipv4_search = nmcli_get(nmcli, name, "ipv4.dns-search").await?;
    let ipv4_priority = nmcli_get(nmcli, name, "ipv4.dns-priority").await?;
    let ipv6_dns = nmcli_get(nmcli, name, "ipv6.dns").await?;
    let ipv6_ignore_auto = normalize_nmcli_bool(nmcli_get(nmcli, name, "ipv6.ignore-auto-dns").await?);
    let ipv6_search = nmcli_get(nmcli, name, "ipv6.dns-search").await?;
    let ipv6_priority = nmcli_get(nmcli, name, "ipv6.dns-priority").await?;

    Ok(NmConnectionState {
        name: name.to_string(),
        device: device.to_string(),
        ipv4_dns,
        ipv4_ignore_auto,
        ipv4_search,
        ipv4_priority,
        ipv6_dns,
        ipv6_ignore_auto,
        ipv6_search,
        ipv6_priority,
    })
}

async fn nmcli_get(
    nmcli: &Path,
    name: &str,
    field: &str,
) -> Result<String, NetworkError> {
    let args = vec![
        "-g".to_string(),
        field.to_string(),
        "connection".to_string(),
        "show".to_string(),
        name.to_string(),
    ];
    let output = run_cmd_capture(nmcli, &args).await?;
    Ok(output.trim().to_string())
}

fn normalize_nmcli_bool(value: String) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "no".to_string()
    } else {
        trimmed.to_string()
    }
}

async fn apply_nm_connection(
    nmcli: &Path,
    name: &str,
    device: &str,
    v4_dns: &str,
    v6_dns: &str,
    search: &str,
) -> Result<(), NetworkError> {
    let args = vec![
        "connection".to_string(),
        "modify".to_string(),
        name.to_string(),
        "ipv4.dns".to_string(),
        v4_dns.to_string(),
        "ipv4.ignore-auto-dns".to_string(),
        "yes".to_string(),
        "ipv4.dns-search".to_string(),
        search.to_string(),
        "ipv4.dns-priority".to_string(),
        "-42".to_string(),
        "ipv6.dns".to_string(),
        v6_dns.to_string(),
        "ipv6.ignore-auto-dns".to_string(),
        "yes".to_string(),
        "ipv6.dns-search".to_string(),
        search.to_string(),
        "ipv6.dns-priority".to_string(),
        "-42".to_string(),
    ];
    run_cmd(nmcli, &args).await?;
    nmcli_reapply(nmcli, device).await
}

async fn restore_nm_connection(
    nmcli: &Path,
    state: &NmConnectionState,
) -> Result<(), NetworkError> {
    let args = vec![
        "connection".to_string(),
        "modify".to_string(),
        state.name.to_string(),
        "ipv4.dns".to_string(),
        state.ipv4_dns.clone(),
        "ipv4.ignore-auto-dns".to_string(),
        state.ipv4_ignore_auto.clone(),
        "ipv4.dns-search".to_string(),
        state.ipv4_search.clone(),
        "ipv4.dns-priority".to_string(),
        state.ipv4_priority.clone(),
        "ipv6.dns".to_string(),
        state.ipv6_dns.clone(),
        "ipv6.ignore-auto-dns".to_string(),
        state.ipv6_ignore_auto.clone(),
        "ipv6.dns-search".to_string(),
        state.ipv6_search.clone(),
        "ipv6.dns-priority".to_string(),
        state.ipv6_priority.clone(),
    ];
    run_cmd(nmcli, &args).await?;
    nmcli_reapply(nmcli, &state.device).await
}

async fn nmcli_reapply(nmcli: &Path, device: &str) -> Result<(), NetworkError> {
    let args = vec![
        "device".to_string(),
        "reapply".to_string(),
        device.to_string(),
    ];
    run_cmd(nmcli, &args).await
}

fn apply_resolv_conf_file(
    info: &ResolvConfInfo,
    servers: &[IpAddr],
    search: &[String],
) -> Result<DnsState, NetworkError> {
    if info.is_symlink {
        log_net("dns resolv.conf skipped: symlink".to_string());
        return Err(NetworkError::DnsNotSupported);
    }

    let original = fs::read_to_string(&info.path)?;
    let content = build_resolv_conf_contents(servers, search);
    write_resolv_conf(&info.path, &content)?;
    if let Err(err) = verify_resolv_conf_servers(&info.path, servers) {
        let _ = write_resolv_conf(&info.path, &original);
        return Err(err);
    }

    Ok(DnsState {
        backend: DnsBackend::ResolvConf {
            path: info.path.clone(),
            original,
        },
    })
}

fn build_resolv_conf_contents(servers: &[IpAddr], search: &[String]) -> String {
    let mut content = String::from("# Generated by r-wg\n");
    if !search.is_empty() {
        content.push_str("search ");
        content.push_str(&search.join(" "));
        content.push('\n');
    }
    for server in servers {
        content.push_str("nameserver ");
        content.push_str(&server.to_string());
        content.push('\n');
    }
    content
}

fn write_resolv_conf(path: &Path, contents: &str) -> Result<(), NetworkError> {
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)?;
    file.write_all(contents.as_bytes())?;
    Ok(())
}

fn split_dns_servers(servers: &[IpAddr]) -> (Vec<String>, Vec<String>) {
    let mut v4 = Vec::new();
    let mut v6 = Vec::new();
    for server in servers {
        match server {
            IpAddr::V4(addr) => v4.push(addr.to_string()),
            IpAddr::V6(addr) => v6.push(addr.to_string()),
        }
    }
    (v4, v6)
}

async fn run_cmd_capture(program: &Path, args: &[String]) -> Result<String, NetworkError> {
    log_command(program, args);
    let output = Command::new(program).args(args).output().await?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).to_string());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(NetworkError::CommandFailed {
        command: format_command(program, args),
        status: output.status.code(),
        stderr,
    })
}

fn verify_resolv_conf_servers(path: &Path, servers: &[IpAddr]) -> Result<(), NetworkError> {
    let expected_v4: HashSet<IpAddr> = servers
        .iter()
        .filter(|addr| matches!(addr, IpAddr::V4(_)))
        .copied()
        .collect();
    let expected_v6: HashSet<IpAddr> = servers
        .iter()
        .filter(|addr| matches!(addr, IpAddr::V6(_)))
        .copied()
        .collect();

    let (found_v4, found_v6) = read_resolv_conf_servers(path)?;

    if expected_v4.is_empty() {
        if !found_v4.is_empty() {
            return Err(NetworkError::DnsVerifyFailed(format!(
                "unexpected IPv4 DNS: {}",
                format_ip_list(&found_v4)
            )));
        }
    } else {
        if found_v4.is_empty() {
            return Err(NetworkError::DnsVerifyFailed(
                "missing IPv4 DNS".to_string(),
            ));
        }
        let extras: Vec<IpAddr> = found_v4
            .iter()
            .filter(|addr| !expected_v4.contains(addr))
            .copied()
            .collect();
        if !extras.is_empty() {
            return Err(NetworkError::DnsVerifyFailed(format!(
                "unexpected IPv4 DNS: {}",
                format_ip_list(&extras)
            )));
        }
    }

    if expected_v6.is_empty() {
        if !found_v6.is_empty() {
            return Err(NetworkError::DnsVerifyFailed(format!(
                "unexpected IPv6 DNS: {}",
                format_ip_list(&found_v6)
            )));
        }
    } else {
        if found_v6.is_empty() {
            return Err(NetworkError::DnsVerifyFailed(
                "missing IPv6 DNS".to_string(),
            ));
        }
        let extras: Vec<IpAddr> = found_v6
            .iter()
            .filter(|addr| !expected_v6.contains(addr))
            .copied()
            .collect();
        if !extras.is_empty() {
            return Err(NetworkError::DnsVerifyFailed(format!(
                "unexpected IPv6 DNS: {}",
                format_ip_list(&extras)
            )));
        }
    }

    Ok(())
}

async fn wait_for_resolv_conf_servers(
    path: &Path,
    servers: &[IpAddr],
) -> Result<(), NetworkError> {
    let mut last_error: Option<NetworkError> = None;
    for _ in 0..5 {
        match verify_resolv_conf_servers(path, servers) {
            Ok(()) => return Ok(()),
            Err(err @ NetworkError::DnsVerifyFailed(_)) => {
                last_error = Some(err);
            }
            Err(err) => return Err(err),
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    Err(last_error.unwrap_or(NetworkError::DnsVerifyFailed(
        "dns verify timeout".to_string(),
    )))
}

fn read_resolv_conf_servers(path: &Path) -> Result<(Vec<IpAddr>, Vec<IpAddr>), NetworkError> {
    let contents = fs::read_to_string(path)?;
    let mut v4 = Vec::new();
    let mut v6 = Vec::new();

    for line in contents.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if !line.starts_with("nameserver") {
            continue;
        }
        let mut parts = line.split_whitespace();
        let _ = parts.next();
        let Some(value) = parts.next() else {
            continue;
        };
        let ip_str = value.split('%').next().unwrap_or(value);
        let Ok(addr) = ip_str.parse::<IpAddr>() else {
            return Err(NetworkError::DnsVerifyFailed(format!(
                "unparseable DNS entry: {value}"
            )));
        };
        match addr {
            IpAddr::V4(_) => v4.push(addr),
            IpAddr::V6(_) => v6.push(addr),
        }
    }

    Ok((v4, v6))
}

fn format_ip_list(addrs: &[IpAddr]) -> String {
    addrs
        .iter()
        .map(|addr| addr.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}
