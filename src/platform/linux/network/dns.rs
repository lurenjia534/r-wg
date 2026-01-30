//! DNS 配置与回滚逻辑。
//!
//! 设计目标：
//! - 优先使用系统原生后端（systemd-resolved / resolvconf / NetworkManager），最后兜底写入 `/etc/resolv.conf`。
//! - 能按“接口级 DNS”处理时不改全局文件；仅当 resolv.conf 是普通文件时才会备份并写入。
//! - 严格验证解析器配置，避免多余 DNS（例如 RDNSS 注入）导致泄漏。
//! - NM 路径在必要时触发 down/up 重连以清掉残留 DNS。
//! - 失败向上抛错，由上层决定是否允许继续启动。

use std::collections::HashSet;
use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::Duration;

use tokio::process::Command;

use super::NetworkError;
use crate::log::events::dns as log_dns;

#[derive(Debug)]
pub(super) struct DnsState {
    /// 记录本次使用的 DNS 后端与其回滚信息，供 stop/cleanup 时撤销。
    backend: DnsBackend,
}

#[derive(Debug)]
enum DnsBackend {
    /// systemd-resolved: 对接口设置 DNS，可通过 resolvectl revert 回滚。
    Resolved,
    /// resolvconf/openresolv: 写入接口条目，可通过 resolvconf -d 回滚。
    Resolvconf,
    /// NetworkManager: 保存连接原状态，失败或停止时恢复。
    NetworkManager { connections: Vec<NmConnectionState> },
    /// 直接写 /etc/resolv.conf（仅当是普通文件）。
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

static DNS_BACKEND_ORDER_CACHE: OnceLock<Vec<DnsBackendKind>> = OnceLock::new();

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
    /// /etc/resolv.conf 路径（固定）。
    path: PathBuf,
    /// 是否为符号链接，用于判断是系统管理还是手写文件。
    is_symlink: bool,
    /// 若为符号链接，记录目标，帮助识别后端类型。
    target: Option<PathBuf>,
    /// 读取内容用于启发式判断（比如 systemd-resolved/NetworkManager 标记）。
    contents: Option<String>,
}

/// 应用 DNS 配置。
///
/// 逻辑说明：
/// 1) 先检测 resolv.conf 当前状态，推断“优先后端”。
/// 2) 依序尝试 resolved/resolvconf/nmcli/直接写文件。
/// 3) 任一后端成功则返回；失败则记录并继续下一后端。
pub(super) async fn apply_dns(
    tun_name: &str,
    servers: &[IpAddr],
    search: &[String],
) -> Result<DnsState, NetworkError> {
    let info = read_resolv_conf_info();
    log_resolv_conf_info(&info);

    let backends = dns_backend_order(&info).await;
    let backend_order = backends
        .iter()
        .map(|backend| backend.as_str())
        .collect::<Vec<_>>()
        .join(" -> ");
    log_dns::backend_order(&backend_order);

    let mut last_error: Option<NetworkError> = None;

    for backend in backends {
        match backend {
            DnsBackendKind::Resolved => {
                let Some(resolvectl) = resolve_command("resolvectl") else {
                    log_dns::backend_not_found("resolvectl");
                    continue;
                };
                log_dns::backend_selected("resolvectl", &resolvectl);
                match apply_resolved(&resolvectl, tun_name, servers, search).await {
                    Ok(()) => {
                        return Ok(DnsState {
                            backend: DnsBackend::Resolved,
                        });
                    }
                    Err(err) => {
                        log_dns::backend_failed("resolvectl", &err);
                        last_error = Some(err);
                    }
                }
            }
            DnsBackendKind::Resolvconf => {
                let Some(resolvconf) = resolve_command("resolvconf") else {
                    log_dns::backend_not_found("resolvconf");
                    continue;
                };
                log_dns::backend_selected("resolvconf", &resolvconf);
                match apply_resolvconf(&resolvconf, tun_name, servers, search).await {
                    Ok(()) => {
                        return Ok(DnsState {
                            backend: DnsBackend::Resolvconf,
                        });
                    }
                    Err(err) => {
                        log_dns::backend_failed("resolvconf", &err);
                        last_error = Some(err);
                    }
                }
            }
            DnsBackendKind::NetworkManager => {
                let Some(nmcli) = resolve_command("nmcli") else {
                    log_dns::backend_not_found("nmcli");
                    continue;
                };
                log_dns::backend_selected("nmcli", &nmcli);
                match apply_network_manager(&nmcli, servers, search).await {
                    Ok(state) => {
                        return Ok(state);
                    }
                    Err(err) => {
                        log_dns::backend_failed("nmcli", &err);
                        last_error = Some(err);
                    }
                }
            }
            DnsBackendKind::ResolvConf => {
                match apply_resolv_conf_file(&info, servers, search) {
                    Ok(state) => {
                        return Ok(state);
                    }
                    Err(err) => {
                        log_dns::resolv_conf_failed(&err);
                        last_error = Some(err);
                    }
                }
            }
        }
    }

    // 所有后端都失败时，返回最后一个错误或“不支持”错误。
    Err(last_error.unwrap_or(NetworkError::DnsNotSupported))
}

