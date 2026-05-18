use gpui::{SharedString, Window};
use gpui_component::theme::ThemeMode;

use crate::ui::features::themes::{self, AppearancePolicy};

use super::WgApp;

impl WgApp {
    pub(crate) fn set_appearance_policy_pref(
        &mut self,
        value: AppearancePolicy,
        window: Option<&mut Window>,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.ui_prefs.appearance_policy != value {
            self.ui_prefs.appearance_policy = value;
            let refresh_all_windows = window.is_none();
            self.apply_theme_prefs(window, cx);
            if refresh_all_windows {
                cx.refresh_windows();
            }
            self.persist_state_async(cx);
        }
        cx.notify();
    }

    pub(crate) fn set_theme_palette_pref(
        &mut self,
        mode: ThemeMode,
        value: Option<SharedString>,
        window: Option<&mut Window>,
        cx: &mut gpui::Context<Self>,
    ) {
        let slot = match mode {
            ThemeMode::Light => &mut self.ui_prefs.theme_light_key,
            ThemeMode::Dark => &mut self.ui_prefs.theme_dark_key,
        };

        if *slot != value {
            *slot = value;
            match mode {
                ThemeMode::Light => self.ui_prefs.theme_light_name = None,
                ThemeMode::Dark => self.ui_prefs.theme_dark_name = None,
            }

            let active_mode_changed = self.ui_prefs.resolved_theme_mode == mode;
            let refresh_all_windows = active_mode_changed && window.is_none();
            self.apply_theme_prefs(if active_mode_changed { window } else { None }, cx);
            if refresh_all_windows {
                cx.refresh_windows();
            }
            self.persist_state_async(cx);
        }
        cx.notify();
    }

    pub(crate) fn reset_theme_prefs(
        &mut self,
        window: Option<&mut Window>,
        cx: &mut gpui::Context<Self>,
    ) {
        let changed = self.ui_prefs.theme_light_key.take().is_some()
            || self.ui_prefs.theme_dark_key.take().is_some()
            || self.ui_prefs.theme_light_name.take().is_some()
            || self.ui_prefs.theme_dark_name.take().is_some();

        if changed {
            let refresh_all_windows = window.is_none();
            self.apply_theme_prefs(window, cx);
            if refresh_all_windows {
                cx.refresh_windows();
            }
            self.persist_state_async(cx);
        }
        cx.notify();
    }

    pub(crate) fn apply_theme_prefs(
        &mut self,
        window: Option<&mut Window>,
        cx: &mut gpui::Context<Self>,
    ) {
        let storage = self.configs.ensure_storage().ok();
        let light = themes::resolve_theme_preference(
            ThemeMode::Light,
            self.ui_prefs.theme_light_key.as_deref(),
            self.ui_prefs.theme_light_name.as_deref(),
            storage.as_ref(),
            cx,
        );
        let dark = themes::resolve_theme_preference(
            ThemeMode::Dark,
            self.ui_prefs.theme_dark_key.as_deref(),
            self.ui_prefs.theme_dark_name.as_deref(),
            storage.as_ref(),
            cx,
        );
        self.ui_prefs.theme_light_key = Some(light.entry.key.clone());
        self.ui_prefs.theme_dark_key = Some(dark.entry.key.clone());
        self.ui_prefs.theme_light_name = Some(light.entry.name.clone());
        self.ui_prefs.theme_dark_name = Some(dark.entry.name.clone());
        self.ui_prefs.resolved_theme_mode = themes::apply_resolved_theme_preferences(
            self.ui_prefs.appearance_policy,
            light.entry.config.clone(),
            dark.entry.config.clone(),
            window,
            cx,
        );
    }
}
