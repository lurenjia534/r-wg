//! DNS 配置与回滚逻辑。
//!
//! 设计目标：
//! - 优先使用系统原生后端（systemd-resolved），再尝试兼容后端（resolvconf）。
//! - 只处理“对接口生效”的 DNS 设置，不直接修改全局 resolv.conf。
//! - 一旦设置失败，向上抛错，由上层决定是否允许“带警告启动”。

use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::process::Stdio;

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
}

/// 应用 DNS 配置。
///
/// 优先使用 `resolvectl`，否则使用 `resolvconf`。
pub(super) async fn apply_dns(
    tun_name: &str,
    servers: &[IpAddr],
    search: &[String],
) -> Result<DnsState, NetworkError> {
    // 优先尝试 systemd-resolved：按接口设置 DNS，适合现代发行版。
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

    // 再尝试 resolvconf：适配仍以 resolvconf 管理 DNS 的系统。
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

    // 两种后端都不可用时，返回明确错误，交给上层决定策略。
    Err(NetworkError::DnsNotSupported)
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
