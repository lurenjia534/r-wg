use crate::ui::i18n::{tr, Language, LanguagePreference};

use super::WgApp;

impl WgApp {
    pub(crate) fn language(&self) -> Language {
        self.ui_prefs.language_preference.resolve()
    }

    pub(crate) fn t(&self, key: &'static str) -> &'static str {
        tr(self.language(), key)
    }

    pub(crate) fn set_language_preference(
        &mut self,
        value: LanguagePreference,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.ui_prefs.language_preference != value {
            self.ui_prefs.language_preference = value;
            self.persist_state_async(cx);
            cx.refresh_windows();
        }
        cx.notify();
    }
}
