use std::collections::HashSet;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use futures_util::stream::TryStreamExt;
use netlink_packet_route::route::{RouteAddress, RouteAttribute, RouteHeader, RouteMessage};
use rtnetlink::{new_connection, Handle, IpVersion};
use tokio::process::Command;

use crate::backend::wg::config::{AllowedIp, InterfaceAddress, InterfaceConfig, PeerConfig, RouteTable};
use crate::log;

#[derive(Debug)]
pub struct AppliedNetworkState {
    tun_name: String,
    addresses: Vec<InterfaceAddress>,
    routes: Vec<AllowedIp>,
    table: Option<RouteTable>,
    dns: Option<DnsState>,
}

#[derive(Debug)]
pub enum NetworkError {
    Io(std::io::Error),
    Netlink(rtnetlink::Error),
    CommandFailed {
        command: String,
        status: Option<i32>,
        stderr: String,
    },
    DnsNotSupported,
    LinkNotFound(String),
}

impl std::fmt::Display for NetworkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetworkError::Io(err) => write!(f, "io error: {err}"),
            NetworkError::CommandFailed {
                command,
                status,
                stderr,
            } => write!(
                f,
                "command failed: {command} (status={status:?}) {stderr}"
            ),
            NetworkError::DnsNotSupported => write!(f, "no supported DNS backend found"),
            NetworkError::Netlink(err) => write!(f, "netlink error: {err}"),
            NetworkError::LinkNotFound(name) => write!(f, "link not found: {name}"),
        }
    }
}

impl std::error::Error for NetworkError {}

impl From<std::io::Error> for NetworkError {
    fn from(err: std::io::Error) -> Self {
        NetworkError::Io(err)
    }
}

impl From<rtnetlink::Error> for NetworkError {
    fn from(err: rtnetlink::Error) -> Self {
        NetworkError::Netlink(err)
    }
}

#[derive(Debug)]
struct DnsState {
    backend: DnsBackend,
}

#[derive(Debug)]
enum DnsBackend {
    Resolved,
    Resolvconf,
}

/// 应用 Linux 网络配置。
///
/// 只负责系统地址/路由/DNS，WireGuard 隧道本身由 gotatun 负责。
pub async fn apply_network_config(
    tun_name: &str,
    interface: &InterfaceConfig,
    peers: &[PeerConfig],
) -> Result<AppliedNetworkState, NetworkError> {
    log_net(format!(
        "apply: tun={tun_name} mtu={:?} addr_count={} route_table={:?} dns_servers={} dns_search={}",
        interface.mtu,
        interface.addresses.len(),
        interface.table,
        interface.dns_servers.len(),
        interface.dns_search.len()
    ));
    log_privileges();

    let handle = netlink_handle()?;
    let link_index = link_index(&handle, tun_name).await?;
    log_net(format!("link index: {link_index}"));

    if let Some(mtu) = interface.mtu {
        handle.link().set(link_index).mtu(mtu.into()).execute().await?;
    }

    handle.link().set(link_index).up().execute().await?;

    for address in &interface.addresses {
        log_net(format!("address: {}/{}", address.addr, address.cidr));
        handle
            .address()
            .add(link_index, address.addr, address.cidr)
            .execute()
            .await?;
    }

    let routes = collect_allowed_routes(peers);
    if interface.table != Some(RouteTable::Off) {
        let table = route_table_id(interface.table);
        for route in &routes {
            log_net(format!("route: {}/{}", route.addr, route.cidr));
            match route.addr {
                IpAddr::V4(addr) => {
                    let mut request = handle
                        .route()
                        .add()
                        .v4()
                        .destination_prefix(addr, route.cidr)
                        .output_interface(link_index);
                    if let Some(table) = table {
                        request = request.table_id(table);
                    }
                    request.execute().await?;
                }
                IpAddr::V6(addr) => {
                    let mut request = handle
                        .route()
                        .add()
                        .v6()
                        .destination_prefix(addr, route.cidr)
                        .output_interface(link_index);
                    if let Some(table) = table {
                        request = request.table_id(table);
                    }
                    request.execute().await?;
                }
            }
        }
    }

    let dns = if interface.dns_servers.is_empty() && interface.dns_search.is_empty() {
        None
    } else {
        log_net(format!(
            "dns: servers={} search={}",
            interface.dns_servers.len(),
            interface.dns_search.len()
        ));
        Some(apply_dns(tun_name, &interface.dns_servers, &interface.dns_search).await?)
    };

    Ok(AppliedNetworkState {
        tun_name: tun_name.to_string(),
        addresses: interface.addresses.clone(),
        routes,
        table: interface.table,
        dns,
    })
}

