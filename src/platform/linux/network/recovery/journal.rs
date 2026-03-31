use std::env;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::super::dns::DnsState;
use super::super::policy::PolicyRoutingState;
use super::super::NetworkError;
use super::snapshot::{policy_snapshot, route_snapshots, route_snapshots_from_ops};
use crate::core::route_plan::{RoutePlan, RoutePlanRouteOp};

const RECOVERY_JOURNAL_FILE: &str = "recovery.json";

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) enum RecoveryPhase {
    Applying,
    Running,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RecoveryJournal {
    pub(crate) tun_name: String,
    pub(crate) phase: RecoveryPhase,
    pub(crate) routes: Vec<super::RecoveryRouteSnapshot>,
    pub(crate) policy: Option<super::RecoveryPolicySnapshot>,
    pub(crate) dns: Option<DnsState>,
}

pub(crate) fn write_applying_journal(
    tun_name: &str,
    route_plan: &RoutePlan,
    policy: Option<&PolicyRoutingState>,
) -> Result<(), NetworkError> {
    write_recovery_journal(&RecoveryJournal {
        tun_name: tun_name.to_string(),
        phase: RecoveryPhase::Applying,
        routes: route_snapshots(route_plan),
        policy: policy.map(policy_snapshot),
        dns: None,
    })
}

pub(crate) fn write_running_journal(
    tun_name: &str,
    routes: &[RoutePlanRouteOp],
    policy: Option<&PolicyRoutingState>,
    dns: Option<&DnsState>,
) -> Result<(), NetworkError> {
    write_recovery_journal(&RecoveryJournal {
        tun_name: tun_name.to_string(),
        phase: RecoveryPhase::Running,
        routes: route_snapshots_from_ops(routes),
        policy: policy.map(policy_snapshot),
        dns: dns.cloned(),
    })
}

pub(crate) fn load_recovery_journal() -> Result<Option<RecoveryJournal>, NetworkError> {
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

pub(crate) fn clear_recovery_journal() -> Result<(), NetworkError> {
    let path = recovery_journal_path();
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(NetworkError::Io(err)),
    }
}

pub(crate) fn journal_requires_exact_cleanup(journal: &RecoveryJournal) -> bool {
    !journal.routes.is_empty() || journal.policy.is_some()
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
