mod protocol;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "windows")]
mod windows;

use std::sync::{Arc, Mutex};

type ActivateCallback = Arc<dyn Fn() + Send + Sync + 'static>;
type KeepAliveHandle = Arc<dyn KeepAlive>;

trait KeepAlive: Send + Sync + 'static {}

impl<T> KeepAlive for T where T: Send + Sync + 'static {}

pub(crate) enum StartupDecision {
    Primary(PrimaryInstance),
    Secondary,
}

#[derive(Clone)]
pub(crate) struct PrimaryInstance {
    activation: Arc<ActivationState>,
    _guard: KeepAliveHandle,
}

#[derive(Default)]
struct ActivationState {
    inner: Mutex<ActivationInner>,
}

#[derive(Default)]
struct ActivationInner {
    callback: Option<ActivateCallback>,
    pending_activate: bool,
}

enum PlatformStartup {
    Primary(PlatformGuard),
    Secondary,
}

#[cfg(target_os = "linux")]
type PlatformGuard = linux::PrimaryGuard;
#[cfg(target_os = "windows")]
type PlatformGuard = windows::PrimaryGuard;
#[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
type PlatformGuard = NoopPrimaryGuard;

#[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
struct NoopPrimaryGuard;

pub(crate) fn startup() -> Result<StartupDecision, String> {
    if allow_multi_instance() {
        return Ok(StartupDecision::Primary(PrimaryInstance {
            activation: Arc::new(ActivationState::default()),
            _guard: Arc::new(()),
        }));
    }

    let activation = Arc::new(ActivationState::default());
    match platform_startup(activation.clone())? {
        PlatformStartup::Primary(guard) => Ok(StartupDecision::Primary(PrimaryInstance {
            activation,
            _guard: Arc::new(guard),
        })),
        PlatformStartup::Secondary => Ok(StartupDecision::Secondary),
    }
}

pub(crate) fn report_startup_error(message: &str) {
    #[cfg(target_os = "windows")]
    windows::show_bootstrap_error(message);

    #[cfg(not(target_os = "windows"))]
    {
        let _ = message;
    }
}

impl PrimaryInstance {
    pub(crate) fn attach<F>(&self, on_activate: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        let callback: ActivateCallback = Arc::new(on_activate);
        let should_deliver_pending = if let Ok(mut inner) = self.activation.inner.lock() {
            inner.callback = Some(callback.clone());
            std::mem::take(&mut inner.pending_activate)
        } else {
            false
        };
        if should_deliver_pending {
            callback();
        }
    }
}

impl ActivationState {
    fn notify_activate(&self) {
        let callback = if let Ok(mut inner) = self.inner.lock() {
            if let Some(callback) = inner.callback.as_ref().cloned() {
                Some(callback)
            } else {
                inner.pending_activate = true;
                None
            }
        } else {
            None
        };
        if let Some(callback) = callback {
            callback();
        }
    }
}

#[cfg(test)]
impl PrimaryInstance {
    pub(crate) fn new_for_tests() -> Self {
        Self {
            activation: Arc::new(ActivationState::default()),
            _guard: Arc::new(()),
        }
    }

    pub(crate) fn trigger_activate_for_tests(&self) {
        self.activation.notify_activate();
    }
}

fn allow_multi_instance() -> bool {
    std::env::var_os("R_WG_ALLOW_MULTI_INSTANCE")
        .map(|value| value == std::ffi::OsStr::new("1"))
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
fn platform_startup(activation: Arc<ActivationState>) -> Result<PlatformStartup, String> {
    linux::startup(activation)
}

#[cfg(target_os = "windows")]
fn platform_startup(activation: Arc<ActivationState>) -> Result<PlatformStartup, String> {
    windows::startup(activation)
}

#[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
fn platform_startup(_activation: Arc<ActivationState>) -> Result<PlatformStartup, String> {
    Ok(PlatformStartup::Primary(NoopPrimaryGuard))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use super::PrimaryInstance;

    #[test]
    fn pending_activate_is_delivered_after_attach() {
        let primary = PrimaryInstance::new_for_tests();
        let calls = Arc::new(AtomicUsize::new(0));

        primary.trigger_activate_for_tests();

        let calls_for_attach = calls.clone();
        primary.attach(move || {
            calls_for_attach.fetch_add(1, Ordering::SeqCst);
        });

        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn activate_after_attach_fires_immediately() {
        let primary = PrimaryInstance::new_for_tests();
        let calls = Arc::new(AtomicUsize::new(0));

        let calls_for_attach = calls.clone();
        primary.attach(move || {
            calls_for_attach.fetch_add(1, Ordering::SeqCst);
        });
        primary.trigger_activate_for_tests();

        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