/// 清理 DNS 配置。
///
/// 注意：必须与 apply_dns 使用的后端匹配，避免误删或破坏用户系统配置。
pub(super) async fn cleanup_dns(tun_name: &str, state: DnsState) -> Result<(), NetworkError> {
    match state.backend {
        DnsBackend::Resolved => {
            if let Some(resolvectl) = resolve_command("resolvectl") {
                // resolvectl revert 会撤销该接口的 DNS 配置。
                log_dns::resolvectl_revert(&resolvectl);
                run_cmd(
                    &resolvectl,
                    &vec!["revert".to_string(), tun_name.to_string()],
                )
                .await?
            }
        }
        DnsBackend::Resolvconf => {
            if let Some(resolvconf) = resolve_command("resolvconf") {
                // resolvconf -d 删除该接口的 DNS 记录。
                log_dns::resolvconf_revert(&resolvconf);
                run_cmd(&resolvconf, &vec!["-d".to_string(), tun_name.to_string()]).await?
            }
        }
        DnsBackend::NetworkManager { connections } => {
            if let Some(nmcli) = resolve_command("nmcli") {
                for conn in connections {
                    log_dns::nmcli_revert(&conn.name);
                    if let Err(err) = restore_nm_connection(&nmcli, &conn).await {
                        log_dns::nmcli_revert_failed(&err);
                    }
                }
            }
        }
        DnsBackend::ResolvConf { path, original } => {
            log_dns::resolv_conf_revert(&path);
            write_resolv_conf(&path, &original)?;
        }
    }
    Ok(())
}

/// 解析命令路径，优先使用 PATH，其次尝试常见系统目录。
///
/// 避免硬编码路径，同时确保在 PATH 缺失时仍可找到系统工具。
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
///
/// 仅当命令返回非 0 时抛错并带上 stderr，便于定位系统调用失败原因。
async fn run_cmd(program: &Path, args: &[String]) -> Result<(), NetworkError> {
    // 将实际执行的命令记录到日志，便于排查系统调用失败。
    log_command(program, args);
    let output = Command::new(program).args(args).output().await?;

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
///
/// 适用于 resolvconf 这类需要从 stdin 接收配置内容的命令。
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
    log_dns::exec(&format_command(program, args));
}

/// 组装可读的命令文本用于错误提示。
///
/// 注意：仅用于日志/错误，不进行 shell 转义。
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
    // systemd-resolved 的 per-link DNS：仅作用于指定接口。
    if !servers.is_empty() {
        let mut args = vec!["dns".to_string(), tun_name.to_string()];
        for server in servers {
            args.push(server.to_string());
        }
        run_cmd(resolvectl, &args).await?;
    }

    if !servers.is_empty() || !search.is_empty() {
        // 追加 ~. 代表全局路由域，确保默认走该接口解析。
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
    // resolvconf/openresolv 通过 stdin 写入临时接口条目。
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
    // 读取 resolv.conf 的符号链接与内容，用于推断系统 DNS 管理器。
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
            log_dns::resolv_conf_symlink(Some(target.as_path()));
        } else {
            log_dns::resolv_conf_symlink(None);
        }
    } else {
        log_dns::resolv_conf_regular();
    }
}

async fn dns_backend_order(info: &ResolvConfInfo) -> Vec<DnsBackendKind> {
    if let Some(cached) = DNS_BACKEND_ORDER_CACHE.get() {
        return cached.clone();
    }
    // 运行时探测可用后端，避免仅凭 resolv.conf 推断导致误选。
    let preferred = detect_preferred_backend(info);
    let (resolved_ok, nm_ok) = tokio::join!(probe_resolved_backend(), probe_network_manager());
    let resolvconf_ok = probe_resolvconf_backend();

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

    let _ = DNS_BACKEND_ORDER_CACHE.set(order.clone());
    order
}

/// 探测 systemd-resolved 是否可用（resolvectl status 成功即视为可用）。
async fn probe_resolved_backend() -> bool {
    let Some(resolvectl) = resolve_command("resolvectl") else {
        return false;
    };
    let args = vec!["status".to_string()];
    probe_command_status(&resolvectl, &args).await
}

