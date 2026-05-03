use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::process::Command;

use super::NetworkError;
use crate::log::events::net as log_net;

const NFT_TABLE_NAME: &str = "r_wg_killswitch";
const IPTABLES_CHAIN_NAME: &str = "R_WG_KILLSWITCH";
const QUANTUM_NFT_TABLE_NAME: &str = "r_wg_quantum_start";
const QUANTUM_IPTABLES_CHAIN_NAME: &str = "R_WG_QUANTUM_START";
const QUANTUM_GUARD_JOURNAL_FILE: &str = "quantum-guard.json";
const CONFIG_SERVICE_GATEWAY: &str = "10.64.0.1";
const CONFIG_SERVICE_PORT: &str = "1337";

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

#[derive(Debug)]
pub struct QuantumNegotiationGuardState {
    backend: KillSwitchBackend,
    tun_name: String,
    block_ipv6: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct QuantumNegotiationGuardJournal {
    backend: KillSwitchBackend,
    tun_name: String,
    block_ipv6: bool,
}

pub(super) async fn apply_quantum_negotiation_guard(
    tun_name: &str,
    block_ipv6: bool,
) -> Result<QuantumNegotiationGuardState, NetworkError> {
    let mut nft_error = None;
    if let Some(nft) = resolve_command("nft") {
        match apply_quantum_nftables(&nft, tun_name).await {
            Ok(()) => {
                let state = QuantumNegotiationGuardState {
                    backend: KillSwitchBackend::Nftables,
                    tun_name: tun_name.to_string(),
                    block_ipv6,
                };
                if let Err(err) = write_quantum_guard_journal(&state) {
                    let _ = state.cleanup_firewall().await;
                    return Err(err);
                }
                return Ok(state);
            }
            Err(err) => {
                let _ = cleanup_quantum_nftables(&nft).await;
                nft_error = Some(err);
            }
        }
    }

    match apply_quantum_iptables(tun_name, block_ipv6).await {
        Ok(()) => {
            let state = QuantumNegotiationGuardState {
                backend: KillSwitchBackend::Iptables,
                tun_name: tun_name.to_string(),
                block_ipv6,
            };
            if let Err(err) = write_quantum_guard_journal(&state) {
                let _ = state.cleanup_firewall().await;
                return Err(err);
            }
            Ok(state)
        }
        Err(NetworkError::KillSwitchUnavailable(_)) => match nft_error {
            Some(err) => Err(err),
            None => Err(NetworkError::KillSwitchUnavailable(
                "nftables and iptables backends are unavailable for quantum negotiation guard"
                    .to_string(),
            )),
        },
        Err(err) => Err(err),
    }
}

impl QuantumNegotiationGuardState {
    pub(super) async fn cleanup(self) -> Result<(), NetworkError> {
        self.cleanup_firewall().await?;
        clear_quantum_guard_journal()
    }

