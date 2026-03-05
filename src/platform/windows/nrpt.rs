use std::net::IpAddr;
use std::process::Command;

use super::adapter::AdapterInfo;
use super::NetworkError;
use crate::log::events::net as log_net;

#[derive(Clone)]
pub(super) struct NrptState {
    rule_names: Vec<String>,
    tag: String,
}

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

pub(super) fn cleanup_nrpt_guard(state: NrptState) -> Result<(), NetworkError> {
    let mut first_error: Option<NetworkError> = None;

    for name in &state.rule_names {
        if let Err(err) = remove_rule_by_name(name) {
            if first_error.is_none() {
                first_error = Some(err);
            }
        }
    }

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

fn remove_rule_by_name(name: &str) -> Result<(), NetworkError> {
    let script = format!(
        "$ErrorActionPreference='SilentlyContinue'; \
         Remove-DnsClientNrptRule -Name '{name}' -Force"
    );
    run_powershell(&script).map(|_| ())
}

fn remove_rules_by_tag(tag: &str) -> Result<(), NetworkError> {
    let script = format!(
        "$ErrorActionPreference='SilentlyContinue'; \
         Get-DnsClientNrptRule | Where-Object {{ $_.Comment -eq '{tag}' }} | ForEach-Object {{ Remove-DnsClientNrptRule -Name $_.Name -Force -ErrorAction SilentlyContinue }}"
    );
    run_powershell(&script).map(|_| ())
}

fn run_powershell(script: &str) -> Result<String, NetworkError> {
    let output = Command::new("powershell")
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
