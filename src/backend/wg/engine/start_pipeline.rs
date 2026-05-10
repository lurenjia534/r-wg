#[cfg(target_os = "linux")]
use std::net::IpAddr;

use gotatun::device;

use crate::core::config::{self, DeviceSettings, WireGuardConfig};
use crate::core::route_plan::{
    normalize_config_for_runtime, RoutePlan, RoutePlanPlatform, DEFAULT_FULL_TUNNEL_FWMARK,
};
use crate::log::events::engine as log_engine;
use crate::platform;

use super::super::ephemeral;
#[cfg(target_os = "linux")]
use super::super::ephemeral::EphemeralFailureKind;
#[cfg(target_os = "linux")]
use super::super::linux_kernel::{self, KernelWireGuardError};
use super::backend_strategy::{resolve_backend, BackendDecision};
use super::state::{ActiveWireGuardBackend, DeviceSlot, EngineState};
use super::{EngineError, StartRequest, WireGuardBackendPreference};

#[cfg(target_os = "linux")]
enum KernelStartError {
    Kernel(KernelWireGuardError),
    Engine(EngineError),
}

#[cfg(target_os = "linux")]
impl KernelStartError {
    fn into_engine_error(self) -> EngineError {
        match self {
            Self::Kernel(error) => EngineError::KernelWireGuard(error.to_string()),
            Self::Engine(error) => error,
        }
    }
}

impl EngineState {
    /// 启动设备：
    /// - 解析配置并转换为 gotatun Set 请求。
    /// - 创建 TUN 并绑定 UDP socket。
    /// - 写入私钥/端口/peer 配置，若失败则停止设备。
    /// - 应用系统网络配置，失败则停止设备并返回错误。
    pub(super) async fn start(&mut self, request: StartRequest) -> Result<(), EngineError> {
        if self.running {
            return Err(EngineError::AlreadyRunning);
        }
        self.route_apply_report = None;
        if self.active_backend.is_some() {
            self.shutdown_active_backend().await;
        }
        self.quantum_active = false;
        self.daita_active = false;
        self.ephemeral_key_active = false;
        self.last_quantum_failure = None;
        self.last_daita_failure = None;

        log_engine::start(&request.tun_name, request.config_text.len());
        log_engine::wireguard_backend_preference(request.wireguard_backend_preference.as_str());
        let backend_decision = resolve_backend(
            request.wireguard_backend_preference,
            request.daita_mode,
            request.quantum_mode,
        )?;
        log_engine::wireguard_backend_resolved(backend_decision.as_str());

        // 解析配置并映射为 gotatun 的 DeviceSettings。
        let parsed = config::parse_config(&request.config_text)?;
        let inserted_fwmark = wants_full_tunnel(&parsed.peers) && parsed.interface.fwmark.is_none();
        let parsed = normalize_config_for_runtime(parsed, request.dns);
        if inserted_fwmark {
            log_engine::auto_fwmark(DEFAULT_FULL_TUNNEL_FWMARK);
        }
        if request.daita_mode.is_enabled() {
            match ephemeral::validate_daita_config(&parsed) {
                Ok(()) => {}
                Err(error) => {
                    self.last_daita_failure = Some(error.kind());
                    return Err(EngineError::Ephemeral(error.to_string()));
                }
            }
        }
        let route_plan = RoutePlan::build(RoutePlanPlatform::current(), &parsed);
        let settings = parsed.to_device_settings().await?;
        log_engine::config_parsed();

        if matches!(backend_decision, BackendDecision::LinuxKernel) {
            #[cfg(target_os = "linux")]
            match self
                .start_linux_kernel(&request.tun_name, &parsed, &route_plan, &settings, &request)
                .await
            {
                Ok(()) => return Ok(()),
                Err(KernelStartError::Kernel(error))
                    if request.wireguard_backend_preference == WireGuardBackendPreference::Auto
                        && error.is_unavailable() =>
                {
                    log_engine::wireguard_backend_fallback(&error.to_string());
                }
                Err(error) => return Err(error.into_engine_error()),
            }

            #[cfg(not(target_os = "linux"))]
            return Err(EngineError::UnsupportedBackend(
                "kernel WireGuard backend is only supported on Linux".to_string(),
            ));
        }

        self.start_userspace_gotatun(&request.tun_name, &parsed, &route_plan, &settings, &request)
            .await
    }

