use std::path::PathBuf;
use std::process::Command;
use std::io::ErrorKind;

use gpui::{AppContext, Context, PathPromptOptions, Window};
use r_wg::backend::wg::config;

use super::super::format::{name_from_path, sanitize_file_stem};
use super::super::state::{ConfigSource, TunnelConfig, WgApp};

impl WgApp {
    /// 点击导入按钮后的处理。
    ///
    /// 说明：先弹文件选择对话框，再在后台读取文件与解析，UI 线程只做状态更新。
    pub(crate) fn handle_import_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // 弹出文件选择并交给后台读取。
        self.set_status("Opening file dialog...");
        cx.notify();

        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import WireGuard Config".into()),
        });

        let view = cx.weak_entity();
        window
            .spawn(cx, async move |cx| {
                let paths = match prompt.await {
                    Ok(Ok(Some(paths))) => paths,
                    Ok(Ok(None)) => {
                        view.update(cx, |this, cx| {
                            this.busy = false;
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
                                Ok(Some(path)) => {
                                    view.update_in(cx, |this, window, cx| {
                                        this.start_import_from_path(path, window, cx);
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

                let Some(path) = paths.into_iter().next() else {
                    view.update(cx, |this, cx| {
                        this.set_error("No file selected");
                        cx.notify();
                    })
                    .ok();
                    return;
                };

                view.update_in(cx, |this, window, cx| {
                    this.start_import_from_path(path, window, cx);
                })
                .ok();
            })
            .detach();
    }

    /// 从指定路径导入配置。
    ///
    /// 说明：读取与解析放到后台线程，完成后再回到 UI 线程写入模型并更新状态。
    pub(crate) fn start_import_from_path(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 从给定路径导入配置，读取与解析都在后台执行。
        self.busy = true;
        self.set_status(format!("Loading {}", path.display()));
        cx.notify();

        let view = cx.weak_entity();
        window
            .spawn(cx, async move |cx| {
                let read_task = cx.background_spawn(async move {
                    let name = name_from_path(&path);
                    let text = std::fs::read_to_string(&path)?;
                    Ok::<_, std::io::Error>((name, text, path))
                });

                let read_result = read_task.await;
                match read_result {
                    Ok((name, text, path)) => {
                        // 先校验配置格式，失败时不写入列表。
                        if let Err(err) = config::parse_config(&text) {
                            view.update(cx, |this, cx| {
                                this.busy = false;
                                this.set_error(format!("Invalid config: {err}"));
                                cx.notify();
                            })
                            .ok();
                            return;
                        }

                        view.update_in(cx, |this, window, cx| {
                            this.busy = false;
                            this.upsert_config(
                                TunnelConfig {
                                    name: name.clone(),
                                    text,
                                    source: ConfigSource::File(path),
                                },
                                window,
                                cx,
                            );
                            this.set_status(format!("Imported {name}"));
                            cx.notify();
                        })
                        .ok();
                    }
                    Err(err) => {
                        view.update(cx, |this, cx| {
                            this.busy = false;
                            this.set_error(format!("Read failed: {err}"));
                            cx.notify();
                        })
                        .ok();
                    }
                }
            })
            .detach();
    }

    /// 导出当前选中的配置到指定目录。
    ///
    /// 说明：写文件放在后台执行，UI 线程只负责提示与状态更新。
    pub(crate) fn handle_export_click(&mut self, cx: &mut Context<Self>) {
        // 选择导出目录后写入 *.conf。
        let Some(selected) = self.selected_config().cloned() else {
            self.set_error("Select a tunnel first");
            cx.notify();
            return;
        };

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
                        this.set_status("Export canceled");
                        cx.notify();
                    })
                    .ok();
                    return;
                }
                Ok(Err(err)) => {
                    view.update(cx, |this, cx| {
                        this.set_error(format!("Export dialog failed: {err}"));
                        cx.notify();
                    })
                    .ok();
                    return;
                }
                Err(err) => {
                    view.update(cx, |this, cx| {
                        this.set_error(format!("Export dialog closed: {err}"));
                        cx.notify();
                    })
                    .ok();
                    return;
                }
            };

            let Some(dir) = paths.into_iter().next() else {
                view.update(cx, |this, cx| {
                    this.set_error("No folder selected");
                    cx.notify();
                })
                .ok();
                return;
            };

            let filename = format!("{}.conf", sanitize_file_stem(&selected.name));
            let export_path = dir.join(filename);
            let text = selected.text.clone();

            let write_task = cx.background_spawn(async move {
                std::fs::write(&export_path, text)?;
                Ok::<_, std::io::Error>(export_path)
            });

            let result = write_task.await;
            view.update(cx, |this, cx| {
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

fn portal_missing_message(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("xdg-desktop-portal")
        || lower.contains("portal request failed")
        || lower.contains("org.freedesktop.portal")
        || lower.contains("portalnotfound")
        || lower.contains("portal not found")
}

fn pick_file_fallback(prompt: &str) -> Result<Option<PathBuf>, String> {
    // 先尝试 zenity，再尝试 kdialog，避免强依赖某一种桌面环境。
    if let Some(path) = pick_with_zenity(prompt)? {
        return Ok(Some(path));
    }
    if let Some(path) = pick_with_kdialog(prompt)? {
        return Ok(Some(path));
    }
    Err("No file picker available (xdg-desktop-portal/zenity/kdialog)".to_string())
}

fn pick_with_zenity(prompt: &str) -> Result<Option<PathBuf>, String> {
    let title = format!("--title={prompt}");
    pick_with_command("zenity", &["--file-selection", &title])
}

fn pick_with_kdialog(prompt: &str) -> Result<Option<PathBuf>, String> {
    let title = format!("--title={prompt}");
    pick_with_command("kdialog", &["--getopenfilename", ".", &title])
}

fn pick_with_command(command: &str, args: &[&str]) -> Result<Option<PathBuf>, String> {
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

    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        return Ok(None);
    }
    Ok(Some(PathBuf::from(path)))
}
