use std::time::Duration;

use gpui::{AppContext, Context, Window};

use super::super::persistence;
use super::super::state::WgApp;
use crate::ui::features::configs::state::build_configs_library_rows;
use restore::PersistedStateRestore;
use snapshot::PersistedStateSnapshot;

mod configs;
mod prefs;
mod restore;
mod snapshot;
mod traffic;

impl WgApp {
    pub(crate) fn start_load_persisted_state(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.selection.begin_persistence_load() {
            return;
        }

        let storage = match self.configs.ensure_storage() {
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
                        Ok(Some(state)) => match PersistedStateRestore::decode(state, &storage, cx)
                        {
                            Ok(restored) => {
                                let summary = restored.apply(
                                    &mut this.configs,
                                    &mut this.selection,
                                    &mut this.stats,
                                    &mut this.ui_prefs,
                                );
                                if let Some(workspace) = this.ui.configs_workspace.clone() {
                                    let rows = build_configs_library_rows(
                                        &this.configs,
                                        &this.runtime,
                                        &workspace.read(cx).draft,
                                    );
                                    workspace.update(cx, |workspace, cx| {
                                        if workspace.set_library_rows(rows) {
                                            cx.notify();
                                        }
                                    });
                                }
                                this.ui_session.sync_from_prefs(&this.ui_prefs);
                                this.apply_theme_prefs(Some(window), cx);
                                if let Some(selected_id) = summary.selected_id {
                                    this.load_config_into_inputs(selected_id, window, cx);
                                }
                                if let Some(theme_notice) = summary.theme_notice {
                                    this.push_success_toast(theme_notice, window, cx);
                                }
                                if summary.missing_files > 0 {
                                    this.set_status(format!(
                                        "Loaded {} configs, {} missing",
                                        summary.loaded_count, summary.missing_files
                                    ));
                                    this.persist_state_async(cx);
                                } else if summary.theme_prefs_migrated {
                                    if summary.loaded_count == 0 {
                                        this.set_status("Ready");
                                    } else {
                                        this.set_status(format!(
                                            "Loaded {} configs",
                                            summary.loaded_count
                                        ));
                                    }
                                    this.persist_state_async(cx);
                                } else if summary.loaded_count == 0 {
                                    this.set_status("Ready");
                                } else {
                                    this.set_status(format!(
                                        "Loaded {} configs",
                                        summary.loaded_count
                                    ));
                                }
                            }
                            Err(err) => {
                                this.set_error(err);
                            }
                        },
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
        // 单 writer + debounce：避免多次快速变更时旧快照后写覆盖新快照。
        let storage = match self.configs.ensure_storage() {
            Ok(storage) => storage,
            Err(err) => {
                self.set_error(err);
                cx.notify();
                return;
            }
        };
        self.persistence.enqueue();

        if self.persistence.worker_active {
            return;
        }
        self.persistence.worker_active = true;

        cx.spawn(async move |view, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(200))
                .await;

            let Some(state) = view
                .update(cx, |this, _cx| {
                    match this.persistence.take_queued_revision() {
                        Some(_) => {}
                        None => {
                            this.persistence.worker_active = false;
                            return None;
                        }
                    }
                    Some(PersistedStateSnapshot::capture(this).build())
                })
                .ok()
                .flatten()
            else {
                break;
            };

            let save_task = cx.background_spawn({
                let storage = storage.clone();
                async move { persistence::save_state(&storage, &state) }
            });

            let result = save_task.await;

            let should_continue = view
                .update(cx, |this, cx| {
                    if let Err(err) = result {
                        this.set_error(err);
                        cx.notify();
                    }

                    if this.persistence.has_pending() {
                        true
                    } else {
                        this.persistence.worker_active = false;
                        false
                    }
                })
                .unwrap_or(false);

            if !should_continue {
                break;
            }
        })
        .detach();
    }
}
