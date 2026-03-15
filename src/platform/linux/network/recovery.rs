//! Linux 崩溃恢复日志与启动期修复。
//!
//! 设计目标：
//! - 在修改系统网络状态前，把“足够回滚”的信息持久化到 root-owned journal；
//! - clean stop 成功后删除 journal，异常退出则保留，供下次 service 启动或 boot-time repair 使用；
//! - 启动期先做无状态清理（stale route / policy rule），再做有状态回滚（DNS）。
//!
//! 当前 journal 主要覆盖：
//! - TUN 名称（用于 stale route 清理与 DNS 回滚）；
//! - phase（applying / running）；
//! - DNS 回滚所需快照（NM / resolv.conf / 其它后端状态）。
use std::env;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::dns::{cleanup_dns, DnsState};
use super::netlink::{link_index, netlink_handle};
use super::policy::{cleanup_policy_rules_once, cleanup_stale_default_routes_once};
use super::NetworkError;

const RECOVERY_JOURNAL_FILE: &str = "recovery.json";

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum RecoveryPhase {
    Applying,
    Running,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RecoveryJournal {
    tun_name: String,
    phase: RecoveryPhase,
    dns: Option<DnsState>,
}

pub(super) fn write_applying_journal(tun_name: &str) -> Result<(), NetworkError> {
    write_recovery_journal(&RecoveryJournal {
        tun_name: tun_name.to_string(),
        phase: RecoveryPhase::Applying,
        dns: None,
    })
}

pub(super) fn write_running_journal(
    state: &super::AppliedNetworkState,
) -> Result<(), NetworkError> {
    write_recovery_journal(&RecoveryJournal {
        tun_name: state.tun_name.clone(),
        phase: RecoveryPhase::Running,
        dns: state.dns.clone(),
    })
}

pub(super) fn clear_recovery_journal() -> Result<(), NetworkError> {
    let path = recovery_journal_path();
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(NetworkError::Io(err)),
    }
}

pub(super) fn attempt_startup_repair_sync() -> Result<(), NetworkError> {
    let runtime = tokio::runtime::Runtime::new().map_err(NetworkError::Io)?;
    runtime.block_on(async { attempt_startup_repair().await })
}

async fn attempt_startup_repair() -> Result<(), NetworkError> {
    let Some(journal) = load_recovery_journal()? else {
        return Ok(());
    };

    let netlink = netlink_handle()?;
    let handle = netlink.handle();

    let stateless_result = async {
        if let Ok(link_index) = link_index(handle, &journal.tun_name).await {
            let _ = cleanup_stale_default_routes_once(handle, &journal.tun_name, link_index).await;
        }
        let _ = cleanup_policy_rules_once(handle).await;
        Ok::<_, NetworkError>(())
    }
    .await;

    netlink.shutdown().await;
    stateless_result?;

    if let Some(dns) = journal.dns {
        cleanup_dns(&journal.tun_name, dns).await?;
    }

    clear_recovery_journal()
}

fn load_recovery_journal() -> Result<Option<RecoveryJournal>, NetworkError> {
    let path = recovery_journal_path();
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(NetworkError::Io(err)),
    };
    let journal = serde_json::from_str(&text).map_err(|err| {
        NetworkError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, err))
    })?;
    Ok(Some(journal))
}

fn write_recovery_journal(journal: &RecoveryJournal) -> Result<(), NetworkError> {
    let path = recovery_journal_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string(journal).map_err(|err| {
        NetworkError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, err))
    })?;
    fs::write(path, json)?;
    Ok(())
}

fn recovery_journal_path() -> PathBuf {
    if let Some(dir) = env::var_os("STATE_DIRECTORY") {
        return PathBuf::from(dir).join(RECOVERY_JOURNAL_FILE);
    }
    PathBuf::from("/var/lib/r-wg").join(RECOVERY_JOURNAL_FILE)
}
