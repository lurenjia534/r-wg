use super::super::ephemeral::{DaitaMode, QuantumMode};
use super::{EngineError, WireGuardBackendPreference};

/// Concrete WireGuard data plane selected for a start request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BackendDecision {
    UserspaceGotaTun,
    LinuxKernel,
}

impl BackendDecision {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::UserspaceGotaTun => "UserspaceGotaTun",
            Self::LinuxKernel => "LinuxKernel",
        }
    }
}

pub(super) fn resolve_backend(
    preference: WireGuardBackendPreference,
    daita_mode: DaitaMode,
    _quantum_mode: QuantumMode,
) -> Result<BackendDecision, EngineError> {
    if daita_mode.is_enabled() {
        return match preference {
            WireGuardBackendPreference::Kernel => Err(EngineError::UnsupportedBackend(
                "DAITA currently requires GotaTun; switch WireGuard implementation to Userspace"
                    .to_string(),
            )),
            WireGuardBackendPreference::Auto | WireGuardBackendPreference::Userspace => {
                Ok(BackendDecision::UserspaceGotaTun)
            }
        };
    }

    match preference {
        WireGuardBackendPreference::Userspace => Ok(BackendDecision::UserspaceGotaTun),
        WireGuardBackendPreference::Kernel => linux_kernel_backend_decision(),
        WireGuardBackendPreference::Auto => Ok(default_auto_backend_decision()),
    }
}

fn linux_kernel_backend_decision() -> Result<BackendDecision, EngineError> {
    #[cfg(target_os = "linux")]
    {
        Ok(BackendDecision::LinuxKernel)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(EngineError::UnsupportedBackend(
            "kernel WireGuard backend is only supported on Linux".to_string(),
        ))
    }
}

fn default_auto_backend_decision() -> BackendDecision {
    #[cfg(target_os = "linux")]
    {
        BackendDecision::LinuxKernel
    }
    #[cfg(not(target_os = "linux"))]
    {
        BackendDecision::UserspaceGotaTun
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_resolution_handles_daita_conflicts_and_quantum_kernel_support() {
        assert_eq!(
            resolve_backend(
                WireGuardBackendPreference::Auto,
                DaitaMode::On,
                QuantumMode::Off,
            )
            .expect("auto daita should choose userspace"),
            BackendDecision::UserspaceGotaTun
        );
        #[cfg(target_os = "linux")]
        {
            assert_eq!(
                resolve_backend(
                    WireGuardBackendPreference::Auto,
                    DaitaMode::Off,
                    QuantumMode::On,
                )
                .expect("auto quantum should prefer kernel on Linux"),
                BackendDecision::LinuxKernel
            );
            assert_eq!(
                resolve_backend(
                    WireGuardBackendPreference::Kernel,
                    DaitaMode::Off,
                    QuantumMode::On,
                )
                .expect("kernel quantum should be supported on Linux"),
                BackendDecision::LinuxKernel
            );
        }
        #[cfg(not(target_os = "linux"))]
        {
            assert_eq!(
                resolve_backend(
                    WireGuardBackendPreference::Auto,
                    DaitaMode::Off,
                    QuantumMode::On,
                )
                .expect("auto quantum should remain userspace off Linux"),
                BackendDecision::UserspaceGotaTun
            );
            assert!(matches!(
                resolve_backend(
                    WireGuardBackendPreference::Kernel,
                    DaitaMode::Off,
                    QuantumMode::On,
                ),
                Err(EngineError::UnsupportedBackend(message)) if message.contains("Linux")
            ));
        }
        assert_eq!(
            resolve_backend(
                WireGuardBackendPreference::Userspace,
                DaitaMode::Off,
                QuantumMode::On,
            )
            .expect("userspace quantum should remain userspace"),
            BackendDecision::UserspaceGotaTun
        );
        assert!(matches!(
            resolve_backend(
                WireGuardBackendPreference::Kernel,
                DaitaMode::On,
                QuantumMode::Off,
            ),
            Err(EngineError::UnsupportedBackend(message)) if message.contains("DAITA")
        ));
    }

    #[test]
    fn backend_resolution_uses_platform_default_without_capability_probe() {
        #[cfg(target_os = "linux")]
        let expected_auto = BackendDecision::LinuxKernel;
        #[cfg(not(target_os = "linux"))]
        let expected_auto = BackendDecision::UserspaceGotaTun;

        assert_eq!(
            resolve_backend(
                WireGuardBackendPreference::Auto,
                DaitaMode::Off,
                QuantumMode::Off,
            )
            .expect("auto should resolve from platform defaults"),
            expected_auto
        );
        #[cfg(target_os = "linux")]
        assert_eq!(
            resolve_backend(
                WireGuardBackendPreference::Kernel,
                DaitaMode::Off,
                QuantumMode::Off,
            )
            .expect("kernel should be selected on Linux"),
            BackendDecision::LinuxKernel
        );
        #[cfg(not(target_os = "linux"))]
        assert!(matches!(
            resolve_backend(
                WireGuardBackendPreference::Kernel,
                DaitaMode::Off,
                QuantumMode::Off,
            ),
            Err(EngineError::UnsupportedBackend(message)) if message.contains("Linux")
        ));
    }
}