    async fn cleanup_firewall(&self) -> Result<(), NetworkError> {
        match self.backend {
            KillSwitchBackend::Nftables => {
                let nft = resolve_command("nft").ok_or_else(|| {
                    NetworkError::KillSwitchUnavailable("nft command not found".to_string())
                })?;
                cleanup_quantum_nftables(&nft).await
            }
            KillSwitchBackend::Iptables => cleanup_quantum_iptables(self.block_ipv6).await,
        }
    }
}

pub(super) async fn cleanup_stale_quantum_negotiation_guard() -> Result<(), NetworkError> {
    match load_quantum_guard_journal()? {
        Some(journal) => {
            let state = QuantumNegotiationGuardState {
                backend: journal.backend,
                tun_name: journal.tun_name,
                block_ipv6: journal.block_ipv6,
            };
            state.cleanup_firewall().await?;
            clear_quantum_guard_journal()
        }
        None => {
            cleanup_quantum_firewall_best_effort(true).await?;
            clear_quantum_guard_journal()
        }
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

async fn apply_quantum_nftables(nft: &Path, tun_name: &str) -> Result<(), NetworkError> {
    let _ = cleanup_quantum_nftables(nft).await;
    run_cmd_with_input(
        nft,
        &[String::from("-f"), String::from("-")],
        &quantum_nft_script(tun_name),
    )
    .await
}

async fn cleanup_quantum_nftables(nft: &Path) -> Result<(), NetworkError> {
    ignore_missing_firewall_state(
        run_cmd(
            nft,
            &[
                String::from("delete"),
                String::from("table"),
                String::from("inet"),
                QUANTUM_NFT_TABLE_NAME.to_string(),
            ],
        )
        .await,
    )
}

fn quantum_nft_script(tun_name: &str) -> String {
    let escaped_tun_name = nft_string(tun_name);
    format!(
        "add table inet {QUANTUM_NFT_TABLE_NAME}\n\
         add chain inet {QUANTUM_NFT_TABLE_NAME} output {{ type filter hook output priority -10; policy accept; }}\n\
         add rule inet {QUANTUM_NFT_TABLE_NAME} output oifname \"{escaped_tun_name}\" ip daddr {CONFIG_SERVICE_GATEWAY} tcp dport {CONFIG_SERVICE_PORT} accept\n\
         add rule inet {QUANTUM_NFT_TABLE_NAME} output oifname \"{escaped_tun_name}\" reject\n"
    )
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

async fn apply_quantum_iptables(tun_name: &str, block_ipv6: bool) -> Result<(), NetworkError> {
    let iptables = resolve_command("iptables").ok_or_else(|| {
        NetworkError::KillSwitchUnavailable("iptables command not found".to_string())
    })?;
    apply_quantum_iptables_v4(&iptables, tun_name).await?;

    if block_ipv6 {
        let ip6tables = match resolve_command("ip6tables") {
            Some(path) => path,
            None => {
                let _ = cleanup_quantum_iptables_family(&iptables).await;
                return Err(NetworkError::KillSwitchUnavailable(
                    "ip6tables command not found".to_string(),
                ));
            }
        };
        if let Err(err) = apply_quantum_iptables_v6(&ip6tables, tun_name).await {
            let _ = cleanup_quantum_iptables_family(&iptables).await;
            return Err(err);
        }
    }

    Ok(())
}

async fn apply_quantum_iptables_v4(program: &Path, tun_name: &str) -> Result<(), NetworkError> {
    let _ = cleanup_quantum_iptables_family(program).await;
    let result = async {
        run_cmd(
            program,
            &iptables_args(["-w", "-N", QUANTUM_IPTABLES_CHAIN_NAME]),
        )
        .await?;
        run_cmd(
            program,
            &iptables_args([
                "-w",
                "-A",
                QUANTUM_IPTABLES_CHAIN_NAME,
                "-o",
                tun_name,
                "-p",
                "tcp",
                "-d",
                CONFIG_SERVICE_GATEWAY,
                "--dport",
                CONFIG_SERVICE_PORT,
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
                QUANTUM_IPTABLES_CHAIN_NAME,
                "-o",
                tun_name,
                "-j",
                "REJECT",
            ]),
        )
        .await?;
        run_cmd(
            program,
            &iptables_args(["-w", "-I", "OUTPUT", "1", "-j", QUANTUM_IPTABLES_CHAIN_NAME]),
        )
        .await
    }
    .await;
    if result.is_err() {
        let _ = cleanup_quantum_iptables_family(program).await;
    }
    result
}

async fn apply_quantum_iptables_v6(program: &Path, tun_name: &str) -> Result<(), NetworkError> {
    let _ = cleanup_quantum_iptables_family(program).await;
    let result = async {
        run_cmd(
            program,
            &iptables_args(["-w", "-N", QUANTUM_IPTABLES_CHAIN_NAME]),
        )
        .await?;
        run_cmd(
            program,
            &iptables_args([
                "-w",
                "-A",
                QUANTUM_IPTABLES_CHAIN_NAME,
                "-o",
                tun_name,
                "-j",
                "REJECT",
            ]),
        )
        .await?;
        run_cmd(
            program,
            &iptables_args(["-w", "-I", "OUTPUT", "1", "-j", QUANTUM_IPTABLES_CHAIN_NAME]),
        )
        .await
    }
    .await;
    if result.is_err() {
        let _ = cleanup_quantum_iptables_family(program).await;
    }
    result
}

async fn cleanup_quantum_iptables(block_ipv6: bool) -> Result<(), NetworkError> {
    let iptables = resolve_command("iptables").ok_or_else(|| {
        NetworkError::KillSwitchUnavailable("iptables command not found".to_string())
    })?;
    cleanup_quantum_iptables_family(&iptables).await?;

    if block_ipv6 {
        let ip6tables = resolve_command("ip6tables").ok_or_else(|| {
            NetworkError::KillSwitchUnavailable("ip6tables command not found".to_string())
        })?;
        cleanup_quantum_iptables_family(&ip6tables).await?;
    }
    Ok(())
}

async fn cleanup_quantum_firewall_best_effort(block_ipv6: bool) -> Result<(), NetworkError> {
    let mut first_error = None;

    if let Some(nft) = resolve_command("nft") {
        if let Err(err) = cleanup_quantum_nftables(&nft).await {
            first_error.get_or_insert(err);
        }
    }

    if let Some(iptables) = resolve_command("iptables") {
        if let Err(err) = cleanup_quantum_iptables_family(&iptables).await {
            first_error.get_or_insert(err);
        }
    }

    if block_ipv6 {
        if let Some(ip6tables) = resolve_command("ip6tables") {
            if let Err(err) = cleanup_quantum_iptables_family(&ip6tables).await {
                first_error.get_or_insert(err);
            }
        }
    }

    match first_error {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

async fn cleanup_quantum_iptables_family(program: &Path) -> Result<(), NetworkError> {
    for _ in 0..8 {
        match run_cmd(
            program,
            &iptables_args(["-w", "-D", "OUTPUT", "-j", QUANTUM_IPTABLES_CHAIN_NAME]),
        )
        .await
        {
            Ok(()) => {}
            Err(err) if is_missing_firewall_state(&err) => break,
            Err(err) => return Err(err),
        }
    }

    ignore_missing_firewall_state(
        run_cmd(
            program,
            &iptables_args(["-w", "-F", QUANTUM_IPTABLES_CHAIN_NAME]),
        )
        .await,
    )?;
    ignore_missing_firewall_state(
        run_cmd(
            program,
            &iptables_args(["-w", "-X", QUANTUM_IPTABLES_CHAIN_NAME]),
        )
        .await,
    )?;
    Ok(())
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

fn write_quantum_guard_journal(state: &QuantumNegotiationGuardState) -> Result<(), NetworkError> {
    let journal = QuantumNegotiationGuardJournal {
        backend: state.backend,
        tun_name: state.tun_name.clone(),
        block_ipv6: state.block_ipv6,
    };
    let json = serde_json::to_vec(&journal).map_err(|err| {
        NetworkError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, err))
    })?;
    write_atomic_json(quantum_guard_journal_path(), &json)
}

fn load_quantum_guard_journal() -> Result<Option<QuantumNegotiationGuardJournal>, NetworkError> {
    let path = quantum_guard_journal_path();
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(NetworkError::Io(err)),
    };
    match serde_json::from_str(&text) {
        Ok(journal) => Ok(Some(journal)),
        Err(err) => {
            quarantine_quantum_guard_journal(&path)?;
            tracing::warn!("quarantined corrupt quantum guard journal: {err}");
            Ok(None)
        }
    }
}

fn clear_quantum_guard_journal() -> Result<(), NetworkError> {
    let path = quantum_guard_journal_path();
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(NetworkError::Io(err)),
    }
}

fn quarantine_quantum_guard_journal(path: &Path) -> Result<(), NetworkError> {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let quarantine_path =
        path.with_file_name(format!("{QUANTUM_GUARD_JOURNAL_FILE}.corrupt.{suffix}"));
    fs::rename(path, quarantine_path).map_err(NetworkError::Io)
}

fn write_atomic_json(path: PathBuf, json: &[u8]) -> Result<(), NetworkError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp_path = path.with_file_name(format!(
        ".{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(QUANTUM_GUARD_JOURNAL_FILE)
    ));
    {
        let mut file = fs::File::create(&tmp_path)?;
        file.write_all(json)?;
        file.sync_all()?;
    }
    fs::rename(&tmp_path, &path)?;
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

fn quantum_guard_journal_path() -> PathBuf {
    env::var_os("STATE_DIRECTORY")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/var/lib/r-wg"))
        .join(QUANTUM_GUARD_JOURNAL_FILE)
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
    fn quantum_nft_script_allows_only_config_service_on_tunnel() {
        let script = quantum_nft_script("wg0");

        assert!(script.contains("hook output priority -10"));
        assert!(script.contains("oifname \"wg0\" ip daddr 10.64.0.1 tcp dport 1337 accept"));
        assert!(script.contains("oifname \"wg0\" reject"));
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