/// 清理之前应用的网络配置。
pub async fn cleanup_network_config(state: AppliedNetworkState) -> Result<(), NetworkError> {
    log_net(format!(
        "cleanup: tun={} addr_count={} route_count={} table={:?} dns={}",
        state.tun_name,
        state.addresses.len(),
        state.routes.len(),
        state.table,
        state.dns.is_some()
    ));
    let handle = netlink_handle()?;
    let link_index = match link_index(&handle, &state.tun_name).await {
        Ok(index) => index,
        Err(err) => {
            log_net(format!("link lookup failed: {err}"));
            return Ok(());
        }
    };

    for address in &state.addresses {
        log_net(format!("address del: {}/{}", address.addr, address.cidr));
        if let Err(err) = delete_address(&handle, link_index, address).await {
            log_net(format!("address del failed: {err}"));
        }
    }

    if state.table != Some(RouteTable::Off) {
        let table = route_table_id(state.table);
        for route in &state.routes {
            log_net(format!("route del: {}/{}", route.addr, route.cidr));
            if let Err(err) = delete_route(&handle, link_index, route, table).await {
                log_net(format!("route del failed: {err}"));
            }
        }
    }

    if let Some(dns) = state.dns {
        log_net("dns revert".to_string());
        let _ = cleanup_dns(state.tun_name.as_str(), dns).await;
    }

    Ok(())
}

/// 从所有 peer 收集 AllowedIPs 并去重。
fn collect_allowed_routes(peers: &[PeerConfig]) -> Vec<AllowedIp> {
    let mut seen = HashSet::new();
    let mut routes = Vec::new();
    for peer in peers {
        for allowed in &peer.allowed_ips {
            if seen.insert((allowed.addr, allowed.cidr)) {
                routes.push(AllowedIp {
                    addr: allowed.addr,
                    cidr: allowed.cidr,
                });
            }
        }
    }
    routes
}

/// 从 RouteTable 生成 netlink 路由表 ID。
fn route_table_id(table: Option<RouteTable>) -> Option<u32> {
    match table {
        Some(RouteTable::Id(value)) => Some(value),
        _ => None,
    }
}

/// 应用 DNS 配置。
///
/// 优先使用 `resolvectl`，否则使用 `resolvconf`。
async fn apply_dns(
    tun_name: &str,
    servers: &[IpAddr],
    search: &[String],
) -> Result<DnsState, NetworkError> {
    if let Some(resolvectl) = resolve_command("resolvectl") {
        log_net(format!("dns backend: resolvectl ({})", resolvectl.display()));
        let mut args = vec!["dns".to_string(), tun_name.to_string()];
        for server in servers {
            args.push(server.to_string());
        }
        run_cmd(&resolvectl, &args).await?;

        if !search.is_empty() {
            let mut domain_args = vec!["domain".to_string(), tun_name.to_string()];
            domain_args.extend(search.iter().cloned());
            run_cmd(&resolvectl, &domain_args).await?;
        }

        return Ok(DnsState {
            backend: DnsBackend::Resolved,
        });
    }

    if let Some(resolvconf) = resolve_command("resolvconf") {
        log_net(format!("dns backend: resolvconf ({})", resolvconf.display()));
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
        run_cmd_with_input(&resolvconf, &args, &content).await?;
        return Ok(DnsState {
            backend: DnsBackend::Resolvconf,
        });
    }

    Err(NetworkError::DnsNotSupported)
}

