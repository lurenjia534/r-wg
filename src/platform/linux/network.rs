use std::collections::HashSet;
use std::net::IpAddr;
use std::process::Stdio;

use tokio::process::Command;

use crate::backend::wg::config::{AllowedIp, InterfaceAddress, InterfaceConfig, PeerConfig, RouteTable};

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
    CommandFailed {
        command: String,
        status: Option<i32>,
        stderr: String,
    },
    DnsNotSupported,
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
        }
    }
}

impl std::error::Error for NetworkError {}

impl From<std::io::Error> for NetworkError {
    fn from(err: std::io::Error) -> Self {
        NetworkError::Io(err)
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
    ensure_command("ip").await?;

    if let Some(mtu) = interface.mtu {
        run_cmd(
            "ip",
            &[
                "link".to_string(),
                "set".to_string(),
                "dev".to_string(),
                tun_name.to_string(),
                "mtu".to_string(),
                mtu.to_string(),
            ],
        )
        .await?;
    }

    run_cmd(
        "ip",
        &[
            "link".to_string(),
            "set".to_string(),
            "dev".to_string(),
            tun_name.to_string(),
            "up".to_string(),
        ],
    )
    .await?;

    for address in &interface.addresses {
        let args = vec![
            ip_family_flag(address.addr),
            "address".to_string(),
            "replace".to_string(),
            format!("{}/{}", address.addr, address.cidr),
            "dev".to_string(),
            tun_name.to_string(),
        ];
        run_cmd("ip", &args).await?;
    }

    let routes = collect_allowed_routes(peers);
    if interface.table != Some(RouteTable::Off) {
        for route in &routes {
            let mut args = vec![
                ip_family_flag(route.addr),
                "route".to_string(),
                "replace".to_string(),
                format!("{}/{}", route.addr, route.cidr),
                "dev".to_string(),
                tun_name.to_string(),
            ];
            if let Some(table) = route_table_arg(interface.table) {
                args.push("table".to_string());
                args.push(table);
            }
            run_cmd("ip", &args).await?;
        }
    }

    let dns = if interface.dns_servers.is_empty() && interface.dns_search.is_empty() {
        None
    } else {
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
    ensure_command("ip").await?;

    for address in &state.addresses {
        let args = vec![
            ip_family_flag(address.addr),
            "address".to_string(),
            "del".to_string(),
            format!("{}/{}", address.addr, address.cidr),
            "dev".to_string(),
            state.tun_name.clone(),
        ];
        let _ = run_cmd("ip", &args).await;
    }

    if state.table != Some(RouteTable::Off) {
        for route in &state.routes {
            let mut args = vec![
                ip_family_flag(route.addr),
                "route".to_string(),
                "del".to_string(),
                format!("{}/{}", route.addr, route.cidr),
                "dev".to_string(),
                state.tun_name.clone(),
            ];
            if let Some(table) = route_table_arg(state.table) {
                args.push("table".to_string());
                args.push(table);
            }
            let _ = run_cmd("ip", &args).await;
        }
    }

    if let Some(dns) = state.dns {
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

/// 从 RouteTable 生成 `ip route ... table` 的参数。
fn route_table_arg(table: Option<RouteTable>) -> Option<String> {
    match table {
        Some(RouteTable::Id(value)) => Some(value.to_string()),
        _ => None,
    }
}

/// 根据 IP 类型返回 `-4` 或 `-6`。
fn ip_family_flag(addr: IpAddr) -> String {
    if addr.is_ipv4() {
        "-4".to_string()
    } else {
        "-6".to_string()
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
    if command_exists("resolvectl").await {
        let mut args = vec!["dns".to_string(), tun_name.to_string()];
        for server in servers {
            args.push(server.to_string());
        }
        run_cmd("resolvectl", &args).await?;

        if !search.is_empty() {
            let mut domain_args = vec!["domain".to_string(), tun_name.to_string()];
            domain_args.extend(search.iter().cloned());
            run_cmd("resolvectl", &domain_args).await?;
        }

        return Ok(DnsState {
            backend: DnsBackend::Resolved,
        });
    }

    if command_exists("resolvconf").await {
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
        run_cmd_with_input("resolvconf", &args, &content).await?;
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
            run_cmd("resolvectl", &vec!["revert".to_string(), tun_name.to_string()]).await?
        }
        DnsBackend::Resolvconf => {
            run_cmd("resolvconf", &vec!["-d".to_string(), tun_name.to_string()]).await?
        }
    }
    Ok(())
}

/// 确保命令存在，否则返回错误。
async fn ensure_command(program: &str) -> Result<(), NetworkError> {
    if command_exists(program).await {
        Ok(())
    } else {
        Err(NetworkError::CommandFailed {
            command: program.to_string(),
            status: None,
            stderr: "command not found".to_string(),
        })
    }
}

/// 通过 `--version` 探测命令是否存在。
async fn command_exists(program: &str) -> bool {
    let status = Command::new(program)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;
    match status {
        Ok(status) => status.success(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => false,
    }
}

/// 执行命令并检查返回码。
async fn run_cmd(program: &str, args: &[String]) -> Result<(), NetworkError> {
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
    program: &str,
    args: &[String],
    input: &str,
) -> Result<(), NetworkError> {
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
fn format_command(program: &str, args: &[String]) -> String {
    let mut command = String::new();
    command.push_str(program);
    for arg in args {
        command.push(' ');
        command.push_str(arg);
    }
    command
}
