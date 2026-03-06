use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::process::Command;

use super::super::NetworkError;
use crate::log::events::dns as log_dns;

/// 解析命令路径，优先使用 PATH，其次尝试常见系统目录。
///
/// 避免硬编码路径，同时确保在 PATH 缺失时仍可找到系统工具。
pub(super) fn resolve_command(program: &str) -> Option<PathBuf> {
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
pub(super) async fn run_cmd(program: &Path, args: &[String]) -> Result<(), NetworkError> {
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
pub(super) async fn run_cmd_with_input(
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

/// 需要 stdout 的命令，用于 nmcli 查询/调试。
pub(super) async fn run_cmd_capture(
    program: &Path,
    args: &[String],
) -> Result<String, NetworkError> {
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
