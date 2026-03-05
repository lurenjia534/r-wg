//! Windows NRPT（Name Resolution Policy Table）策略下发。
//!
//! 目的：在全隧道场景下，把系统默认 DNS 查询策略强制指向隧道 DNS，
//! 避免仅靠接口 DNS 设置时出现的跨接口回退与超时。

use std::net::IpAddr;
use std::os::windows::process::CommandExt;
use std::process::Command;

use super::adapter::AdapterInfo;
use super::NetworkError;
use crate::log::events::net as log_net;

/// Windows CreateProcess 标志：让控制台子进程在后台运行而不弹出黑窗。
/// 这里用于调用 `powershell` 管理 NRPT 规则，避免用户在开关隧道时看到闪烁窗口。
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// NRPT 规则回滚状态。
#[derive(Clone)]
pub(super) struct NrptState {
    /// 规则实际 Name（用于精准删除）。
    rule_names: Vec<String>,
    /// 本客户端规则标签（用于批量兜底清理）。
    tag: String,
}

/// 应用 NRPT 规则。
///
/// 策略：对 `.`（根命名空间）下发 NameServers，实现“全域 DNS 指向隧道 DNS”。
pub(super) fn apply_nrpt_guard(
    adapter: AdapterInfo,
    dns_servers: &[IpAddr],
) -> Result<Option<NrptState>, NetworkError> {
    if dns_servers.is_empty() {
        return Ok(None);
    }

    let tag = format!("r-wg-nrpt-if{}", adapter.if_index);
    let servers = dns_servers
        .iter()
        .map(|ip| ip.to_string())
        .collect::<Vec<_>>()
        .join(",");

    // 先清理同标签旧规则，避免重连叠加。
    let _ = remove_rules_by_tag(&tag);

    let script = format!(
        "$ErrorActionPreference='Stop'; \
         $tag='{tag}'; \
         $servers='{servers}'.Split(',') | Where-Object {{ $_ -ne '' }}; \
         $rule=Add-DnsClientNrptRule -Namespace '.' -NameServers $servers -DisplayName $tag -Comment $tag -PassThru; \
         $rule.Name"
    );

    let output = run_powershell(&script)?;
    let names = output
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .map(|line| line.to_string())
        .collect::<Vec<_>>();

    if names.is_empty() {
        return Err(NetworkError::UnsafeRouting(
            "NRPT apply returned no rule name; refusing to continue".to_string(),
        ));
    }

    log_net::nrpt_apply(dns_servers.len(), names.len());

    Ok(Some(NrptState {
        rule_names: names,
        tag,
    }))
}

/// 回滚 NRPT 规则。
pub(super) fn cleanup_nrpt_guard(state: NrptState) -> Result<(), NetworkError> {
    let mut first_error: Option<NetworkError> = None;

    for name in &state.rule_names {
        if let Err(err) = remove_rule_by_name(name) {
            if first_error.is_none() {
                first_error = Some(err);
            }
        }
    }

    // 再按 tag 清理一次，兜底处理异常中断场景。
    if let Err(err) = remove_rules_by_tag(&state.tag) {
        if first_error.is_none() {
            first_error = Some(err);
        }
    }

    if let Some(err) = first_error {
        Err(err)
    } else {
        Ok(())
    }
}

/// 按规则 Name 删除 NRPT 规则。
fn remove_rule_by_name(name: &str) -> Result<(), NetworkError> {
    let script = format!(
        "$ErrorActionPreference='SilentlyContinue'; \
         Remove-DnsClientNrptRule -Name '{name}' -Force"
    );
    run_powershell(&script).map(|_| ())
}

/// 按 Comment 标签批量删除 NRPT 规则。
fn remove_rules_by_tag(tag: &str) -> Result<(), NetworkError> {
    let script = format!(
        "$ErrorActionPreference='SilentlyContinue'; \
         Get-DnsClientNrptRule | Where-Object {{ $_.Comment -eq '{tag}' }} | ForEach-Object {{ Remove-DnsClientNrptRule -Name $_.Name -Force -ErrorAction SilentlyContinue }}"
    );
    run_powershell(&script).map(|_| ())
}

/// 执行 PowerShell 命令并返回 stdout。
fn run_powershell(script: &str) -> Result<String, NetworkError> {
    let output = Command::new("powershell")
        // 避免每次执行 NRPT 命令都创建可见控制台窗口。
        .creation_flags(CREATE_NO_WINDOW)
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .map_err(NetworkError::Io)?;

    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).to_string());
    }

    let detail = command_detail(&output.stdout, &output.stderr);
    Err(NetworkError::UnsafeRouting(format!(
        "NRPT command failed: {detail}"
    )))
}

/// 从命令输出中提取错误详情。
fn command_detail(stdout: &[u8], stderr: &[u8]) -> String {
    let stderr_text = String::from_utf8_lossy(stderr).trim().to_string();
    if !stderr_text.is_empty() {
        return stderr_text;
    }
    let stdout_text = String::from_utf8_lossy(stdout).trim().to_string();
    if !stdout_text.is_empty() {
        return stdout_text;
    }
    "powershell returned a failure status".to_string()
}
