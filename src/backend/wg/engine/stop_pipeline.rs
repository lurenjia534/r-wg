use gotatun::device;

use crate::log::events::engine as log_engine;
use crate::platform;

#[cfg(target_os = "linux")]
use super::super::linux_kernel;
use super::state::{ActiveWireGuardBackend, DeviceSlot, EngineState};
use super::EngineError;

#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn trim_allocator() {
    // 尝试让 glibc 将空闲堆页归还给 OS，尽量降低 RSS（不保证一定生效）。
    unsafe {
        libc::malloc_trim(0);
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn trim_allocator() {
    // 非 glibc 平台不执行 trim，避免引入不兼容行为。
}

impl EngineState {
    pub(super) async fn cleanup_active_network_state(&mut self) -> Result<(), EngineError> {
        match self.net_state.take() {
            Some(state) => platform::cleanup_network_config(state)
                .await
                .map_err(EngineError::from),
            None => Ok(()),
        }
    }

    /// 停止设备：
    /// - 先回滚系统网络配置（若存在）。
    /// - 再按当前 active backend 清理数据面。
    pub(super) async fn stop(&mut self) -> Result<(), EngineError> {
        if !self.running {
            return Err(EngineError::NotRunning);
        };

        log_engine::stop_requested();

        // 优先回滚系统网络配置，避免留下路由/DNS 污染。
        let cleanup_result = if let Some(state) = self.net_state.take() {
            platform::cleanup_network_config(state)
                .await
                .map_err(EngineError::from)
        } else {
            Ok(())
        };

        if let Err(error) = self.stop_active_backend().await {
            if cleanup_result.is_ok() {
                self.route_apply_report = None;
                self.running = false;
                self.quantum_active = false;
                self.daita_active = false;
                self.ephemeral_key_active = false;
                self.last_quantum_failure = None;
                self.last_daita_failure = None;
                trim_allocator();
            }
            return Err(error);
        }
        self.route_apply_report = None;
        self.running = false;
        self.quantum_active = false;
        self.daita_active = false;
        self.ephemeral_key_active = false;
        self.last_quantum_failure = None;
        self.last_daita_failure = None;

        trim_allocator();

        // 若回滚失败，仍然返回错误以便上层提示。
        cleanup_result
    }

    async fn stop_active_backend(&mut self) -> Result<(), EngineError> {
        let Some(active_backend) = self.active_backend.take() else {
            return Ok(());
        };

        match active_backend {
            ActiveWireGuardBackend::Userspace(slot) => {
                if self.ephemeral_key_active {
                    stop_userspace_slot(slot).await;
                } else {
                    clear_userspace_slot_peers(&slot).await?;
                    self.cached_userspace_device = Some(slot);
                }
            }
            #[cfg(target_os = "linux")]
            ActiveWireGuardBackend::LinuxKernel(kernel_device) => {
                if let Err(error) =
                    linux_kernel::delete_kernel_device(kernel_device.tun_name()).await
                {
                    self.active_backend = Some(ActiveWireGuardBackend::LinuxKernel(kernel_device));
                    return Err(EngineError::KernelWireGuard(format!(
                        "failed to delete kernel WireGuard interface: {error}"
                    )));
                }
                linux_kernel::clear_kernel_backend_journal()
                    .map_err(|error| EngineError::KernelWireGuard(error.to_string()))?;
            }
        }
        Ok(())
    }

    /// 彻底停止并释放当前 active backend 与缓存的 userspace device。
    pub(super) async fn shutdown_active_backend(&mut self) {
        if let Some(active_backend) = self.active_backend.take() {
            match active_backend {
                ActiveWireGuardBackend::Userspace(slot) => stop_userspace_slot(slot).await,
                #[cfg(target_os = "linux")]
                ActiveWireGuardBackend::LinuxKernel(kernel_device) => {
                    if let Err(error) =
                        linux_kernel::delete_kernel_device(kernel_device.tun_name()).await
                    {
                        tracing::warn!("failed to delete kernel WireGuard interface: {error}");
                    } else if let Err(error) = linux_kernel::clear_kernel_backend_journal() {
                        tracing::warn!("failed to clear kernel WireGuard journal: {error}");
                    }
                }
            }
        }
        self.running = false;
        self.quantum_active = false;
        self.daita_active = false;
        self.ephemeral_key_active = false;
        self.last_quantum_failure = None;
        self.last_daita_failure = None;
        self.net_state = None;
        self.route_apply_report = None;
        trim_allocator();
    }

    pub(super) async fn shutdown_cached_userspace_device(&mut self) {
        if let Some(slot) = self.cached_userspace_device.take() {
            stop_userspace_slot(slot).await;
        }
    }
}

async fn clear_userspace_slot_peers(slot: &DeviceSlot) -> Result<(), device::Error> {
    let _ = slot.device.clear_peers().await?;
    Ok(())
}

pub(super) async fn cache_cleared_userspace_slot(cache: &mut Option<DeviceSlot>, slot: DeviceSlot) {
    if let Err(error) = clear_userspace_slot_peers(&slot).await {
        tracing::warn!("failed to clear cached GotaTun peers: {error}");
        stop_userspace_slot(slot).await;
        return;
    }
    *cache = Some(slot);
}

pub(super) async fn stop_userspace_slot(slot: DeviceSlot) {
    slot.device.stop().await;
    log_engine::device_stopped();
}