    async fn start_userspace_gotatun(
        &mut self,
        tun_name: &str,
        parsed: &WireGuardConfig,
        route_plan: &RoutePlan,
        settings: &DeviceSettings,
        request: &StartRequest,
    ) -> Result<(), EngineError> {
        if self.active_backend.is_some() {
            self.shutdown_active_backend().await;
        }
        if let Some(slot) = &self.cached_userspace_device {
            if slot.tun_name != tun_name {
                self.shutdown_cached_userspace_device().await;
            }
        }

        let mut created_new = false;
        let slot = match self.cached_userspace_device.take() {
            Some(slot) => slot,
            None => {
                let handle = device::build()
                    .with_default_udp()
                    .create_tun(tun_name)?
                    .build()
                    .await?;
                created_new = true;
                log_engine::device_created();
                DeviceSlot {
                    device: handle,
                    tun_name: tun_name.to_string(),
                }
            }
        };

        let slot = match self.configure_userspace_device(slot, settings).await {
            Ok(slot) => slot,
            Err((slot, err)) => {
                self.recover_userspace_slot_after_start_failure(slot, created_new)
                    .await;
                return Err(EngineError::Device(err));
            }
        };
        log_engine::device_configured();

        let net_result = match platform::apply_network_config(
            tun_name,
            parsed,
            route_plan,
            request.kill_switch_enabled,
        )
        .await
        {
            Ok(result) => result,
            Err(err) => {
                self.route_apply_report = Some(err.report);
                self.recover_userspace_slot_after_start_failure(slot, created_new)
                    .await;
                return Err(EngineError::Network(err.error));
            }
        };
        self.net_state = Some(net_result.state);
        log_engine::network_configured();
        self.route_apply_report = Some(net_result.report.clone());

        if request.quantum_mode.is_enabled() || request.daita_mode.is_enabled() {
            if let Err(error) = self
                .upgrade_userspace_ephemeral(tun_name, parsed, settings, &slot, request)
                .await
            {
                let cleanup_result = self.cleanup_active_network_state().await;
                self.recover_userspace_slot_after_start_failure(slot, created_new)
                    .await;
                return match cleanup_result {
                    Ok(()) => Err(error),
                    Err(err) => Err(err),
                };
            }
        }

        self.active_backend = Some(ActiveWireGuardBackend::Userspace(slot));
        self.running = true;
        Ok(())
    }

    async fn configure_userspace_device(
        &self,
        slot: DeviceSlot,
        settings: &DeviceSettings,
    ) -> Result<DeviceSlot, (DeviceSlot, device::Error)> {
        let result = slot
            .device
            .write(async |device| {
                device.set_private_key(settings.private_key.clone()).await;
                if let Some(port) = settings.listen_port {
                    device.set_listen_port(port);
                }
                #[cfg(target_os = "linux")]
                if let Some(fwmark) = settings.fwmark {
                    device.set_fwmark(fwmark)?;
                }
                device.clear_peers();
                device.add_peers(settings.peers.clone());
                Ok::<_, device::Error>(())
            })
            .await
            .and_then(|result| result);

        match result {
            Ok(()) => Ok(slot),
            Err(error) => Err((slot, error)),
        }
    }

    async fn upgrade_userspace_ephemeral(
        &mut self,
        tun_name: &str,
        parsed: &WireGuardConfig,
        settings: &DeviceSettings,
        slot: &DeviceSlot,
        request: &StartRequest,
    ) -> Result<(), EngineError> {
        log_engine::ephemeral_negotiation_requested(
            request.quantum_mode.is_enabled(),
            request.daita_mode.is_enabled(),
        );
        let Some(base_peer) = settings.peers.first() else {
            let error = ephemeral::Error::UnsupportedConfig(
                "ephemeral peer negotiation requires exactly one configured peer",
            );
            if request.quantum_mode.is_enabled() {
                self.last_quantum_failure = Some(error.kind());
            }
            if request.daita_mode.is_enabled() {
                self.last_daita_failure = Some(error.kind());
            }
            let message = error.to_string();
            log_engine::ephemeral_negotiation_failed(&message);
            return Err(EngineError::Ephemeral(message));
        };

        match ephemeral::upgrade_tunnel(
            request.quantum_mode,
            request.daita_mode,
            &slot.device,
            tun_name,
            parsed,
            base_peer,
        )
        .await
        {
            Ok(outcome) => {
                log_engine::ephemeral_negotiation_completed(
                    outcome.quantum_applied,
                    outcome.daita_applied,
                );
                self.quantum_active = outcome.quantum_applied;
                self.daita_active = outcome.daita_applied;
                self.ephemeral_key_active = outcome.quantum_applied || outcome.daita_applied;
                self.last_quantum_failure = None;
                self.last_daita_failure = None;
                Ok(())
            }
            Err(error) => {
                if request.quantum_mode.is_enabled() {
                    self.last_quantum_failure = Some(error.kind());
                }
                if request.daita_mode.is_enabled() {
                    self.last_daita_failure = Some(error.kind());
                }
                let message = error.to_string();
                log_engine::ephemeral_negotiation_failed(&message);
                Err(EngineError::Ephemeral(message))
            }
        }
    }

