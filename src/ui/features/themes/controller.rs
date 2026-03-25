use std::io::ErrorKind;
use std::path::PathBuf;
use std::process::Command;

use gpui::{AppContext, Context, PathPromptOptions, Window};
use gpui_component::notification::Notification;
use gpui_component::theme::ThemeMode;
use gpui_component::{ActiveTheme as _, WindowExt};

use crate::ui::format::sanitize_file_stem;
use crate::ui::state::WgApp;

use super::{
    build_theme_template, ensure_themes_dir, import_theme_file, lint_theme_config, lint_theme_set,
    resolve_theme_config, restore_curated_themes, sanitize_theme_set_with_inventory,
    theme_name_inventory, write_theme_set, ThemeLintCounts,
};

struct ImportedThemeFile {
    path: PathBuf,
    lint_counts: ThemeLintCounts,
}

struct ThemeImportSummary {
    imported: Vec<ImportedThemeFile>,
    failed: Vec<(PathBuf, String)>,
}

impl WgApp {
    pub(crate) fn open_themes_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let storage = match self.configs.ensure_storage() {
            Ok(storage) => storage,
            Err(err) => {
                self.set_error(err);
                cx.notify();
                return;
            }
        };

        match ensure_themes_dir(&storage) {
            Ok(themes_dir) => {
                cx.reveal_path(&themes_dir);
                self.set_status(format!("Opened themes folder: {}", themes_dir.display()));
                self.push_success_toast("Themes folder opened", window, cx);
            }
            Err(err) => {
                self.set_error(err.clone());
                window.push_notification(Notification::error(err), cx);
            }
        }
        cx.notify();
    }

    pub(crate) fn handle_theme_import_click(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let storage = match self.configs.ensure_storage() {
            Ok(storage) => storage,
            Err(err) => {
                self.set_error(err);
                cx.notify();
                return;
            }
        };

        self.set_status("Opening theme import dialog...");
        cx.notify();

        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Import Theme JSON".into()),
        });
        let names_in_use = theme_name_inventory(Some(&storage), cx);

        let view = cx.weak_entity();
        window
            .spawn(cx, async move |cx| {
                let paths = match prompt.await {
                    Ok(Ok(Some(paths))) => paths,
                    Ok(Ok(None)) => {
                        view.update(cx, |this, cx| {
                            this.set_status("Theme import canceled");
                            cx.notify();
                        })
                        .ok();
                        return;
                    }
                    Ok(Err(err)) => {
                        let message = err.to_string();
                        if portal_missing_message(&message) {
                            view.update(cx, |this, cx| {
                                this.set_status("Theme picker unavailable, trying fallback...");
                                cx.notify();
                            })
                            .ok();

                            let fallback = cx
                                .background_spawn(async move {
                                    pick_theme_files_fallback("Import Theme JSON")
                                })
                                .await;

                            match fallback {
                                Ok(Some(paths)) => paths,
                                Ok(None) => {
                                    view.update(cx, |this, cx| {
                                        this.set_status("Theme import canceled");
                                        cx.notify();
                                    })
                                    .ok();
                                    return;
                                }
                                Err(err) => {
                                    view.update_in(cx, |this, window, cx| {
                                        this.set_error(err.clone());
                                        window.push_notification(Notification::error(err), cx);
                                        cx.notify();
                                    })
                                    .ok();
                                    return;
                                }
                            }
                        } else {
                            view.update_in(cx, |this, window, cx| {
                                let message = format!("Theme dialog failed: {message}");
                                this.set_error(message.clone());
                                window.push_notification(Notification::error(message), cx);
                                cx.notify();
                            })
                            .ok();
                            return;
                        }
                    }
                    Err(err) => {
                        view.update(cx, |this, cx| {
                            this.set_error(format!("Theme dialog closed: {err}"));
                            cx.notify();
                        })
                        .ok();
                        return;
                    }
                };

                let paths: Vec<PathBuf> = paths.into_iter().collect();
                if paths.is_empty() {
                    view.update(cx, |this, cx| {
                        this.set_status("Theme import canceled");
                        cx.notify();
                    })
                    .ok();
                    return;
                }

                view.update(cx, |this, cx| {
                    this.set_status(format!("Importing {} theme files...", paths.len()));
                    cx.notify();
                })
                .ok();

                let import_task = cx.background_spawn(async move {
                    import_theme_files(paths, storage, names_in_use).await
                });
                let summary = import_task.await;

                view.update_in(cx, |this, window, cx| {
                    finish_theme_import(this, summary, window, cx);
                })
                .ok();
            })
            .detach();
    }

    pub(crate) fn duplicate_current_theme_template(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let storage = match self.configs.ensure_storage() {
            Ok(storage) => storage,
            Err(err) => {
                self.set_error(err);
                cx.notify();
                return;
            }
        };

        let light_theme = resolve_theme_config(
            ThemeMode::Light,
            self.ui_prefs.theme_light_key.as_deref().map(|key| &**key),
            self.ui_prefs
                .theme_light_name
                .as_deref()
                .map(|name| &**name),
            Some(&storage),
            cx,
        );
        let dark_theme = resolve_theme_config(
            ThemeMode::Dark,
            self.ui_prefs.theme_dark_key.as_deref().map(|key| &**key),
            self.ui_prefs.theme_dark_name.as_deref().map(|name| &**name),
            Some(&storage),
            cx,
        );
        let template = build_theme_template(light_theme.as_ref(), dark_theme.as_ref());
        let mut names_in_use = theme_name_inventory(Some(&storage), cx);
        let template = sanitize_theme_set_with_inventory(template, &mut names_in_use);
        let lint_counts = lint_theme_set(&template);
        let file_stem = sanitize_file_stem(&format!("{}-template", cx.theme().theme_name()));

        self.set_status("Writing theme template...");
        cx.notify();

        let view = cx.weak_entity();
        window
            .spawn(cx, async move |cx| {
                let write_task =
                    cx.background_spawn(
                        async move { write_theme_set(&storage, &file_stem, &template) },
                    );
                let result = write_task.await;

                view.update_in(cx, |this, window, cx| {
                    match result {
                        Ok(path) => {
                            cx.reveal_path(&path);
                            let file_name = path
                                .file_name()
                                .and_then(|name| name.to_str())
                                .unwrap_or("theme-template.json");
                            let message = format!(
                                "Created theme template: {file_name} · {}",
                                lint_counts.summary_label()
                            );
                            this.set_status(message.clone());
                            this.push_success_toast(message, window, cx);
                        }
                        Err(err) => {
                            this.set_error(err.clone());
                            window.push_notification(Notification::error(err), cx);
                        }
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
    }

    pub(crate) fn restore_curated_theme_files(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let storage = match self.configs.ensure_storage() {
            Ok(storage) => storage,
            Err(err) => {
                self.set_error(err);
                cx.notify();
                return;
            }
        };

        self.set_status("Restoring curated themes...");
        cx.notify();

        let view = cx.weak_entity();
        window
            .spawn(cx, async move |cx| {
                let lint_storage = storage.clone();
                let restore_task =
                    cx.background_spawn(async move { restore_curated_themes(&storage) });
                let result = restore_task.await;

                view.update_in(cx, |this, window, cx| {
                    match result {
                        Ok(themes_dir) => {
                            let lint_counts =
                                lint_counts_for_active_palettes(this, &lint_storage, cx);
                            let message = format!(
                                "Curated themes restored · active palettes: {}",
                                lint_counts.summary_label()
                            );
                            this.set_status(message.clone());
                            this.push_success_toast(message, window, cx);
                            cx.reveal_path(&themes_dir);
                        }
                        Err(err) => {
                            this.set_error(err.clone());
                            window.push_notification(Notification::error(err), cx);
                        }
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
    }
}

async fn import_theme_files(
    paths: Vec<PathBuf>,
    storage: crate::ui::persistence::StoragePaths,
    mut names_in_use: std::collections::HashSet<String>,
) -> ThemeImportSummary {
    let mut imported = Vec::new();
    let mut failed = Vec::new();

    for path in paths {
        match import_theme_file(&path, &storage, &mut names_in_use) {
            Ok(imported_theme) => imported.push(ImportedThemeFile {
                path: imported_theme.path,
                lint_counts: lint_theme_set(&imported_theme.theme_set),
            }),
            Err(err) => failed.push((path, err)),
        }
    }

    ThemeImportSummary { imported, failed }
}

fn finish_theme_import(
    app: &mut WgApp,
    summary: ThemeImportSummary,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) {
    let imported_count = summary.imported.len();
    let failed_count = summary.failed.len();
    let lint_counts =
        summary
            .imported
            .iter()
            .fold(ThemeLintCounts::default(), |mut counts, imported| {
                counts.add_counts(imported.lint_counts);
                counts
            });

    if imported_count > 0 {
        if let Some(first_import) = summary.imported.first() {
            cx.reveal_path(&first_import.path);
        }
        let message = if failed_count == 0 {
            format!(
                "Imported {imported_count} theme file(s) · {}",
                lint_counts.summary_label()
            )
        } else {
            format!(
                "Imported {imported_count} theme file(s), {failed_count} failed · {}",
                lint_counts.summary_label()
            )
        };
        app.set_status(message.clone());
        app.push_success_toast(message, window, cx);
    } else if let Some((_, err)) = summary.failed.first() {
        app.set_error(err.clone());
        window.push_notification(Notification::error(err.clone()), cx);
    } else {
        app.set_status("Theme import canceled");
    }

    if failed_count > 0 {
        let detail = if let Some((path, err)) = summary.failed.first() {
            format!("Theme import failed for {}: {err}", path.display())
        } else {
            format!("{failed_count} theme files failed to import")
        };
        app.set_error(detail.clone());
        window.push_notification(Notification::error(detail), cx);
    }

    cx.notify();
}

fn lint_counts_for_active_palettes(
    app: &WgApp,
    storage: &crate::ui::persistence::StoragePaths,
    cx: &Context<WgApp>,
) -> ThemeLintCounts {
    let light_theme = resolve_theme_config(
        ThemeMode::Light,
        app.ui_prefs.theme_light_key.as_deref().map(|key| &**key),
        app.ui_prefs.theme_light_name.as_deref().map(|name| &**name),
        Some(storage),
        cx,
    );
    let dark_theme = resolve_theme_config(
        ThemeMode::Dark,
        app.ui_prefs.theme_dark_key.as_deref().map(|key| &**key),
        app.ui_prefs.theme_dark_name.as_deref().map(|name| &**name),
        Some(storage),
        cx,
    );

    let mut counts = ThemeLintCounts::default();
    let light_items = lint_theme_config(light_theme.as_ref());
    counts.add_items(light_items.iter());
    let dark_items = lint_theme_config(dark_theme.as_ref());
    counts.add_items(dark_items.iter());
    counts
}

fn portal_missing_message(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("xdg-desktop-portal")
        || lower.contains("portal request failed")
        || lower.contains("org.freedesktop.portal")
        || lower.contains("portalnotfound")
        || lower.contains("portal not found")
}

fn pick_theme_files_fallback(prompt: &str) -> Result<Option<Vec<PathBuf>>, String> {
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
            "*.json",
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
        Err(err) => return Err(format!("{command} failed: {err}")),
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

    let paths = parts
        .into_iter()
        .filter(|part| !part.trim().is_empty())
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    if paths.is_empty() {
        return Ok(None);
    }

    Ok(Some(paths))
}
