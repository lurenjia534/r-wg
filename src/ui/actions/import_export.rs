use std::collections::HashSet;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::process::Command;

use futures_util::stream::{FuturesUnordered, StreamExt};
use gpui::{AppContext, Context, PathPromptOptions, SharedString, Window};
use r_wg::backend::wg::config;

use super::super::format::{name_from_path, sanitize_file_stem};
use super::super::persistence;
use super::super::state::{
    ConfigSource, EditorOperation, EndpointFamily, PendingDraftAction, TunnelConfig, WgApp,
};
use super::config::resolve_endpoint_family_from_text;

struct ImportJob {
    id: u64,
    origin_path: PathBuf,
    storage_path: PathBuf,
}

enum ImportOutcome {
    Ok {
        id: u64,
        name: String,
        origin_path: PathBuf,
        storage_path: PathBuf,
        endpoint_family: EndpointFamily,
    },
    Err {
        path: PathBuf,
        message: String,
    },
}

const IMPORT_CONCURRENCY: usize = 8;
const IMPORT_BATCH_SIZE: usize = 200;

impl WgApp {
    /// 点击导入按钮后的处理。
    ///
    /// 说明：先弹文件选择对话框，再在后台读取文件与解析，UI 线程只做状态更新。
    pub(crate) fn handle_import_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.configs_is_busy(cx) {
            return;
        }
        if self.configs_draft_snapshot(cx).is_dirty() {
            self.confirm_discard_or_save(
                PendingDraftAction::Import,
                window,
                cx,
                "Import configs?",
                "Importing will replace the current unsaved draft context.",
            );
            return;
        }
        // 弹出文件选择并交给后台读取。
        self.set_status("Opening file dialog...");
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
                                        this.start_import_from_paths(paths, window, cx);
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
                    this.start_import_from_paths(paths, window, cx);
                })
                .ok();
            })
            .detach();
    }

    /// 从多个路径导入配置。
    ///
    /// 说明：读取与解析放到后台线程，完成后再回到 UI 线程写入模型并更新状态。
    pub(crate) fn start_import_from_paths(
        &mut self,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if paths.is_empty() {
            self.set_error("No file selected");
            cx.notify();
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

        let mut jobs = Vec::with_capacity(paths.len());
        for path in paths {
            let id = self.configs.alloc_config_id();
            let storage_path = persistence::config_path(&storage, id);
            jobs.push(ImportJob {
                id,
                origin_path: path,
                storage_path,
            });
        }

        // 记录总数，用于状态提示与批量导入节奏控制。
        let total = jobs.len();
        self.set_editor_operation(
            Some(EditorOperation::Importing {
                processed: 0,
                total,
            }),
            cx,
        );
        self.set_status(format!("Loading {total} files..."));
        cx.notify();

        let view = cx.weak_entity();
        window
            .spawn(cx, async move |cx| {
                // 在 UI 线程里收集已有名称，避免重名。
                let mut names_in_use = match view.update(cx, |this, _| {
                    this.configs
                        .iter()
                        .map(|cfg| cfg.name.clone())
                        .collect::<HashSet<_>>()
                }) {
                    Ok(names) => names,
                    Err(_) => return,
                };

                let mut processed = 0usize;
                let mut imported = 0usize;
                let mut failed = 0usize;
                let mut last_error = None;
                let mut outcomes_batch = Vec::new();

                // 并行读/解析：限制并发，避免同时打开过多文件。
                let concurrency = IMPORT_CONCURRENCY.min(total.max(1));
                let mut pending = jobs.into_iter();
                let mut tasks = FuturesUnordered::new();
                for _ in 0..concurrency {
                    if let Some(job) = pending.next() {
                        tasks.push(cx.background_spawn(async move { read_config(job).await }));
                    }
                }

                while let Some(outcome) = tasks.next().await {
                    // 先在后台线程完成解析，UI 线程只接收结果。
                    let outcome = match outcome {
                        ImportOutcome::Ok {
                            id,
                            name,
                            origin_path,
                            storage_path,
                            endpoint_family,
                        } => {
                            let name = unique_name(&mut names_in_use, &name);
                            imported += 1;
                            ImportOutcome::Ok {
                                id,
                                name,
                                origin_path,
                                storage_path,
                                endpoint_family,
                            }
                        }
                        ImportOutcome::Err { path, message } => {
                            failed += 1;
                            last_error = Some(format!("{message} ({})", path.display()));
                            ImportOutcome::Err { path, message }
                        }
                    };

                    processed += 1;
                    outcomes_batch.push(outcome);

                    if let Some(job) = pending.next() {
                        tasks.push(cx.background_spawn(async move { read_config(job).await }));
                    }

                    // 批量提交：减少 UI 线程频繁更新带来的卡顿。
                    if outcomes_batch.len() >= IMPORT_BATCH_SIZE || processed == total {
                        let outcomes = std::mem::take(&mut outcomes_batch);
                        view.update_in(cx, |this, _window, cx| {
                            let mut imported_configs = Vec::new();
                            for outcome in outcomes {
                                if let ImportOutcome::Ok {
                                    id,
                                    name,
                                    origin_path,
                                    storage_path,
                                    endpoint_family,
                                } = outcome
                                {
                                    imported_configs.push(TunnelConfig {
                                        id,
                                        name_lower: name.to_lowercase(),
                                        name,
                                        text: None,
                                        source: ConfigSource::File {
                                            origin_path: Some(origin_path),
                                        },
                                        storage_path,
                                        endpoint_family,
                                    });
                                }
                            }
                            if !imported_configs.is_empty() {
                                this.configs.extend(imported_configs.iter().cloned());
                                this.append_configs_workspace_library_rows(&imported_configs, cx);
                            }
                            // 使用状态文本作为轻量“进度提示”。
                            this.set_editor_operation(
                                Some(EditorOperation::Importing { processed, total }),
                                cx,
                            );
                            this.set_status(format!("Importing {processed}/{total}..."));
                            cx.notify();
                        })
                        .ok();
                    }
                }

                view.update_in(cx, |this, window, cx| {
                    this.set_editor_operation(None, cx);
                    if imported > 0 {
                        let selected_id = this.configs.last().map(|config| config.id);
                        this.set_selected_config_id(selected_id, cx);
                        if let Some(config_id) = selected_id {
                            this.load_config_into_inputs(config_id, window, cx);
                        }
                    }
                    // 导入结束后给出总结提示（成功/失败数）。
                    if imported == 0 && failed > 0 {
                        this.set_error(last_error.unwrap_or_else(|| "Import failed".to_string()));
                    } else if failed > 0 {
                        this.set_status(format!("Imported {imported} configs, {failed} failed"));
                    } else {
                        this.set_status(format!("Imported {imported} configs"));
                    }
                    if imported > 0 {
                        this.persist_state_async(cx);
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
    }

    /// 导出当前选中的配置到指定目录。
    ///
    /// 说明：写文件放在后台执行，UI 线程只负责提示与状态更新。
    pub(crate) fn handle_export_click(&mut self, cx: &mut Context<Self>) {
        if self.configs_is_busy(cx) {
            return;
        }
        // 选择导出目录后写入 *.conf。
        let Some(selected) = self.selected_config().cloned() else {
            self.set_error("Select a tunnel first");
            cx.notify();
            return;
        };
        let cached_text = self.cached_config_text(&selected.storage_path);
        let initial_text = selected.text.clone().or(cached_text);

        self.set_editor_operation(Some(EditorOperation::Exporting), cx);
        self.set_status("Choose export folder...");
        cx.notify();

        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Export WireGuard Config".into()),
        });

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

            let filename = format!("{}.conf", sanitize_file_stem(&selected.name));
            let export_path = dir.join(filename);
            let text_result = match initial_text {
                Some(text) => Ok(text.to_string()),
                None => {
                    let path_for_cache = selected.storage_path.clone();
                    let read_task = cx.background_spawn(async move {
                        std::fs::read_to_string(&selected.storage_path)
                    });
                    match read_task.await {
                        Ok(text) => {
                            let shared: SharedString = text.clone().into();
                            view.update(cx, |this, _| {
                                this.cache_config_text(path_for_cache, shared);
                            })
                            .ok();
                            Ok(text)
                        }
                        Err(err) => Err(format!("Read failed: {err}")),
                    }
                }
            };

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

            let write_task = cx.background_spawn(async move {
                std::fs::write(&export_path, text)?;
                Ok::<_, std::io::Error>(export_path)
            });

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
}

async fn read_config(job: ImportJob) -> ImportOutcome {
    // 后台线程读取 + 解析，返回结果给 UI 线程消费。
    let name = name_from_path(&job.origin_path);
    match std::fs::read_to_string(&job.origin_path) {
        Ok(text) => {
            if let Err(err) = config::parse_config(&text) {
                ImportOutcome::Err {
                    path: job.origin_path,
                    message: format!("Invalid config: {err}"),
                }
            } else {
                let endpoint_family = resolve_endpoint_family_from_text(text.clone()).await;
                match persistence::write_config_text(&job.storage_path, &text) {
                    Ok(()) => ImportOutcome::Ok {
                        id: job.id,
                        name,
                        origin_path: job.origin_path,
                        storage_path: job.storage_path,
                        endpoint_family,
                    },
                    Err(message) => ImportOutcome::Err {
                        path: job.origin_path,
                        message,
                    },
                }
            }
        }
        Err(err) => ImportOutcome::Err {
            path: job.origin_path,
            message: format!("Read failed: {err}"),
        },
    }
}

fn unique_name(names_in_use: &mut HashSet<String>, base: &str) -> String {
    // 生成唯一名称，避免导入时覆盖已有配置。
    if !names_in_use.contains(base) {
        names_in_use.insert(base.to_string());
        return base.to_string();
    }
    for idx in 2..1000 {
        let candidate = format!("{base}-{idx}");
        if names_in_use.insert(candidate.clone()) {
            return candidate;
        }
    }
    let candidate = format!("{base}-{}", names_in_use.len() + 1);
    names_in_use.insert(candidate.clone());
    candidate
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
    // 先尝试 zenity，再尝试 kdialog，避免强依赖某一种桌面环境。
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
    let raw = stdout.trim();
    if raw.is_empty() {
        return Ok(None);
    }

    let parts: Vec<&str> = if raw.contains('\n') {
        raw.lines().collect()
    } else if raw.contains('|') {
        raw.split('|').collect()
    } else {
        vec![raw]
    };

    let paths: Vec<PathBuf> = parts
        .into_iter()
        .filter(|part| !part.trim().is_empty())
        .map(PathBuf::from)
        .collect();

    if paths.is_empty() {
        return Ok(None);
    }
    Ok(Some(paths))
}
