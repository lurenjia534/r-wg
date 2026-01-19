use gpui::{AppContext, Context, Window};
use gpui_component::theme::Theme;

use super::super::persistence::{
    self, PersistedConfig, PersistedSource, PersistedState, StoragePaths, STATE_VERSION,
};
use super::super::state::{ConfigSource, TunnelConfig, WgApp};

impl WgApp {
    pub(crate) fn ensure_storage(&mut self) -> Result<StoragePaths, String> {
        if let Some(storage) = &self.storage {
            return Ok(storage.clone());
        }
        let storage = persistence::ensure_storage_dirs()?;
        self.storage = Some(storage.clone());
        Ok(storage)
    }

    pub(crate) fn alloc_config_id(&mut self) -> u64 {
        let id = self.next_config_id.max(1);
        self.next_config_id = id.saturating_add(1);
        id
    }

    pub(crate) fn start_load_persisted_state(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.persistence_loaded {
            return;
        }
        self.persistence_loaded = true;

        let storage = match self.ensure_storage() {
            Ok(storage) => storage,
            Err(err) => {
                self.set_error(err);
                cx.notify();
                return;
            }
        };

        self.set_status("Loading configs...");
        cx.notify();

        let view = cx.weak_entity();
        window
            .spawn(cx, async move |cx| {
                let storage_for_task = storage.clone();
                let load_task =
                    cx.background_spawn(async move { persistence::load_state(&storage_for_task) });
                let result = load_task.await;
                view.update_in(cx, |this, window, cx| {
                    match result {
                        Ok(Some(state)) => {
                            this.apply_persisted_state(state, &storage, window, cx);
                        }
                        Ok(None) => {
                            this.set_status("Ready");
                        }
                        Err(err) => {
                            this.set_error(err);
                        }
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
    }

    pub(crate) fn persist_state_async(&mut self, cx: &mut Context<Self>) {
        let storage = match self.ensure_storage() {
            Ok(storage) => storage,
            Err(err) => {
                self.set_error(err);
                cx.notify();
                return;
            }
        };
        let state = self.build_persisted_state();

        cx.spawn(async move |view, cx| {
            let save_task =
                cx.background_spawn(async move { persistence::save_state(&storage, &state) });
            if let Err(err) = save_task.await {
                view.update(cx, |this, cx| {
                    this.set_error(err);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    fn apply_persisted_state(
        &mut self,
        state: PersistedState,
        storage: &StoragePaths,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if state.version != STATE_VERSION {
            self.set_error(format!(
                "Unsupported state version: {}",
                state.version
            ));
            return;
        }

        // 尽早应用主题，避免启动时闪烁。
        if let Some(theme_mode) = state.theme_mode {
            self.theme_mode = theme_mode;
            Theme::change(theme_mode, Some(window), cx);
        }

        let mut configs = Vec::new();
        let mut max_id = 0u64;
        let mut missing_files = 0usize;
        for entry in state.configs {
            if entry.id == 0 || entry.name.trim().is_empty() {
                continue;
            }
            max_id = max_id.max(entry.id);
            let storage_path = persistence::config_path(storage, entry.id);
            if !storage_path.exists() {
                // 清理逻辑说明：
                // - 元数据里存在条目，但内部 configs/<id>.conf 丢失，说明用户手动删了文件
                //   或者上一次写盘失败导致文件缺失。
                // - 这种情况下继续保留条目会导致启动/编辑/运行时反复报错，
                //   用户看见但无法使用，体验更差。
                // - 因此我们在加载阶段直接跳过该条目，并在后续保存时把 state.json
                //   里的“悬空记录”清理掉，让元数据与磁盘一致。
                // - missing_files 计数用于状态提示，便于用户意识到有条目被清理。
                missing_files += 1;
                continue;
            }
            let source = ConfigSource::from(entry.source);
            configs.push(TunnelConfig {
                id: entry.id,
                name: entry.name.clone(),
                name_lower: entry.name.to_lowercase(),
                text: None,
                source,
                storage_path,
            });
        }

        self.configs = configs;
        self.selected = state.selected_id.and_then(|id| {
            self.configs.iter().position(|cfg| cfg.id == id)
        });
        self.next_config_id = state
            .next_id
            .max(max_id.saturating_add(1));
        self.proxy_filter_total = 0;
        self.parse_cache = None;
        self.loaded_config = None;
        self.loading_config = None;
        self.loading_config_path = None;

        if let Some(idx) = self.selected {
            self.load_config_into_inputs(idx, window, cx);
        }
        if missing_files > 0 {
            // 如果发现缺失文件，马上落盘一次：
            // - 这会把刚才跳过的条目从 state.json 中移除；
            // - 下次启动不会再遇到同样的“幽灵配置”；
            // - 这属于修复性写入，不改变其他配置内容。
            self.set_status(format!(
                "Loaded {} configs, {} missing",
                self.configs.len(),
                missing_files
            ));
            self.persist_state_async(cx);
        } else if self.configs.is_empty() {
            self.set_status("Ready");
        } else {
            self.set_status(format!("Loaded {} configs", self.configs.len()));
        }
    }

    fn build_persisted_state(&self) -> PersistedState {
        let selected_id = self
            .selected
            .and_then(|idx| self.configs.get(idx))
            .map(|cfg| cfg.id);
        PersistedState {
            version: STATE_VERSION,
            next_id: self.next_config_id,
            selected_id,
            // 保存当前主题，便于下次启动恢复。
            theme_mode: Some(self.theme_mode),
            configs: self
                .configs
                .iter()
                .map(|cfg| PersistedConfig {
                    id: cfg.id,
                    name: cfg.name.clone(),
                    source: PersistedSource::from(&cfg.source),
                })
                .collect(),
        }
    }
}