    async fn recover_userspace_slot_after_start_failure(
        &mut self,
        slot: DeviceSlot,
        created_new: bool,
    ) {
        if created_new {
            super::stop_pipeline::stop_userspace_slot(slot).await;
        } else {
            super::stop_pipeline::cache_cleared_userspace_slot(
                &mut self.cached_userspace_device,
                slot,
            )
            .await;
        }
    }

    #[cfg(target_os = "linux")]
    async fn start_linux_kernel(
        &mut self,
        tun_name: &str,
        parsed: &WireGuardConfig,
        route_plan: &RoutePlan,
        settings: &DeviceSettings,
        request: &StartRequest,
    ) -> Result<(), KernelStartError> {
        if self.active_backend.is_some() {
            self.shutdown_active_backend().await;
        }
        self.shutdown_cached_userspace_device().await;

        let kernel_device = linux_kernel::start_kernel_device(tun_name, settings)
            .await
            .map_err(KernelStartError::Kernel)?;
        log_engine::kernel_device_created(tun_name);

        let mut quantum_guard = if request.quantum_mode.is_enabled() {
            match platform::linux::apply_quantum_negotiation_traffic_guard(
                tun_name,
                route_plan_has_ipv6_tunnel_routes(route_plan),
            )
            .await
            {
                Ok(guard) => Some(guard),
                Err(error) => {
                    if linux_kernel::delete_kernel_device(kernel_device.tun_name())
                        .await
                        .is_ok()
                    {
                        let _ = linux_kernel::clear_kernel_backend_journal();
                    }
                    return Err(KernelStartError::Engine(EngineError::Network(error)));
                }
            }
        } else {
            None
        };

        let net_result = match platform::apply_network_config(
            tun_name,
            parsed,
            route_plan,
            request.kill_switch_enabled,
        )
        .await
        {
            Ok(result) => result,
            Err(err) => {
                if let Some(guard) = quantum_guard.take() {
                    let _ = platform::linux::cleanup_quantum_negotiation_traffic_guard(guard).await;
                }
                self.route_apply_report = Some(err.report);
                if linux_kernel::delete_kernel_device(kernel_device.tun_name())
                    .await
                    .is_ok()
                {
                    let _ = linux_kernel::clear_kernel_backend_journal();
                }
                return Err(KernelStartError::Engine(EngineError::Network(err.error)));
            }
        };

        self.net_state = Some(net_result.state);
        log_engine::network_configured();
        self.route_apply_report = Some(net_result.report.clone());
        if let Err(error) = linux_kernel::mark_kernel_device_running(tun_name) {
            if let Some(guard) = quantum_guard.take() {
                let _ = platform::linux::cleanup_quantum_negotiation_traffic_guard(guard).await;
            }
            let cleanup_result = self.cleanup_active_network_state().await;
            if linux_kernel::delete_kernel_device(kernel_device.tun_name())
                .await
                .is_ok()
            {
                let _ = linux_kernel::clear_kernel_backend_journal();
            }
            return match cleanup_result {
                Ok(()) => Err(KernelStartError::Kernel(error)),
                Err(err) => Err(KernelStartError::Engine(err)),
            };
        }

        if request.quantum_mode.is_enabled() {
            if let Err(error) = self
                .upgrade_kernel_ephemeral(tun_name, parsed, settings, request)
                .await
            {
                let guard_cleanup_result = if let Some(guard) = quantum_guard.take() {
                    platform::linux::cleanup_quantum_negotiation_traffic_guard(guard)
                        .await
                        .map_err(EngineError::from)
                } else {
                    Ok(())
                };
                let cleanup_result = self.cleanup_active_network_state().await;
                if linux_kernel::delete_kernel_device(kernel_device.tun_name())
                    .await
                    .is_ok()
                {
                    let _ = linux_kernel::clear_kernel_backend_journal();
                }
                return match (guard_cleanup_result, cleanup_result) {
                    (Err(err), _) => Err(KernelStartError::Engine(err)),
                    (Ok(()), Ok(())) => Err(error),
                    (Ok(()), Err(err)) => Err(KernelStartError::Engine(err)),
                };
            }

            if let Some(guard) = quantum_guard.take() {
                if let Err(error) =
                    platform::linux::cleanup_quantum_negotiation_traffic_guard(guard).await
                {
                    let cleanup_result = self.cleanup_active_network_state().await;
                    if linux_kernel::delete_kernel_device(kernel_device.tun_name())
                        .await
                        .is_ok()
                    {
                        let _ = linux_kernel::clear_kernel_backend_journal();
                    }
                    return match cleanup_result {
                        Ok(()) => Err(KernelStartError::Engine(EngineError::Network(error))),
                        Err(err) => Err(KernelStartError::Engine(err)),
                    };
                }
            }
        }

        self.active_backend = Some(ActiveWireGuardBackend::LinuxKernel(kernel_device));
        self.running = true;
        Ok(())
    }

