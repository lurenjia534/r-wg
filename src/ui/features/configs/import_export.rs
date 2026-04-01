use std::collections::HashSet;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::process::Command;

use futures_util::stream::{FuturesUnordered, StreamExt};
use gpui::{AppContext, ClipboardItem, Context, PathPromptOptions, SharedString, Window};
use r_wg::application::{
    ConfigLibraryService, ConfigSourceKind, DeleteConfigsDecision, DeleteConfigsRequest,
    DeletePolicy, ExistingStoredConfig, ImportConfigJob, ImportedConfigArtifact,
    ImportedConfigRecord, PostDeleteSelection, PostDeleteSelectionRequest, RenameConfigDecision,
    RenameConfigRequest, SaveTargetRequest,
};
use r_wg::core::config;

use crate::ui::persistence;
use crate::ui::state::{
    ConfigSource, DraftValidationState, EditorOperation, EndpointFamily, LoadedConfigState,
    PendingDraftAction, TunnelConfig, WgApp,
};

use super::{dialogs, draft, endpoint_family};

pub(crate) enum ImportOutcome {
    Ok {
        imported: ImportedConfigRecord,
        endpoint_family: EndpointFamily,
    },
    Err {
        path: PathBuf,
        message: String,
    },
}

const IMPORT_CONCURRENCY: usize = 8;
const IMPORT_BATCH_SIZE: usize = 200;

pub(crate) fn handle_import_click(app: &mut WgApp, window: &mut Window, cx: &mut Context<WgApp>) {
    if app.configs_is_busy(cx) {
        return;
    }
    if app.configs_draft_snapshot(cx).is_dirty() {
        dialogs::confirm_discard_or_save(
            app,
            PendingDraftAction::Import,
            window,
            cx,
            "Import configs?",
            "Importing will replace the current unsaved draft context.",
        );
        return;
    }

    app.set_status("Opening file dialog...");
    cx.notify();

    let prompt = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: true,
        prompt: Some("Import WireGuard Config".into()),
    });

    let view = cx.weak_entity();
    window
        .spawn(cx, async move |cx| {
            let paths = match prompt.await {
                Ok(Ok(Some(paths))) => paths,
                Ok(Ok(None)) => {
                    view.update(cx, |this, cx| {
                        this.set_editor_operation(None, cx);
                        this.set_status("Import canceled");
                        cx.notify();
                    })
                    .ok();
                    return;
                }
                Ok(Err(err)) => {
                    let message = err.to_string();
                    if portal_missing_message(&message) {
                        view.update(cx, |this, cx| {
                            this.set_status("File dialog unavailable, trying fallback...");
                            cx.notify();
                        })
                        .ok();

                        let fallback = cx
                            .background_spawn(async move {
                                pick_file_fallback("Import WireGuard Config")
                            })
                            .await;

                        match fallback {
                            Ok(Some(paths)) => {
                                view.update_in(cx, |this, window, cx| {
                                    start_import_from_paths(this, paths, window, cx);
                                })
                                .ok();
                            }
                            Ok(None) => {
                                view.update(cx, |this, cx| {
                                    this.set_status("Import canceled");
                                    cx.notify();
                                })
                                .ok();
                            }
                            Err(err) => {
                                view.update(cx, |this, cx| {
                                    this.set_error(err);
                                    cx.notify();
                                })
                                .ok();
                            }
                        }
                        return;
                    }

                    view.update(cx, |this, cx| {
                        this.set_error(format!("File dialog failed: {message}"));
                        cx.notify();
                    })
                    .ok();
                    return;
                }
                Err(err) => {
                    view.update(cx, |this, cx| {
                        this.set_error(format!("File dialog closed: {err}"));
                        cx.notify();
                    })
                    .ok();
                    return;
                }
            };

            let paths: Vec<PathBuf> = paths.into_iter().collect();
            if paths.is_empty() {
                view.update(cx, |this, cx| {
                    this.set_error("No file selected");
                    cx.notify();
                })
                .ok();
                return;
            }

            view.update_in(cx, |this, window, cx| {
                start_import_from_paths(this, paths, window, cx);
            })
            .ok();
        })
        .detach();
}

