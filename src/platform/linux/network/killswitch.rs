use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::{Deserialize, Serialize};
use tokio::process::Command;

use super::NetworkError;
use crate::log::events::net as log_net;

const NFT_TABLE_NAME: &str = "r_wg_killswitch";
const IPTABLES_CHAIN_NAME: &str = "R_WG_KILLSWITCH";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct KillSwitchState {
    backend: KillSwitchBackend,
    tun_name: String,
    fwmark: u32,
    ipv4: bool,
    ipv6: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum KillSwitchBackend {
    Nftables,
    Iptables,
}

pub(super) async fn apply_kill_switch(
    tun_name: &str,
    fwmark: u32,
    ipv4: bool,
    ipv6: bool,
) -> Result<KillSwitchState, NetworkError> {
    if !ipv4 && !ipv6 {
        return Err(NetworkError::KillSwitchUnavailable(
            "no full-tunnel address families require protection".to_string(),
        ));
    }

    let mut nft_error = None;
    if let Some(nft) = resolve_command("nft") {
        match apply_nftables(&nft, tun_name, fwmark, ipv4, ipv6).await {
            Ok(()) => {
                log_net::kill_switch_apply(tun_name, "nftables", ipv4, ipv6);
                return Ok(KillSwitchState {
                    backend: KillSwitchBackend::Nftables,
                    tun_name: tun_name.to_string(),
                    fwmark,
                    ipv4,
                    ipv6,
                });
            }
            Err(err) => {
                let _ = cleanup_nftables(&nft).await;
                nft_error = Some(err);
            }
        }
    }

    match apply_iptables(tun_name, fwmark, ipv4, ipv6).await {
        Ok(()) => {
            log_net::kill_switch_apply(tun_name, "iptables", ipv4, ipv6);
            Ok(KillSwitchState {
                backend: KillSwitchBackend::Iptables,
                tun_name: tun_name.to_string(),
                fwmark,
                ipv4,
                ipv6,
            })
        }
        Err(NetworkError::KillSwitchUnavailable(_)) => match nft_error {
            Some(err) => Err(err),
            None => Err(NetworkError::KillSwitchUnavailable(
                "nftables and iptables backends are unavailable".to_string(),
            )),
        },
        Err(err) => Err(err),
    }
}

impl KillSwitchState {
    pub(super) async fn cleanup(self) -> Result<(), NetworkError> {
        match self.backend {
            KillSwitchBackend::Nftables => {
                let nft = resolve_command("nft").ok_or_else(|| {
                    NetworkError::KillSwitchUnavailable("nft command not found".to_string())
                })?;
                cleanup_nftables(&nft).await
            }
            KillSwitchBackend::Iptables => cleanup_iptables(self.ipv4, self.ipv6).await,
        }
    }
}

async fn apply_nftables(
    nft: &Path,
    tun_name: &str,
    fwmark: u32,
    ipv4: bool,
    ipv6: bool,
) -> Result<(), NetworkError> {
    let _ = cleanup_nftables(nft).await;
    run_cmd_with_input(
        nft,
        &[String::from("-f"), String::from("-")],
        &nft_script(tun_name, fwmark, ipv4, ipv6),
    )
    .await
}

async fn cleanup_nftables(nft: &Path) -> Result<(), NetworkError> {
    ignore_missing_firewall_state(
        run_cmd(
            nft,
            &[
                String::from("delete"),
                String::from("table"),
                String::from("inet"),
                NFT_TABLE_NAME.to_string(),
            ],
        )
        .await,
    )
}

fn nft_script(tun_name: &str, fwmark: u32, ipv4: bool, ipv6: bool) -> String {
    let escaped_tun_name = nft_string(tun_name);
    let mut script = format!(
        "add table inet {NFT_TABLE_NAME}\n\
         add chain inet {NFT_TABLE_NAME} output {{ type filter hook output priority 0; policy accept; }}\n\
         add rule inet {NFT_TABLE_NAME} output oifname \"lo\" accept\n\
         add rule inet {NFT_TABLE_NAME} output oifname \"{escaped_tun_name}\" accept\n\
         add rule inet {NFT_TABLE_NAME} output meta mark 0x{fwmark:x} accept\n"
    );

    if ipv4 {
        script.push_str(&format!(
            "add rule inet {NFT_TABLE_NAME} output meta nfproto ipv4 drop\n"
        ));
    }
    if ipv6 {
        script.push_str(&format!(
            "add rule inet {NFT_TABLE_NAME} output meta nfproto ipv6 drop\n"
        ));
    }

    script
}

async fn apply_iptables(
    tun_name: &str,
    fwmark: u32,
    ipv4: bool,
    ipv6: bool,
) -> Result<(), NetworkError> {
    apply_iptables_with_resolver(tun_name, fwmark, ipv4, ipv6, resolve_command).await
}

async fn apply_iptables_with_resolver<F>(
    tun_name: &str,
    fwmark: u32,
    ipv4: bool,
    ipv6: bool,
    resolve: F,
) -> Result<(), NetworkError>
where
    F: Fn(&str) -> Option<PathBuf>,
{
    let mut applied = false;
    let mut iptables_path = None;

    if ipv4 {
        let iptables = resolve("iptables").ok_or_else(|| {
            NetworkError::KillSwitchUnavailable("iptables command not found".to_string())
        })?;
        apply_iptables_family(&iptables, tun_name, fwmark).await?;
        iptables_path = Some(iptables);
        applied = true;
    }

    if ipv6 {
        let ip6tables = match resolve("ip6tables") {
            Some(path) => path,
            None => {
                if let Some(iptables) = iptables_path.as_deref() {
                    cleanup_iptables_family(iptables).await?;
                }
                return Err(NetworkError::KillSwitchUnavailable(
                    "ip6tables command not found".to_string(),
                ));
            }
        };
        if let Err(err) = apply_iptables_family(&ip6tables, tun_name, fwmark).await {
            if let Some(iptables) = iptables_path.as_deref() {
                let _ = cleanup_iptables_family(iptables).await;
            }
            return Err(err);
        }
        applied = true;
    }

    if applied {
        Ok(())
    } else {
        Err(NetworkError::KillSwitchUnavailable(
            "no enabled iptables address family".to_string(),
        ))
    }
}

async fn apply_iptables_family(
    program: &Path,
    tun_name: &str,
    fwmark: u32,
) -> Result<(), NetworkError> {
    let _ = cleanup_iptables_family(program).await;

    let result = apply_iptables_family_inner(program, tun_name, fwmark).await;
    if result.is_err() {
        let _ = cleanup_iptables_family(program).await;
    }
    result
}

async fn apply_iptables_family_inner(
    program: &Path,
    tun_name: &str,
    fwmark: u32,
) -> Result<(), NetworkError> {
    run_cmd(program, &iptables_args(["-w", "-N", IPTABLES_CHAIN_NAME])).await?;
    run_cmd(
        program,
        &iptables_args(["-w", "-A", IPTABLES_CHAIN_NAME, "-o", "lo", "-j", "RETURN"]),
    )
    .await?;
    run_cmd(
        program,
        &iptables_args([
            "-w",
            "-A",
            IPTABLES_CHAIN_NAME,
            "-o",
            tun_name,
            "-j",
            "RETURN",
        ]),
    )
    .await?;
    run_cmd(
        program,
        &iptables_args([
            "-w",
            "-A",
            IPTABLES_CHAIN_NAME,
            "-m",
            "mark",
            "--mark",
            &format!("0x{fwmark:x}/0xffffffff"),
            "-j",
            "RETURN",
        ]),
    )
    .await?;
    run_cmd(
        program,
        &iptables_args(["-w", "-A", IPTABLES_CHAIN_NAME, "-j", "REJECT"]),
    )
    .await?;
    run_cmd(
        program,
        &iptables_args(["-w", "-I", "OUTPUT", "1", "-j", IPTABLES_CHAIN_NAME]),
    )
    .await
}

async fn cleanup_iptables(ipv4: bool, ipv6: bool) -> Result<(), NetworkError> {
    if ipv4 {
        let iptables = resolve_command("iptables").ok_or_else(|| {
            NetworkError::KillSwitchUnavailable("iptables command not found".to_string())
        })?;
        cleanup_iptables_family(&iptables).await?;
    }
    if ipv6 {
        let ip6tables = resolve_command("ip6tables").ok_or_else(|| {
            NetworkError::KillSwitchUnavailable("ip6tables command not found".to_string())
        })?;
        cleanup_iptables_family(&ip6tables).await?;
    }
    Ok(())
}

async fn cleanup_iptables_family(program: &Path) -> Result<(), NetworkError> {
    for _ in 0..8 {
        match run_cmd(
            program,
            &iptables_args(["-w", "-D", "OUTPUT", "-j", IPTABLES_CHAIN_NAME]),
        )
        .await
        {
            Ok(()) => {}
            Err(err) if is_missing_firewall_state(&err) => break,
            Err(err) => return Err(err),
        }
    }

    ignore_missing_firewall_state(
        run_cmd(program, &iptables_args(["-w", "-F", IPTABLES_CHAIN_NAME])).await,
    )?;
    ignore_missing_firewall_state(
        run_cmd(program, &iptables_args(["-w", "-X", IPTABLES_CHAIN_NAME])).await,
    )?;
    Ok(())
}

fn ignore_missing_firewall_state(result: Result<(), NetworkError>) -> Result<(), NetworkError> {
    match result {
        Ok(()) => Ok(()),
        Err(err) if is_missing_firewall_state(&err) => Ok(()),
        Err(err) => Err(err),
    }
}

fn is_missing_firewall_state(error: &NetworkError) -> bool {
    let NetworkError::CommandFailed { stderr, .. } = error else {
        return false;
    };
    let normalized = stderr.to_ascii_lowercase();
    normalized.contains("no such file")
        || normalized.contains("no such table")
        || normalized.contains("no such chain")
        || normalized.contains("does a matching rule exist")
        || normalized.contains("no chain/target/match")
}

fn resolve_command(program: &str) -> Option<PathBuf> {
    if program.contains('/') {
        let path = PathBuf::from(program);
        return path.is_file().then_some(path);
    }

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

async fn run_cmd(program: &Path, args: &[String]) -> Result<(), NetworkError> {
    let output = Command::new(program).args(args).output().await?;
    if output.status.success() {
        return Ok(());
    }

    Err(command_failed(
        program,
        args,
        output.status.code(),
        &output.stderr,
    ))
}

async fn run_cmd_with_input(
    program: &Path,
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

    Err(command_failed(
        program,
        args,
        output.status.code(),
        &output.stderr,
    ))
}

fn command_failed(
    program: &Path,
    args: &[String],
    status: Option<i32>,
    stderr: &[u8],
) -> NetworkError {
    NetworkError::CommandFailed {
        command: format_command(program, args),
        status,
        stderr: String::from_utf8_lossy(stderr).trim().to_string(),
    }
}

fn format_command(program: &Path, args: &[String]) -> String {
    let mut command = program.display().to_string();
    for arg in args {
        command.push(' ');
        command.push_str(arg);
    }
    command
}

fn iptables_args<const N: usize>(args: [&str; N]) -> Vec<String> {
    args.into_iter().map(str::to_string).collect()
}

fn nft_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir(test_name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("r-wg-{test_name}-{}-{nanos}", std::process::id()))
    }

    fn shell_quote_path(path: &Path) -> String {
        format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
    }

    fn write_fake_firewall_command(path: &Path, log_path: &Path) {
        fs::write(
            path,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> {}\nexit 0\n",
                shell_quote_path(log_path)
            ),
        )
        .unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn nft_script_limits_enabled_families() {
        let script = nft_script("tun0", 0x5257, true, false);

        assert!(script.contains("meta nfproto ipv4 drop"));
        assert!(!script.contains("meta nfproto ipv6 drop"));
        assert!(script.contains("meta mark 0x5257 accept"));
    }

    #[test]
    fn nft_script_escapes_interface_name() {
        let script = nft_script("wg\\\"0", 0x12, true, true);

        assert!(script.contains("oifname \"wg\\\\\\\"0\" accept"));
    }

    #[test]
    fn iptables_apply_cleans_ipv4_when_ip6tables_is_missing() {
        let temp_dir = unique_temp_dir("iptables-missing-ip6tables");
        fs::create_dir_all(&temp_dir).unwrap();
        let log_path = temp_dir.join("commands.log");
        let iptables_path = temp_dir.join("iptables");
        write_fake_firewall_command(&iptables_path, &log_path);

        let runtime = tokio::runtime::Runtime::new().unwrap();
        let error = runtime
            .block_on(apply_iptables_with_resolver(
                "tun0",
                0x5257,
                true,
                true,
                |program| match program {
                    "iptables" => Some(iptables_path.clone()),
                    "ip6tables" => None,
                    _ => None,
                },
            ))
            .unwrap_err();

        assert!(matches!(
            error,
            NetworkError::KillSwitchUnavailable(message)
                if message == "ip6tables command not found"
        ));

        let commands = fs::read_to_string(&log_path).unwrap();
        assert!(commands.contains("-w -I OUTPUT 1 -j R_WG_KILLSWITCH"));
        assert!(commands.contains("-w -D OUTPUT -j R_WG_KILLSWITCH"));
        assert!(commands.contains("-w -F R_WG_KILLSWITCH"));
        assert!(commands.contains("-w -X R_WG_KILLSWITCH"));

        let _ = fs::remove_dir_all(temp_dir);
    }
}