    #[cfg(target_os = "linux")]
    async fn upgrade_kernel_ephemeral(
        &mut self,
        tun_name: &str,
        parsed: &WireGuardConfig,
        settings: &DeviceSettings,
        request: &StartRequest,
    ) -> Result<(), KernelStartError> {
        if request.daita_mode.is_enabled() {
            return Err(KernelStartError::Engine(EngineError::UnsupportedBackend(
                "DAITA requires userspace GotaTun".to_string(),
            )));
        }
        log_engine::ephemeral_negotiation_requested(
            request.quantum_mode.is_enabled(),
            request.daita_mode.is_enabled(),
        );
        let Some(base_peer) = settings.peers.first() else {
            let error = ephemeral::Error::UnsupportedConfig(
                "ephemeral peer negotiation requires exactly one configured peer",
            );
            self.last_quantum_failure = Some(error.kind());
            let message = error.to_string();
            log_engine::ephemeral_negotiation_failed(&message);
            return Err(KernelStartError::Engine(EngineError::Ephemeral(message)));
        };

        let update = match ephemeral::negotiate_tunnel_upgrade(
            request.quantum_mode,
            request.daita_mode,
            tun_name,
            parsed,
            base_peer,
        )
        .await
        {
            Ok(update) => update,
            Err(error) => {
                self.last_quantum_failure = Some(error.kind());
                let message = error.to_string();
                log_engine::ephemeral_negotiation_failed(&message);
                return Err(KernelStartError::Engine(EngineError::Ephemeral(message)));
            }
        };

        if let Err(error) = linux_kernel::apply_ephemeral_update(tun_name, &update).await {
            self.last_quantum_failure = Some(EphemeralFailureKind::Reconfigure);
            let message = error.to_string();
            log_engine::ephemeral_negotiation_failed(&message);
            return Err(KernelStartError::Engine(EngineError::Ephemeral(format!(
                "failed to apply kernel ephemeral update: {message}"
            ))));
        }

        let outcome = update.outcome();
        log_engine::ephemeral_negotiation_completed(outcome.quantum_applied, outcome.daita_applied);
        self.quantum_active = outcome.quantum_applied;
        self.daita_active = outcome.daita_applied;
        self.ephemeral_key_active = outcome.quantum_applied || outcome.daita_applied;
        self.last_quantum_failure = None;
        self.last_daita_failure = None;
        Ok(())
    }
}

fn wants_full_tunnel(peers: &[config::PeerConfig]) -> bool {
    peers.iter().any(|peer| {
        peer.allowed_ips
            .iter()
            .any(|allowed| allowed.addr.is_unspecified() && allowed.cidr == 0)
    })
}

#[cfg(target_os = "linux")]
fn route_plan_has_ipv6_tunnel_routes(route_plan: &RoutePlan) -> bool {
    route_plan
        .allowed_routes
        .iter()
        .any(|route| matches!(route.addr, IpAddr::V6(_)))
}
