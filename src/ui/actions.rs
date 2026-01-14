use std::path::PathBuf;
use std::time::Duration;

use gpui::{AppContext, ClipboardItem, Context, PathPromptOptions, SharedString, Window};
use gpui_component::input::InputState;
use r_wg::backend::wg::{config, EngineStats, StartRequest};

use super::format::{name_from_path, sanitize_file_stem};
use super::permissions::start_permission_message;
use super::state::{ConfigSource, TunnelConfig, WgApp};

impl WgApp {
    /// 初始化输入控件（按需创建，避免在构造阶段依赖窗口）。
    pub(crate) fn ensure_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.name_input.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Tunnel name"));
            self.name_input = Some(input);
        }

        if self.config_input.is_none() {
            let placeholder = "[Interface]\nPrivateKey = ...\nAddress = 10.0.0.2/32\n\n[Peer]\nPublicKey = ...\nAllowedIPs = 0.0.0.0/0\nEndpoint = example.com:51820";
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .multi_line(true)
                    .rows(16)
                    .placeholder(placeholder)
            });
            self.config_input = Some(input);
        }
    }

    pub(crate) fn upsert_config(
        &mut self,
        config: TunnelConfig,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 同名配置覆盖，不存在则追加。
        let idx = match self
            .configs
            .iter()
            .position(|entry| entry.name == config.name)
        {
            Some(idx) => {
                self.configs[idx] = config;
                idx
            }
            None => {
                self.configs.push(config);
                self.configs.len() - 1
            }
        };

        self.selected = Some(idx);
        self.load_config_into_inputs(idx, window, cx);
    }

    pub(crate) fn load_config_into_inputs(
        &mut self,
        idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 将模型数据灌入输入控件。
        self.ensure_inputs(window, cx);

        let Some(name_input) = self.name_input.as_ref() else {
            return;
        };
        let Some(config_input) = self.config_input.as_ref() else {
            return;
        };

        let config = &self.configs[idx];
        name_input.update(cx, |input, cx| {
            input.set_value(config.name.clone(), window, cx);
        });
        config_input.update(cx, |input, cx| {
            input.set_value(config.text.clone(), window, cx);
        });
    }

    pub(crate) fn start_stats_polling(&mut self, cx: &mut Context<Self>) {
        // 每次启动使用新的 generation，停止后自动中断旧轮询。
        self.stats_generation = self.stats_generation.wrapping_add(1);
        let generation = self.stats_generation;
        let engine = self.engine.clone();
        let poll_interval = Duration::from_secs(2);

        // 异步轮询 peer 统计，避免阻塞 UI。
        cx.spawn(async move |view, cx| {
            loop {
                cx.background_executor().timer(poll_interval).await;
                let engine = engine.clone();
                let result = cx.background_spawn(async move { engine.stats() }).await;

                let continue_polling = view
                    .update(cx, |this, cx| {
                        if !this.running || this.stats_generation != generation {
                            return false;
                        }

                        match result {
                            Ok(stats) => this.apply_stats(stats),
                            Err(err) => {
                                this.stats_note = format!("Stats failed: {err}").into();
                            }
                        }
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);

                if !continue_polling {
                    break;
                }
            }
        })
        .detach();
    }

    pub(crate) fn apply_stats(&mut self, stats: EngineStats) {
        // 聚合统计，用于右侧状态卡片展示。
        self.peer_stats = stats.peers;
        if self.peer_stats.is_empty() {
            self.stats_note = "No peers reported".into();
        } else {
            self.stats_note = format!("Peers: {}", self.peer_stats.len()).into();
        }
    }

    /// 清空统计，通常用于停止隧道后。
    pub(crate) fn clear_stats(&mut self) {
        self.peer_stats.clear();
        self.stats_note = "Peer stats unavailable".into();
    }

    /// 选中指定隧道并加载到输入框。
    pub(crate) fn select_tunnel(
        &mut self,
        idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected = Some(idx);
        self.load_config_into_inputs(idx, window, cx);
        self.set_status("Loaded tunnel");
        cx.notify();
    }

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
                        view.update(cx, |this, cx| {
                            this.set_error(format!("File dialog failed: {err}"));
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

    pub(crate) fn handle_paste_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // 从剪贴板读取配置文本并校验。
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            self.set_error("Clipboard is empty");
            cx.notify();
            return;
        };

        if let Err(err) = config::parse_config(&text) {
            self.set_error(format!("Invalid config: {err}"));
            cx.notify();
            return;
        }

        let name = self
            .name_input
            .as_ref()
            .map(|input| input.read(cx).value().to_string())
            .unwrap_or_default();
        let name = name.trim();
        let name = if name.is_empty() {
            self.next_config_name("pasted")
        } else {
            name.to_string()
        };

        self.upsert_config(
            TunnelConfig {
                name: name.clone(),
                text,
                source: ConfigSource::Paste,
            },
            window,
            cx,
        );
        self.set_status(format!("Pasted {name}"));
        cx.notify();
    }

    pub(crate) fn handle_save_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // 读取输入框并写回配置列表。
        self.ensure_inputs(window, cx);
        let Some(name_input) = self.name_input.as_ref() else {
            self.set_error("Name input not ready");
            cx.notify();
            return;
        };
        let Some(config_input) = self.config_input.as_ref() else {
            self.set_error("Config input not ready");
            cx.notify();
            return;
        };

        let name = name_input.read(cx).value().to_string();
        let name = name.trim();
        if name.is_empty() {
            self.set_error("Tunnel name is required");
            cx.notify();
            return;
        }

        let text = config_input.read(cx).value().to_string();
        if text.trim().is_empty() {
            self.set_error("Config text is required");
            cx.notify();
            return;
        }

        if let Err(err) = config::parse_config(&text) {
            self.set_error(format!("Invalid config: {err}"));
            cx.notify();
            return;
        }

        if self.configs.iter().any(|entry| {
            entry.name == name && self.selected_config().map(|cfg| cfg.name.as_str()) != Some(name)
        }) {
            self.set_error("Tunnel name already exists");
            cx.notify();
            return;
        }

        let source = self
            .selected_config()
            .filter(|cfg| cfg.name == name)
            .map(|cfg| cfg.source.clone())
            .unwrap_or(ConfigSource::Paste);

        self.upsert_config(
            TunnelConfig {
                name: name.to_string(),
                text,
                source,
            },
            window,
            cx,
        );
        self.set_status("Saved tunnel");
        cx.notify();
    }

    pub(crate) fn handle_rename_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // 仅更新名称，不修改配置文本。
        self.ensure_inputs(window, cx);
        let Some(name_input) = self.name_input.as_ref() else {
            self.set_error("Name input not ready");
            cx.notify();
            return;
        };
        let new_name = name_input.read(cx).value().to_string();
        let new_name = new_name.trim();
        if new_name.is_empty() {
            self.set_error("Tunnel name is required");
            cx.notify();
            return;
        }

        let Some(idx) = self.selected else {
            self.set_error("Select a tunnel first");
            cx.notify();
            return;
        };
        let old_name = self.configs[idx].name.clone();
        if old_name == new_name {
            self.set_status("Name unchanged");
            cx.notify();
            return;
        }
        if self.configs.iter().any(|cfg| cfg.name == new_name) {
            self.set_error("Tunnel name already exists");
            cx.notify();
            return;
        }

        self.configs[idx].name = new_name.to_string();
        if self.running_name.as_deref() == Some(old_name.as_str()) {
            self.running_name = Some(new_name.to_string());
        }
        self.set_status(format!("Renamed to {new_name}"));
        self.load_config_into_inputs(idx, window, cx);
        cx.notify();
    }

    pub(crate) fn handle_delete_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // 运行中的隧道不允许删除。
        let Some(idx) = self.selected else {
            self.set_error("Select a tunnel first");
            cx.notify();
            return;
        };
        let name = self.configs[idx].name.clone();
        if self.running_name.as_deref() == Some(name.as_str()) {
            self.set_error("Stop the tunnel before deleting");
            cx.notify();
            return;
        }

        self.configs.remove(idx);
        if self.configs.is_empty() {
            self.selected = None;
            self.clear_inputs(window, cx);
        } else {
            let next_idx = idx.saturating_sub(1).min(self.configs.len() - 1);
            self.selected = Some(next_idx);
            self.load_config_into_inputs(next_idx, window, cx);
        }
        self.set_status(format!("Deleted {name}"));
        cx.notify();
    }

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

    pub(crate) fn handle_copy_click(&mut self, cx: &mut Context<Self>) {
        // 直接复制配置文本到剪贴板。
        let Some(selected) = self.selected_config() else {
            self.set_error("Select a tunnel first");
            cx.notify();
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(selected.text.clone()));
        self.set_status("Config copied to clipboard");
        cx.notify();
    }

    pub(crate) fn handle_start_stop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // 根据运行状态决定 start/stop。
        if self.running {
            self.busy = true;
            self.set_status("Stopping...");
            cx.notify();

            let engine = self.engine.clone();
            let view = cx.weak_entity();
            window
                .spawn(cx, async move |cx| {
                    let stop_task = cx.background_spawn(async move { engine.stop() });
                    let result = stop_task.await;
                    view.update(cx, |this, cx| {
                        this.busy = false;
                        match result {
                            Ok(()) => {
                                this.running = false;
                                this.running_name = None;
                                this.set_status("Stopped");
                                this.clear_stats();
                            }
                            Err(err) => {
                                this.set_error(format!("Stop failed: {err}"));
                            }
                        }
                        cx.notify();
                    })
                    .ok();
                })
                .detach();
            return;
        }

        let Some(selected) = self.selected_config().cloned() else {
            self.set_error("Select a tunnel first");
            cx.notify();
            return;
        };

        if let Some(message) = start_permission_message() {
            // 运行前检查权限提示（Linux cap_net_admin）。
            self.set_error(message);
            cx.notify();
            return;
        }

        self.busy = true;
        self.set_status(format!("Starting {}...", selected.name));
        cx.notify();

        let engine = self.engine.clone();
        let view = cx.weak_entity();
        window
            .spawn(cx, async move |cx| {
                let request = StartRequest::new(selected.name.clone(), selected.text.clone());
                let start_task = cx.background_spawn(async move { engine.start(request) });
                let result = start_task.await;
                view.update(cx, |this, cx| {
                    this.busy = false;
                    match result {
                        Ok(()) => {
                            this.running = true;
                            this.running_name = Some(selected.name.clone());
                            this.set_status(format!("Running {}", selected.name));
                            this.stats_note = "Fetching peer stats...".into();
                            // 启动成功后开始轮询统计。
                            this.start_stats_polling(cx);
                        }
                        Err(err) => {
                            this.set_error(format!("Start failed: {err}"));
                        }
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
    }

    pub(crate) fn start_import_from_path(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 从给定路径导入配置，读取与解析都在后台执行。
        // 后台读取文件，避免阻塞 UI 线程。
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

    /// 更新状态栏文案。
    pub(crate) fn set_status(&mut self, message: impl Into<SharedString>) {
        self.status = message.into();
    }

    /// 写入错误并同步到状态栏，便于统一展示。
    pub(crate) fn set_error(&mut self, message: impl Into<SharedString>) {
        let message = message.into();
        self.status = message.clone();
        self.last_error = Some(message);
    }

    /// 获取当前选中的配置。
    pub(crate) fn selected_config(&self) -> Option<&TunnelConfig> {
        self.selected.and_then(|idx| self.configs.get(idx))
    }

    /// 清空输入框内容（用于删除最后一个配置等场景）。
    pub(crate) fn clear_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(name_input) = self.name_input.as_ref() {
            name_input.update(cx, |input, cx| input.set_value("", window, cx));
        }
        if let Some(config_input) = self.config_input.as_ref() {
            config_input.update(cx, |input, cx| input.set_value("", window, cx));
        }
    }

    pub(crate) fn next_config_name(&self, base: &str) -> String {
        // 生成不冲突的配置名（pasted-2 / pasted-3 ...）。
        if !self.configs.iter().any(|cfg| cfg.name == base) {
            return base.to_string();
        }
        for idx in 2..1000 {
            let candidate = format!("{base}-{idx}");
            if !self.configs.iter().any(|cfg| cfg.name == candidate) {
                return candidate;
            }
        }
        format!("{base}-{}", self.configs.len() + 1)
    }
}
