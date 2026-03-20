use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::process::Command;

use super::super::NetworkError;
use crate::log::events::dns as log_dns;

/// 解析命令路径，优先使用 PATH，其次尝试常见系统目录。
///
/// 为避免 PATH 劫持，仅在受信任的系统目录中查找工具。
pub(super) fn resolve_command(program: &str) -> Option<PathBuf> {
    if program.contains('/') {
        let path = PathBuf::from(program);
        return path.is_file().then_some(path);
    }

    // 仅在常见系统目录中查找，避免受到进程环境 PATH 影响。
    for dir in [
        "/usr/local/sbin",
        "/usr/local/bin",
        "/usr/sbin",
        "/usr/bin",
        "/sbin",
        "/bin",
    ] {
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

#[cfg(test)]
mod tests {
    use super::resolve_command;
    use std::env;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct PathGuard {
        original: Option<std::ffi::OsString>,
    }

    impl PathGuard {
        fn set(path: &str) -> Self {
            let original = env::var_os("PATH");
            // Tests share one process; save and restore PATH around each mutation.
            unsafe {
                env::set_var("PATH", path);
            }
            Self { original }
        }
    }

    impl Drop for PathGuard {
        fn drop(&mut self) {
            if let Some(value) = self.original.take() {
                unsafe {
                    env::set_var("PATH", value);
                }
            } else {
                unsafe {
                    env::remove_var("PATH");
                }
            }
        }
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_nanos();
        let pid = std::process::id();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = env::temp_dir().join(format!("r_wg_{prefix}_{pid}_{nanos}_{seq}"));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn resolve_command_does_not_use_path_entries() {
        let temp_dir = unique_temp_dir("path_hijack");
        let _guard = PathGuard::set(temp_dir.to_str().expect("utf8 temp path"));

        let fake_name = "r_wg_fake_dns_command";
        let fake_binary = temp_dir.join(fake_name);
        fs::write(&fake_binary, "#!/bin/sh\necho hijack\n").expect("write fake binary");

        assert!(resolve_command(fake_name).is_none());

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn resolve_command_accepts_absolute_path() {
        let temp_dir = unique_temp_dir("absolute");
        let binary = temp_dir.join("tool");
        fs::write(&binary, "placeholder").expect("write placeholder binary");

        assert_eq!(
            resolve_command(binary.to_str().expect("utf8 path")),
            Some(binary.clone())
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
