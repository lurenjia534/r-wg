use r_wg::backend::wg::{DaitaMode, QuantumMode, WireGuardBackendPreference};
use r_wg::dns::{DnsMode, DnsPreset};

use super::WgApp;

impl WgApp {
    pub(crate) fn set_log_auto_follow_pref(&mut self, value: bool, cx: &mut gpui::Context<Self>) {
        if self.ui_prefs.log_auto_follow != value {
            self.ui_prefs.log_auto_follow = value;
            self.persist_state_async(cx);
        }
        cx.notify();
    }

    pub(crate) fn set_log_viewer_enabled_pref(
        &mut self,
        value: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.ui_prefs.log_viewer_enabled != value {
            self.ui_prefs.log_viewer_enabled = value;
            r_wg::log::set_buffer_enabled(value);
            if !value {
                self.stop_backend_log_polling();
                self.ui.backend_log_lines.clear();
                self.ui.backend_log_last_error = None;
                self.ui.backend_log_last_sync = None;
            }
            self.persist_state_async(cx);
        }
        cx.notify();
    }

    pub(crate) fn set_connect_password_required_pref(
        &mut self,
        value: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.ui_prefs.require_connect_password != value {
            self.ui_prefs.require_connect_password = value;
            self.persist_state_async(cx);
        }
        cx.notify();
    }

    pub(crate) fn set_kill_switch_enabled_pref(
        &mut self,
        value: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.ui_prefs.kill_switch_enabled != value {
            self.ui_prefs.kill_switch_enabled = value;
            self.persist_state_async(cx);
        }
        cx.notify();
    }

    pub(crate) fn set_dns_mode_pref(&mut self, value: DnsMode, cx: &mut gpui::Context<Self>) {
        if self.ui_prefs.dns_mode != value {
            self.ui_prefs.dns_mode = value;
            self.persist_state_async(cx);
        }
        cx.notify();
    }

    pub(crate) fn set_dns_preset_pref(&mut self, value: DnsPreset, cx: &mut gpui::Context<Self>) {
        if self.ui_prefs.dns_preset != value {
            self.ui_prefs.dns_preset = value;
            self.persist_state_async(cx);
        }
        cx.notify();
    }

    pub(crate) fn set_quantum_mode_pref(
        &mut self,
        value: QuantumMode,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.ui_prefs.quantum_mode != value {
            self.ui_prefs.quantum_mode = value;
            self.persist_state_async(cx);
        }
        cx.notify();
    }

    pub(crate) fn set_daita_mode_pref(&mut self, value: DaitaMode, cx: &mut gpui::Context<Self>) {
        if self.ui_prefs.daita_mode != value {
            self.ui_prefs.daita_mode = value;
            self.persist_state_async(cx);
        }
        cx.notify();
    }

    pub(crate) fn set_wireguard_backend_preference(
        &mut self,
        value: WireGuardBackendPreference,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.ui_prefs.wireguard_backend_preference != value {
            self.ui_prefs.wireguard_backend_preference = value;
            self.persist_state_async(cx);
        }
        cx.notify();
    }
}