/// 清理 DNS 配置。
async fn cleanup_dns(tun_name: &str, state: DnsState) -> Result<(), NetworkError> {
    match state.backend {
        DnsBackend::Resolved => {
            if let Some(resolvectl) = resolve_command("resolvectl") {
                log_net(format!("dns revert: resolvectl ({})", resolvectl.display()));
                run_cmd(&resolvectl, &vec!["revert".to_string(), tun_name.to_string()]).await?
            }
        }
        DnsBackend::Resolvconf => {
            if let Some(resolvconf) = resolve_command("resolvconf") {
                log_net(format!("dns revert: resolvconf ({})", resolvconf.display()));
                run_cmd(&resolvconf, &vec!["-d".to_string(), tun_name.to_string()]).await?
            }
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

    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join(program);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    for dir in ["/usr/sbin", "/sbin", "/usr/bin", "/bin"] {
        let candidate = Path::new(dir).join(program);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    None
}

fn log_net(message: String) {
    log::log("net", message);
}

fn log_command(program: &Path, args: &[String]) {
    log_net(format!("exec: {}", format_command(program, args)));
}

fn netlink_handle() -> Result<Handle, NetworkError> {
    let (connection, handle, _) = new_connection()?;
    tokio::spawn(connection);
    Ok(handle)
}

async fn link_index(handle: &Handle, tun_name: &str) -> Result<u32, NetworkError> {
    let mut links = handle.link().get().match_name(tun_name.to_string()).execute();
    match links.try_next().await? {
        Some(link) => Ok(link.header.index),
        None => Err(NetworkError::LinkNotFound(tun_name.to_string())),
    }
}

async fn delete_address(
    handle: &Handle,
    link_index: u32,
    address: &InterfaceAddress,
) -> Result<(), NetworkError> {
    let mut addresses = handle
        .address()
        .get()
        .set_link_index_filter(link_index)
        .set_address_filter(address.addr)
        .set_prefix_length_filter(address.cidr)
        .execute();
    if let Some(message) = addresses.try_next().await? {
        handle.address().del(message).execute().await?;
    }
    Ok(())
}

async fn delete_route(
    handle: &Handle,
    link_index: u32,
    route: &AllowedIp,
    table: Option<u32>,
) -> Result<(), NetworkError> {
    let ip_version = match route.addr {
        IpAddr::V4(_) => IpVersion::V4,
        IpAddr::V6(_) => IpVersion::V6,
    };
    let mut routes = handle.route().get(ip_version).execute();
    while let Some(message) = routes.try_next().await? {
        if route_message_matches(&message, link_index, route, table) {
            handle.route().del(message).execute().await?;
            break;
        }
    }
    Ok(())
}

fn route_message_matches(
    message: &RouteMessage,
    link_index: u32,
    route: &AllowedIp,
    table: Option<u32>,
) -> bool {
    if message.header.destination_prefix_length != route.cidr {
        return false;
    }

    if route_message_oif(message) != Some(link_index) {
        return false;
    }

    let expected_table = match table {
        Some(value) => value,
        None => RouteHeader::RT_TABLE_MAIN as u32,
    };
    if route_message_table_id(message) != expected_table {
        return false;
    }

    match (route.addr, route_message_destination(message)) {
        (IpAddr::V4(addr), Some(IpAddr::V4(dst))) => addr == dst,
        (IpAddr::V6(addr), Some(IpAddr::V6(dst))) => addr == dst,
        _ => false,
    }
}

fn route_message_table_id(message: &RouteMessage) -> u32 {
    for attr in &message.attributes {
        if let RouteAttribute::Table(value) = attr {
            return *value;
        }
    }
    message.header.table as u32
}

fn route_message_oif(message: &RouteMessage) -> Option<u32> {
    message
        .attributes
        .iter()
        .find_map(|attr| if let RouteAttribute::Oif(value) = attr { Some(*value) } else { None })
}

fn route_message_destination(message: &RouteMessage) -> Option<IpAddr> {
    message.attributes.iter().find_map(|attr| match attr {
        RouteAttribute::Destination(RouteAddress::Inet(addr)) => Some(IpAddr::V4(*addr)),
        RouteAttribute::Destination(RouteAddress::Inet6(addr)) => Some(IpAddr::V6(*addr)),
        _ => None,
    })
}

fn log_privileges() {
    if !log::enabled() {
        return;
    }

    let status = match std::fs::read_to_string("/proc/self/status") {
        Ok(status) => status,
        Err(err) => {
            log_net(format!("proc status read failed: {err}"));
            return;
        }
    };

    let euid = parse_status_uid(&status);
    let cap_eff = parse_status_cap_eff(&status);
    match (euid, cap_eff) {
        (Some(euid), Some(cap_eff)) => {
            let cap_net_admin = 1u64 << 12;
            let has_net_admin = (cap_eff & cap_net_admin) != 0;
            log_net(format!(
                "euid={euid} cap_eff=0x{cap_eff:x} net_admin={has_net_admin}"
            ));
        }
        _ => {
            log_net("proc status parse failed".to_string());
        }
    }
}

fn parse_status_uid(status: &str) -> Option<u32> {
    status
        .lines()
        .find(|line| line.starts_with("Uid:"))
        .and_then(|line| line.split_whitespace().nth(2))
        .and_then(|value| value.parse().ok())
}

fn parse_status_cap_eff(status: &str) -> Option<u64> {
    status
        .lines()
        .find(|line| line.starts_with("CapEff:"))
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| u64::from_str_radix(value, 16).ok())
}

/// 执行命令并检查返回码。
async fn run_cmd(program: &Path, args: &[String]) -> Result<(), NetworkError> {
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

/// 组装可读的命令文本用于错误提示。
fn format_command(program: &Path, args: &[String]) -> String {
    let mut command = program.display().to_string();
    for arg in args {
        command.push(' ');
        command.push_str(arg);
    }
    command
}