pub(crate) fn start_import_from_paths(
    app: &mut WgApp,
    paths: Vec<PathBuf>,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) {
    if paths.is_empty() {
        app.set_error("No file selected");
        cx.notify();
        return;
    }

    let storage = match app.configs.ensure_storage() {
        Ok(storage) => storage,
        Err(err) => {
            app.set_error(err);
            cx.notify();
            return;
        }
    };

    let mut jobs = Vec::with_capacity(paths.len());
    for path in paths {
        let id = app.configs.alloc_config_id();
        let storage_path = persistence::config_path(&storage, id);
        jobs.push(ImportConfigJob {
            id,
            origin_path: path,
            storage_path,
        });
    }

    let total = jobs.len();
    app.set_editor_operation(
        Some(EditorOperation::Importing {
            processed: 0,
            total,
        }),
        cx,
    );
    app.set_status(format!("Loading {total} files..."));
    cx.notify();

    let view = cx.weak_entity();
    let config_library = app.config_library.clone();
    window
        .spawn(cx, async move |cx| {
            let mut batch = match view.update(cx, |this, _| {
                this.configs
                    .iter()
                    .map(|cfg| cfg.name.clone())
                    .collect::<HashSet<_>>()
            }) {
                Ok(names) => config_library.begin_import_batch(names, total),
                Err(_) => return,
            };

            let mut outcomes_batch = Vec::new();

            let concurrency = IMPORT_CONCURRENCY.min(total.max(1));
            let mut pending = jobs.into_iter();
            let mut tasks = FuturesUnordered::new();
            for _ in 0..concurrency {
                if let Some(job) = pending.next() {
                    let config_library = config_library.clone();
                    tasks.push(
                        cx.background_spawn(
                            async move { import_config(job, config_library).await },
                        ),
                    );
                }
            }

            while let Some(outcome) = tasks.next().await {
                let (outcome, progress) = match outcome {
                    ImportOutcome::Ok {
                        imported,
                        endpoint_family,
                    } => {
                        let recorded = config_library.record_import_success(&mut batch, imported);
                        (
                            ImportOutcome::Ok {
                                imported: recorded.config,
                                endpoint_family,
                            },
                            recorded.progress,
                        )
                    }
                    ImportOutcome::Err { path, message } => {
                        let progress =
                            config_library.record_import_failure(&mut batch, &path, &message);
                        (ImportOutcome::Err { path, message }, progress)
                    }
                };

                outcomes_batch.push(outcome);

                if let Some(job) = pending.next() {
                    let config_library = config_library.clone();
                    tasks.push(
                        cx.background_spawn(
                            async move { import_config(job, config_library).await },
                        ),
                    );
                }

                if outcomes_batch.len() >= IMPORT_BATCH_SIZE || progress.processed == progress.total
                {
                    let outcomes = std::mem::take(&mut outcomes_batch);
                    view.update_in(cx, |this, _window, cx| {
                        let mut imported_configs = Vec::new();
                        for outcome in outcomes {
                            if let ImportOutcome::Ok {
                                imported,
                                endpoint_family,
                            } = outcome
                            {
                                imported_configs.push(TunnelConfig {
                                    id: imported.id,
                                    name_lower: imported.name.to_lowercase(),
                                    name: imported.name,
                                    text: None,
                                    source: imported_config_source(
                                        imported.source,
                                        imported.origin_path,
                                    ),
                                    storage_path: imported.storage_path,
                                    endpoint_family,
                                });
                            }
                        }
                        if !imported_configs.is_empty() {
                            this.configs.extend(imported_configs.iter().cloned());
                            this.append_configs_workspace_library_rows(&imported_configs, cx);
                        }
                        this.set_editor_operation(
                            Some(EditorOperation::Importing {
                                processed: progress.processed,
                                total: progress.total,
                            }),
                            cx,
                        );
                        this.set_status(progress.status_message.clone());
                        cx.notify();
                    })
                    .ok();
                }
            }

            let summary = config_library.finish_import_batch(batch);
            view.update_in(cx, |this, window, cx| {
                this.set_editor_operation(None, cx);
                if let Some(config_id) = summary.selected_import_id {
                    this.set_selected_config_id(Some(config_id), cx);
                    this.load_config_into_inputs(config_id, window, cx);
                }
                if let Some(error_message) = summary.error_message.as_ref() {
                    this.set_error(error_message.clone());
                } else if let Some(status_message) = summary.status_message.as_ref() {
                    this.set_status(status_message.clone());
                }
                if summary.should_persist {
                    this.persist_state_async(cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
}

pub(crate) fn handle_export_click(app: &mut WgApp, cx: &mut Context<WgApp>) {
    if app.configs_is_busy(cx) {
        return;
    }

    let Some(selected) = app.selected_config().cloned() else {
        app.set_error("Select a tunnel first");
        cx.notify();
        return;
    };
    let cached_text = app.cached_config_text(&selected.storage_path);
    let initial_text = selected.text.clone().or(cached_text);

    app.set_editor_operation(Some(EditorOperation::Exporting), cx);
    app.set_status("Choose export folder...");
    cx.notify();

    let prompt = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some("Export WireGuard Config".into()),
    });
    let config_library = app.config_library.clone();

    cx.spawn(async move |view, cx| {
        let paths = match prompt.await {
            Ok(Ok(Some(paths))) => paths,
            Ok(Ok(None)) => {
                view.update(cx, |this, cx| {
                    this.set_editor_operation(None, cx);
                    this.set_status("Export canceled");
                    cx.notify();
                })
                .ok();
                return;
            }
            Ok(Err(err)) => {
                view.update(cx, |this, cx| {
                    this.set_editor_operation(None, cx);
                    this.set_error(format!("Export dialog failed: {err}"));
                    cx.notify();
                })
                .ok();
                return;
            }
            Err(err) => {
                view.update(cx, |this, cx| {
                    this.set_editor_operation(None, cx);
                    this.set_error(format!("Export dialog closed: {err}"));
                    cx.notify();
                })
                .ok();
                return;
            }
        };

        let Some(dir) = paths.into_iter().next() else {
            view.update(cx, |this, cx| {
                this.set_editor_operation(None, cx);
                this.set_error("No folder selected");
                cx.notify();
            })
            .ok();
            return;
        };

        let export_path = config_library.plan_export_path(&dir, &selected.name);
        let loaded_from_storage = initial_text.is_none();
        let initial_text = initial_text.map(|text| text.to_string());
        let storage_path = selected.storage_path.clone();
        let text_task = cx.background_spawn(async move {
            config_library.resolve_export_text(initial_text, &storage_path)
        });
        let text_result = text_task.await;

        let text = match text_result {
            Ok(text) => text,
            Err(message) => {
                view.update(cx, |this, cx| {
                    this.set_editor_operation(None, cx);
                    this.set_error(message);
                    cx.notify();
                })
                .ok();
                return;
            }
        };
        if loaded_from_storage {
            let path_for_cache = selected.storage_path.clone();
            let shared: SharedString = text.clone().into();
            view.update(cx, |this, _| {
                this.cache_config_text(path_for_cache, shared);
            })
            .ok();
        }

        let export_service = ConfigLibraryService::new();
        let write_task =
            cx.background_spawn(async move { export_service.export_config(&export_path, &text) });

        let result = write_task.await;
        view.update(cx, |this, cx| {
            this.set_editor_operation(None, cx);
            match result {
                Ok(path) => {
                    this.set_status(format!("Exported to {}", path.display()));
                }
                Err(err) => {
                    this.set_error(format!("Export failed: {err}"));
                }
            }
            cx.notify();
        })
        .ok();
    })
    .detach();
}

pub(crate) fn handle_paste_click(app: &mut WgApp, window: &mut Window, cx: &mut Context<WgApp>) {
    if app.configs_draft_snapshot(cx).is_dirty() {
        dialogs::confirm_discard_or_save(
            app,
            PendingDraftAction::Paste,
            window,
            cx,
            "Replace draft?",
            "Pasting a config will replace the current unsaved draft.",
        );
        return;
    }
    let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
        app.set_error("Clipboard is empty");
        cx.notify();
        return;
    };

    if let Err(err) = config::parse_config(&text) {
        app.set_error(format!("Invalid config: {err}"));
        cx.notify();
        return;
    }
    let text: SharedString = text.into();

    let suggested_name = app
        .configs_name_input(cx)
        .map(|input| input.read(cx).value().to_string())
        .unwrap_or_default();
    let suggested_name = suggested_name.trim();
    let name = if suggested_name.is_empty() {
        app.next_config_name("pasted")
    } else if app.configs.iter().any(|cfg| cfg.name == suggested_name) {
        app.next_config_name(suggested_name)
    } else {
        suggested_name.to_string()
    };

    app.set_selected_config_id(None, cx);
    app.selection.loading_config_id = None;
    app.selection.loading_config_path = None;
    app.selection.loaded_config = None;
    draft::set_unsaved_draft(app, name.clone().into(), text.clone(), cx);
    if let Some(name_input) = app.configs_name_input(cx) {
        name_input.update(cx, |input, cx| {
            input.set_value(name.clone(), window, cx);
        });
    }
    if let Some(config_input) = app.configs_config_input(cx) {
        config_input.update(cx, |input, cx| {
            input.set_value(text, window, cx);
        });
    }
    app.set_status("Pasted config into draft");
    cx.notify();
}

pub(crate) fn handle_copy_click(app: &mut WgApp, cx: &mut Context<WgApp>) {
    let Some(selected) = app.selected_config().cloned() else {
        app.set_error("Select a tunnel first");
        cx.notify();
        return;
    };
    let cached_text = app.cached_config_text(&selected.storage_path);
    let text = selected.text.clone().or(cached_text);
    if let Some(text) = text {
        cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
        app.set_status("Config copied to clipboard");
        cx.notify();
        return;
    }

    app.set_status("Loading config...");
    cx.notify();

    let config_library = app.config_library.clone();
    cx.spawn(async move |view, cx| {
        let path_for_cache = selected.storage_path.clone();
        let read_task = cx.background_spawn(async move {
            config_library.read_config_text(&selected.storage_path)
        });
        let result = read_task.await;
        view.update(cx, |this, cx| {
            if this.selection.selected_id != Some(selected.id) {
                return;
            }
            match result {
                Ok(text) => {
                    let text: SharedString = text.into();
                    this.cache_config_text(path_for_cache, text.clone());
                    cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
                    this.set_status("Config copied to clipboard");
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

async fn import_config(
    job: ImportConfigJob,
    config_library: ConfigLibraryService,
) -> ImportOutcome {
    let origin_path = job.origin_path.clone();
    match config_library.import_config_job(job) {
        Ok(imported) => build_import_outcome(imported).await,
        Err(err) => ImportOutcome::Err {
            path: origin_path,
            message: err,
        },
    }
}

async fn build_import_outcome(imported: ImportedConfigArtifact) -> ImportOutcome {
    let endpoint_family =
        endpoint_family::resolve_endpoint_family_from_text(imported.text.clone()).await;
    ImportOutcome::Ok {
        imported: ImportedConfigRecord {
            id: imported.id,
            name: imported.suggested_name,
            origin_path: imported.origin_path,
            storage_path: imported.storage_path,
            source: ConfigSourceKind::File,
        },
        endpoint_family,
    }
}

fn portal_missing_message(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("xdg-desktop-portal")
        || lower.contains("portal request failed")
        || lower.contains("org.freedesktop.portal")
        || lower.contains("portalnotfound")
        || lower.contains("portal not found")
}

fn pick_file_fallback(prompt: &str) -> Result<Option<Vec<PathBuf>>, String> {
    if let Some(paths) = pick_with_zenity(prompt)? {
        return Ok(Some(paths));
    }
    if let Some(paths) = pick_with_kdialog(prompt)? {
        return Ok(Some(paths));
    }
    Err("No file picker available (xdg-desktop-portal/zenity/kdialog)".to_string())
}

fn pick_with_zenity(prompt: &str) -> Result<Option<Vec<PathBuf>>, String> {
    let title = format!("--title={prompt}");
    pick_with_command(
        "zenity",
        &["--file-selection", "--multiple", "--separator=|", &title],
    )
}

fn pick_with_kdialog(prompt: &str) -> Result<Option<Vec<PathBuf>>, String> {
    let title = format!("--title={prompt}");
    pick_with_command(
        "kdialog",
        &[
            "--getopenfilename",
            ".",
            "--multiple",
            "--separate-output",
            &title,
        ],
    )
}

fn pick_with_command(command: &str, args: &[&str]) -> Result<Option<Vec<PathBuf>>, String> {
    let output = match Command::new(command).args(args).output() {
        Ok(output) => output,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(format!("{command} failed: {err}"));
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            return Ok(None);
        }
        return Err(format!("{command} failed: {stderr}"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let entries = stdout
        .lines()
        .flat_map(|line| line.split('|'))
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    if entries.is_empty() {
        Ok(None)
    } else {
        Ok(Some(entries))
    }
}

fn imported_config_source(source: ConfigSourceKind, origin_path: PathBuf) -> ConfigSource {
    match source {
        ConfigSourceKind::File => ConfigSource::File {
            origin_path: Some(origin_path),
        },
        ConfigSourceKind::Paste => ConfigSource::Paste,
    }
}
