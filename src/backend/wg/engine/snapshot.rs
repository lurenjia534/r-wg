#[cfg(target_os = "linux")]
use super::super::linux_kernel;
use super::super::relay_inventory;
use super::state::{ActiveWireGuardBackend, DeviceSlot, EngineState};
use super::{
    DaitaStats, EngineError, EngineRuntimeSnapshot, EngineStats, EngineStatus, PeerStats,
    RelayInventoryStatusSnapshot,
};

impl EngineState {
    /// 查询状态。
    pub(super) fn status(&self) -> EngineStatus {
        if self.running {
            EngineStatus::Running
        } else {
            EngineStatus::Stopped
        }
    }

    /// 获取 gotatun 或 kernel backend 运行时统计信息。
    pub(super) async fn stats(&self) -> Result<EngineStats, EngineError> {
        if !self.running {
            return Err(EngineError::NotRunning);
        }
        let Some(active_backend) = self.active_backend.as_ref() else {
            return Err(EngineError::NotRunning);
        };
        match active_backend {
            ActiveWireGuardBackend::Userspace(slot) => read_userspace_stats(slot).await,
            #[cfg(target_os = "linux")]
            ActiveWireGuardBackend::LinuxKernel(kernel_device) => {
                linux_kernel::read_kernel_stats(kernel_device.tun_name())
                    .await
                    .map_err(|error| EngineError::KernelWireGuard(error.to_string()))
            }
        }
    }

    pub(super) fn apply_report(&self) -> Option<crate::core::route_plan::RouteApplyReport> {
        self.route_apply_report.clone()
    }

    pub(super) fn runtime_snapshot(&self) -> EngineRuntimeSnapshot {
        EngineRuntimeSnapshot {
            status: self.status(),
            active_backend: self
                .active_backend
                .as_ref()
                .map(ActiveWireGuardBackend::status),
            apply_report: self.apply_report(),
            quantum_protected: self.running && self.quantum_active,
            last_quantum_failure: self.last_quantum_failure,
            daita_active: self.running && self.daita_active,
            last_daita_failure: self.last_daita_failure,
        }
    }

    pub(super) fn relay_inventory_status(
        &self,
    ) -> Result<RelayInventoryStatusSnapshot, EngineError> {
        relay_inventory::status_snapshot()
            .map(map_relay_inventory_status_snapshot)
            .map_err(|error| EngineError::Remote(error.to_string()))
    }
}

async fn read_userspace_stats(slot: &DeviceSlot) -> Result<EngineStats, EngineError> {
    let peers = slot
        .device
        .read(async |device| device.peers().await)
        .await
        .into_iter()
        .map(|peer| PeerStats {
            public_key: peer.peer.public_key.to_bytes(),
            endpoint: peer.peer.endpoint,
            last_handshake: peer.stats.last_handshake,
            rx_bytes: peer.stats.rx_bytes as u64,
            tx_bytes: peer.stats.tx_bytes as u64,
            daita: peer.stats.daita.map(|stats| DaitaStats {
                tx_padding_bytes: stats.tx_padding_bytes as u64,
                rx_padding_bytes: stats.rx_padding_bytes as u64,
                tx_decoy_packet_bytes: stats.tx_decoy_packet_bytes as u64,
                rx_decoy_packet_bytes: stats.rx_decoy_packet_bytes as u64,
            }),
        })
        .collect();

    Ok(EngineStats { peers })
}

pub(super) fn map_relay_inventory_status_snapshot(
    snapshot: relay_inventory::RelayInventoryStatusSnapshot,
) -> RelayInventoryStatusSnapshot {
    RelayInventoryStatusSnapshot {
        cache_path: snapshot.cache_path,
        present: snapshot.present,
        relay_count: snapshot.relay_count,
        daita_relay_count: snapshot.daita_relay_count,
        fetched_at_unix_secs: snapshot.fetched_at_unix_secs,
    }
}