/// 探测 resolvconf 是否存在（仅检查二进制）。
fn probe_resolvconf_backend() -> bool {
    resolve_command("resolvconf").is_some()
}

/// 探测 NetworkManager 是否处于运行态。
async fn probe_network_manager() -> bool {
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
async fn probe_command_status(program: &Path, args: &[String]) -> bool {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    match tokio::time::timeout(Duration::from_millis(800), cmd.status()).await {
        Ok(Ok(status)) => status.success(),
        _ => false,
    }
}

/// 探测命令输出（超时或失败返回 None）。
async fn probe_command_output(program: &Path, args: &[String]) -> Option<String> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let output = match tokio::time::timeout(Duration::from_millis(800), cmd.output()).await {
        Ok(Ok(output)) => output,
        _ => return None,
    };
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

fn detect_preferred_backend(info: &ResolvConfInfo) -> Option<DnsBackendKind> {
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

async fn apply_network_manager(
    nmcli: &Path,
    servers: &[IpAddr],
    search: &[String],
) -> Result<DnsState, NetworkError> {
    // 只处理 NM 管理的“活动”连接；无活动连接则认为不可用。
    let connections = nmcli_active_connections(nmcli).await?;
    if connections.is_empty() {
        return Err(NetworkError::DnsNotSupported);
    }

    let (v4_dns, v6_dns) = split_dns_servers(servers);
    let v4_dns = v4_dns.join(",");
    let v6_dns = v6_dns.join(",");
    let search_value = search.join(",");

    // 逐个连接应用 DNS，并保存原状态用于回滚。
    let mut touched = Vec::new();
    for conn in connections {
        let state = read_nm_connection_state(nmcli, &conn.name, &conn.device).await?;
        if let Err(err) = apply_nm_connection(
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

    // 记录 NM 与 resolv.conf 的即时状态，便于判断是否成功写入。
    log_nmcli_dns_snapshot(nmcli, &touched, "post-apply").await;
    log_resolv_conf_snapshot(Path::new("/etc/resolv.conf"), "post-apply");
    tokio::time::sleep(Duration::from_millis(800)).await;
    log_nmcli_dns_snapshot(nmcli, &touched, "after-wait").await;
    log_resolv_conf_snapshot(Path::new("/etc/resolv.conf"), "after-wait");

    if let Err(err) = wait_for_resolv_conf_servers(Path::new("/etc/resolv.conf"), servers).await {
        let mut final_err = err;
        if matches!(final_err, NetworkError::DnsVerifyFailed(_)) {
            // reapply 有时无法清掉 RDNSS 注入（如 fe80::1），尝试 down/up 强制刷新。
            log_dns::nmcli_verify_failed();
            if let Err(err) = nmcli_reconnect(nmcli, &touched).await {
                log_dns::nmcli_reconnect_failed(&err);
            } else {
                // 重新连接后再次采样并验证，确认是否仍残留意外 DNS。
                log_nmcli_dns_snapshot(nmcli, &touched, "post-reconnect").await;
                log_resolv_conf_snapshot(Path::new("/etc/resolv.conf"), "post-reconnect");
                tokio::time::sleep(Duration::from_millis(1200)).await;
                log_nmcli_dns_snapshot(nmcli, &touched, "after-reconnect-wait").await;
                log_resolv_conf_snapshot(Path::new("/etc/resolv.conf"), "after-reconnect-wait");
                match wait_for_resolv_conf_servers(Path::new("/etc/resolv.conf"), servers).await {
                    Ok(()) => {
                        return Ok(DnsState {
                            backend: DnsBackend::NetworkManager {
                                connections: touched,
                            },
                        });
                    }
                    Err(err) => {
                        final_err = err;
                    }
                }
            }
        }
        for restored in touched.iter().rev() {
            let _ = restore_nm_connection(nmcli, restored).await;
        }
        return Err(final_err);
    }

    Ok(DnsState {
        backend: DnsBackend::NetworkManager {
            connections: touched,
        },
    })
}

async fn nmcli_active_connections(nmcli: &Path) -> Result<Vec<NmConnection>, NetworkError> {
    // 只筛选 activated 且非 loopback/tun/vpn 的连接，避免误改隧道自身。
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
    // 读取连接现有 DNS 设置，供失败时回滚。
    let ipv4_dns = nmcli_get(nmcli, name, "ipv4.dns").await?;
    let ipv4_ignore_auto =
        normalize_nmcli_bool(nmcli_get(nmcli, name, "ipv4.ignore-auto-dns").await?);
    let ipv4_search = nmcli_get(nmcli, name, "ipv4.dns-search").await?;
    let ipv4_priority = nmcli_get(nmcli, name, "ipv4.dns-priority").await?;
    let ipv6_dns = nmcli_get(nmcli, name, "ipv6.dns").await?;
    let ipv6_ignore_auto =
        normalize_nmcli_bool(nmcli_get(nmcli, name, "ipv6.ignore-auto-dns").await?);
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

async fn nmcli_get(nmcli: &Path, name: &str, field: &str) -> Result<String, NetworkError> {
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
    // ignore-auto-dns=yes + dns-priority=-42 让 NM 以手动 DNS 为“独占”优先级。
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
    // 用之前保存的值还原，尽量避免破坏用户网络配置。
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
    // reapply 不会完全断网，但某些情况下不会清掉 RA 注入的 DNS。
    let args = vec![
        "device".to_string(),
        "reapply".to_string(),
        device.to_string(),
    ];
    run_cmd(nmcli, &args).await
}

async fn nmcli_reconnect(
    nmcli: &Path,
    connections: &[NmConnectionState],
) -> Result<(), NetworkError> {
    // down/up 会短暂断网，但可强制 NM 重新应用连接设置。
    for conn in connections {
        let args = vec![
            "connection".to_string(),
            "down".to_string(),
            conn.name.clone(),
        ];
        run_cmd(nmcli, &args).await?;
    }
    for conn in connections {
        let args = vec![
            "connection".to_string(),
            "up".to_string(),
            conn.name.clone(),
            "ifname".to_string(),
            conn.device.clone(),
        ];
        run_cmd(nmcli, &args).await?;
    }
    Ok(())
}

fn apply_resolv_conf_file(
    info: &ResolvConfInfo,
    servers: &[IpAddr],
    search: &[String],
) -> Result<DnsState, NetworkError> {
    // 仅允许在 resolv.conf 是普通文件时写入，避免与系统管理器冲突。
    if info.is_symlink {
        log_dns::resolv_conf_skipped_symlink();
        return Err(NetworkError::DnsNotSupported);
    }

    let original = fs::read_to_string(&info.path)?;
    let content = build_resolv_conf_contents(servers, search);
    write_resolv_conf(&info.path, &content)?;
    // 写入后必须验证，防止部分系统立即覆盖导致“误判成功”。
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
    // 生成最小 resolv.conf，仅包含 search 与 nameserver。
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
    // 直接截断写入，失败则抛出 io error。
    let mut file = fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)?;
    file.write_all(contents.as_bytes())?;
    Ok(())
}

fn split_dns_servers(servers: &[IpAddr]) -> (Vec<String>, Vec<String>) {
    // 分离 IPv4/IPv6，便于 nmcli 字段写入。
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
    // 需要 stdout 的命令，用于 nmcli 查询/调试。
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
    // 严格模式：不允许出现“额外的” DNS，避免泄漏或旁路解析。
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

async fn wait_for_resolv_conf_servers(path: &Path, servers: &[IpAddr]) -> Result<(), NetworkError> {
    // 允许系统异步写入（如 NM reapply/RA），短暂重试等待稳定态。
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

async fn log_nmcli_dns_snapshot(nmcli: &Path, connections: &[NmConnectionState], label: &str) {
    // 辅助日志：用于判断 NM 是否已写入期望 DNS。
    for state in connections {
        let args = vec![
            "-f".to_string(),
            "IP4.DNS,IP6.DNS,IP4.DOMAIN,IP6.DOMAIN".to_string(),
            "device".to_string(),
            "show".to_string(),
            state.device.clone(),
        ];
        match run_cmd_capture(nmcli, &args).await {
            Ok(output) => {
                let output = output.trim();
                if output.is_empty() {
                    log_dns::nmcli_snapshot_empty(label, &state.device);
                } else {
                    log_dns::nmcli_snapshot(label, &state.device, output);
                }
            }
            Err(err) => {
                log_dns::nmcli_snapshot_failed(label, &state.device, &err);
            }
        }
    }
}

fn log_resolv_conf_snapshot(path: &Path, label: &str) {
    // resolv.conf 里的 nameserver 可能包含 zone index（%eth0），读时剥离。
    match read_resolv_conf_servers(path) {
        Ok((v4, v6)) => {
            log_dns::resolv_conf_snapshot(label, &v4, &v6);
        }
        Err(err) => {
            log_dns::resolv_conf_snapshot_failed(label, &err);
        }
    }
}

fn read_resolv_conf_servers(path: &Path) -> Result<(Vec<IpAddr>, Vec<IpAddr>), NetworkError> {
    // 只解析 nameserver 行；忽略注释与空行。
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
        // IPv6 可能带有 scope（如 fe80::1%enp0s3），需去掉 % 后再解析。
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
