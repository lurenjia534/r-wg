use super::*;

impl WgApp {
    pub(crate) fn cache_config_text(&mut self, path: PathBuf, text: SharedString) {
        self.selection.config_text_cache.insert(path.clone(), text);
        self.selection
            .config_text_cache_order
            .retain(|entry| entry != &path);
        self.selection.config_text_cache_order.push_back(path);
        while self.selection.config_text_cache_order.len() > CONFIG_TEXT_CACHE_LIMIT {
            if let Some(evicted) = self.selection.config_text_cache_order.pop_front() {
                self.selection.config_text_cache.remove(&evicted);
            }
        }
        self.selection.selection_revision = self.selection.selection_revision.wrapping_add(1);
    }

    pub(crate) fn cached_config_text(&mut self, path: &Path) -> Option<SharedString> {
        let text = self.selection.config_text_cache.get(path).cloned();
        if text.is_some() {
            self.selection
                .config_text_cache_order
                .retain(|entry| entry != path);
            self.selection
                .config_text_cache_order
                .push_back(path.to_path_buf());
        }
        text
    }

    pub(crate) fn peek_cached_config_text(&self, path: &Path) -> Option<SharedString> {
        self.selection.config_text_cache.get(path).cloned()
    }
}
