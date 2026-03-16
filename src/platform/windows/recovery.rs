use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::backend::wg::config::InterfaceAddress;

use super::adapter::{AdapterInfo, AdapterSnapshot};
use super::addresses::delete_unicast_address;
use super::dns::{cleanup_dns, DnsState, DnsStateSnapshot};
use super::firewall::{cleanup_dns_guard, cleanup_stale_dns_guard_rules, DnsGuardStateSnapshot};
use super::metrics::{restore_interface_metric, InterfaceMetricSnapshot, InterfaceMetricState};
use super::nrpt::{cleanup_nrpt_guard, cleanup_stale_nrpt_rules, NrptState, NrptStateSnapshot};
use super::routes::{delete_route, RouteEntry, RouteSnapshot};
use super::NetworkError;

const RECOVERY_JOURNAL_FILE: &str = "windows-recovery.json";
const RECOVERY_JOURNAL_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum RecoveryPhase {
    Applying,
    Running,
    Stopping,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AddressSnapshot {
    addr: std::net::IpAddr,
    cidr: u8,
}

impl From<&InterfaceAddress> for AddressSnapshot {
    fn from(address: &InterfaceAddress) -> Self {
        Self {
            addr: address.addr,
            cidr: address.cidr,
        }
    }
}

impl AddressSnapshot {
    fn to_interface_address(&self) -> InterfaceAddress {
        InterfaceAddress {
            addr: self.addr,
            cidr: self.cidr,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RecoveryJournal {
    version: u32,
    phase: RecoveryPhase,
    tun_name: String,
    adapter: AdapterSnapshot,
    addresses: Vec<AddressSnapshot>,
    routes: Vec<RouteSnapshot>,
    bypass_routes: Vec<RouteSnapshot>,
    iface_metrics: Vec<InterfaceMetricSnapshot>,
    dns: Option<DnsStateSnapshot>,
    nrpt: Option<NrptStateSnapshot>,
    dns_guard: Option<DnsGuardStateSnapshot>,
}

pub(super) struct RecoveryGuard {
    path: PathBuf,
    journal: RecoveryJournal,
}

impl RecoveryGuard {
    pub(super) fn begin(tun_name: &str, adapter: AdapterInfo) -> Result<Self, NetworkError> {
        let path = recovery_journal_path();
        let mut guard = Self {
            path,
            journal: RecoveryJournal {
                version: RECOVERY_JOURNAL_VERSION,
                phase: RecoveryPhase::Applying,
                tun_name: tun_name.to_string(),
                adapter: adapter.into(),
                addresses: Vec::new(),
                routes: Vec::new(),
                bypass_routes: Vec::new(),
                iface_metrics: Vec::new(),
                dns: None,
                nrpt: None,
                dns_guard: None,
            },
        };
        guard.persist()?;
        Ok(guard)
    }

    pub(super) fn record_address(
        &mut self,
        address: &InterfaceAddress,
    ) -> Result<(), NetworkError> {
        self.journal.addresses.push(AddressSnapshot::from(address));
        self.persist()
    }

    pub(super) fn record_route(&mut self, route: &RouteEntry) -> Result<(), NetworkError> {
        self.journal.routes.push(RouteSnapshot::from(route));
        self.persist()
    }

    pub(super) fn record_bypass_route(&mut self, route: &RouteEntry) -> Result<(), NetworkError> {
        self.journal.bypass_routes.push(RouteSnapshot::from(route));
        self.persist()
    }

    pub(super) fn record_metric(
        &mut self,
        metric: InterfaceMetricState,
    ) -> Result<(), NetworkError> {
        self.journal.iface_metrics.push(metric.into());
        self.persist()
    }

    pub(super) fn record_dns(&mut self, dns: &DnsState) -> Result<(), NetworkError> {
        self.journal.dns = Some(dns.snapshot());
        self.persist()
    }

    pub(super) fn record_nrpt(&mut self, nrpt: &NrptState) -> Result<(), NetworkError> {
        self.journal.nrpt = Some(nrpt.snapshot());
        self.persist()
    }

    pub(super) fn record_dns_guard(
        &mut self,
        dns_guard: &super::firewall::DnsGuardState,
    ) -> Result<(), NetworkError> {
        self.journal.dns_guard = Some(dns_guard.snapshot());
        self.persist()
    }

    pub(super) fn mark_running(&mut self) -> Result<(), NetworkError> {
        self.journal.phase = RecoveryPhase::Running;
        self.persist()
    }

    pub(super) fn mark_stopping(&mut self) -> Result<(), NetworkError> {
        self.journal.phase = RecoveryPhase::Stopping;
        self.persist()
    }

    pub(super) fn clear(self) -> Result<(), NetworkError> {
        clear_recovery_journal_at(self.path.as_path())
    }

    fn persist(&mut self) -> Result<(), NetworkError> {
        write_recovery_journal(self.path.as_path(), &self.journal)
    }
}

pub(super) fn attempt_startup_repair() -> Result<(), NetworkError> {
    let _ = cleanup_stale_nrpt_rules();
    let _ = cleanup_stale_dns_guard_rules();

    let Some(journal) = load_recovery_journal()? else {
        return Ok(());
    };

    repair_journal(&journal)?;
    clear_recovery_journal_at(recovery_journal_path().as_path())
}

fn repair_journal(journal: &RecoveryJournal) -> Result<(), NetworkError> {
    let adapter = journal
        .adapter
        .to_adapter_info()
        .map_err(NetworkError::Io)?;

    for route in journal.bypass_routes.iter().rev() {
        let _ = delete_route(&route.to_route_entry());
    }
    for route in journal.routes.iter().rev() {
        let _ = delete_route(&route.to_route_entry());
    }
    for address in &journal.addresses {
        let _ = delete_unicast_address(adapter, &address.to_interface_address());
    }
    for metric in journal.iface_metrics.iter().rev() {
        let _ = restore_interface_metric(adapter, metric.to_state());
    }

    if let Some(dns) = &journal.dns {
        cleanup_dns(dns.to_state().map_err(NetworkError::Io)?).map_err(|err| {
            NetworkError::UnsafeRouting(format!("startup DNS repair failed: {err}"))
        })?;
    }
    if let Some(nrpt) = &journal.nrpt {
        cleanup_nrpt_guard(nrpt.to_state()).map_err(|err| {
            NetworkError::UnsafeRouting(format!("startup NRPT repair failed: {err}"))
        })?;
    }
    if let Some(dns_guard) = &journal.dns_guard {
        cleanup_dns_guard(dns_guard.to_state()).map_err(|err| {
            NetworkError::UnsafeRouting(format!("startup DNS guard repair failed: {err}"))
        })?;
    }

    Ok(())
}

fn load_recovery_journal() -> Result<Option<RecoveryJournal>, NetworkError> {
    let path = recovery_journal_path();
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(NetworkError::Io(err)),
    };
    let journal = serde_json::from_str(&text).map_err(|err| {
        NetworkError::Io(io::Error::new(io::ErrorKind::InvalidData, err.to_string()))
    })?;
    Ok(Some(journal))
}

fn write_recovery_journal(path: &Path, journal: &RecoveryJournal) -> Result<(), NetworkError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_vec(journal).map_err(|err| {
        NetworkError::Io(io::Error::new(io::ErrorKind::InvalidData, err.to_string()))
    })?;
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, json)?;
    fs::rename(tmp_path, path)?;
    Ok(())
}

fn clear_recovery_journal_at(path: &Path) -> Result<(), NetworkError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(NetworkError::Io(err)),
    }
}

fn recovery_journal_path() -> PathBuf {
    let base = env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    base.join("r-wg").join(RECOVERY_JOURNAL_FILE)
}
