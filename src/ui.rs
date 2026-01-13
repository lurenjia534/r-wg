use std::path::{Path, PathBuf};

use editor::{Editor, EditorElement, EditorStyle};
use gpui::*;
use r_wg::backend::wg::{config, Engine, StartRequest};
use settings as app_settings;
use theme as app_theme;

#[derive(Clone)]
enum ConfigSource {
    File(PathBuf),
    Paste,
}

#[derive(Clone)]
struct TunnelConfig {
    name: String,
    text: String,
    source: ConfigSource,
}

impl TunnelConfig {
    fn label(&self) -> String {
        match &self.source {
            ConfigSource::File(path) => {
                let file = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("file");
                format!("{} ({})", self.name, file)
            }
            ConfigSource::Paste => format!("{} (pasted)", self.name),
        }
    }
}

pub fn run() {
    let engine = Engine::new();

    Application::new().run(move |cx: &mut App| {
        app_settings::init(cx);
        app_theme::init(app_theme::LoadThemes::JustBase, cx);
        bind_editor_keys(cx);

        let engine = engine.clone();
        cx.open_window(WindowOptions::default(), move |_, cx| {
            cx.new(|_cx| WgApp::new(engine))
        })
        .unwrap();
    });
}

fn bind_editor_keys(cx: &mut App) {
    use editor::actions as editor_actions;

    cx.bind_keys([
        KeyBinding::new("backspace", editor_actions::Backspace, Some("Editor")),
        KeyBinding::new("delete", editor_actions::Delete, Some("Editor")),
        KeyBinding::new("left", editor_actions::MoveLeft, Some("Editor")),
        KeyBinding::new("right", editor_actions::MoveRight, Some("Editor")),
        KeyBinding::new("up", editor_actions::MoveUp, Some("Editor")),
        KeyBinding::new("down", editor_actions::MoveDown, Some("Editor")),
        KeyBinding::new("shift-left", editor_actions::SelectLeft, Some("Editor")),
        KeyBinding::new("shift-right", editor_actions::SelectRight, Some("Editor")),
        KeyBinding::new("shift-up", editor_actions::SelectUp, Some("Editor")),
        KeyBinding::new("shift-down", editor_actions::SelectDown, Some("Editor")),
        KeyBinding::new("enter", editor_actions::Newline, Some("Editor")),
        KeyBinding::new("tab", editor_actions::Tab, Some("Editor")),
        KeyBinding::new("shift-tab", editor_actions::Backtab, Some("Editor")),
        KeyBinding::new("home", editor_actions::MoveToBeginning, Some("Editor")),
        KeyBinding::new("end", editor_actions::MoveToEnd, Some("Editor")),
        KeyBinding::new("pageup", editor_actions::PageUp, Some("Editor")),
        KeyBinding::new("pagedown", editor_actions::PageDown, Some("Editor")),
        KeyBinding::new("cmd-a", editor_actions::SelectAll, Some("Editor")),
        KeyBinding::new("ctrl-a", editor_actions::SelectAll, Some("Editor")),
        KeyBinding::new("cmd-c", editor_actions::Copy, Some("Editor")),
        KeyBinding::new("ctrl-c", editor_actions::Copy, Some("Editor")),
        KeyBinding::new("cmd-v", editor_actions::Paste, Some("Editor")),
        KeyBinding::new("ctrl-v", editor_actions::Paste, Some("Editor")),
        KeyBinding::new("cmd-x", editor_actions::Cut, Some("Editor")),
        KeyBinding::new("ctrl-x", editor_actions::Cut, Some("Editor")),
        KeyBinding::new("cmd-z", editor_actions::Undo, Some("Editor")),
        KeyBinding::new("ctrl-z", editor_actions::Undo, Some("Editor")),
        KeyBinding::new("cmd-shift-z", editor_actions::Redo, Some("Editor")),
        KeyBinding::new("ctrl-shift-z", editor_actions::Redo, Some("Editor")),
    ]);
}

#[cfg(target_os = "linux")]
fn start_permission_message() -> Option<SharedString> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let euid = parse_status_uid(&status)?;
    if euid == 0 {
        return None;
    }

    let cap_eff = parse_status_cap_eff(&status)?;
    let cap_net_admin = 1u64 << 12;
    if cap_eff & cap_net_admin != 0 {
        return None;
    }

    let exe = std::env::current_exe()
        .ok()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "target/debug/r-wg".to_string());
    Some(format!(
        "需要 cap_net_admin 才能配置网络。请运行：sudo setcap cap_net_admin+ep {exe}"
    )
    .into())
}

#[cfg(not(target_os = "linux"))]
fn start_permission_message() -> Option<SharedString> {
    None
}

#[cfg(target_os = "linux")]
fn parse_status_uid(status: &str) -> Option<u32> {
    status
        .lines()
        .find(|line| line.starts_with("Uid:"))
        .and_then(|line| line.split_whitespace().nth(2))
        .and_then(|value| value.parse().ok())
}

#[cfg(target_os = "linux")]
fn parse_status_cap_eff(status: &str) -> Option<u64> {
    status
        .lines()
        .find(|line| line.starts_with("CapEff:"))
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| u64::from_str_radix(value, 16).ok())
}

struct WgApp {
    engine: Engine,
    configs: Vec<TunnelConfig>,
    selected: Option<usize>,
    name_editor: Option<Entity<Editor>>,
    config_editor: Option<Entity<Editor>>,
    status: SharedString,
    running: bool,
    busy: bool,
    running_name: Option<String>,
}

impl WgApp {
    fn new(engine: Engine) -> Self {
        Self {
            engine,
            configs: Vec::new(),
            selected: None,
            name_editor: None,
            config_editor: None,
            status: "Ready".into(),
            running: false,
            busy: false,
            running_name: None,
        }
    }

    fn ensure_editors(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.name_editor.is_none() {
            let editor = cx.new(|cx| {
                let mut editor = Editor::single_line(window, cx);
                editor.set_placeholder_text("Tunnel name", window, cx);
                editor
            });
            self.name_editor = Some(editor);
        }

        if self.config_editor.is_none() {
            let editor = cx.new(|cx| {
                let mut editor = Editor::multi_line(window, cx);
                editor.set_placeholder_text(
                    "[Interface]\nPrivateKey = ...\nAddress = 10.0.0.2/32\n\n[Peer]\nPublicKey = ...\nAllowedIPs = 0.0.0.0/0\nEndpoint = example.com:51820",
                    window,
                    cx,
                );
                editor
            });
            self.config_editor = Some(editor);
        }
    }

    fn editor_style(window: &Window) -> EditorStyle {
        let mut style = EditorStyle::default();
        style.text = window.text_style();
        style
    }

    fn upsert_config(
        &mut self,
        config: TunnelConfig,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
        self.load_config_into_editors(idx, window, cx);
    }

    fn load_config_into_editors(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.ensure_editors(window, cx);

        let Some(name_editor) = self.name_editor.as_ref() else {
            return;
        };
        let Some(config_editor) = self.config_editor.as_ref() else {
            return;
        };

        let config = &self.configs[idx];
        name_editor.update(cx, |editor, cx| {
            editor.set_text(config.name.clone(), window, cx);
        });
        config_editor.update(cx, |editor, cx| {
            editor.set_text(config.text.clone(), window, cx);
        });
    }

    fn start_import_from_path(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
                        if let Err(err) = config::parse_config(&text) {
                            view.update(cx, |this, cx| {
                                this.busy = false;
                                this.set_status(format!("Invalid config: {err}"));
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
                            this.set_status(format!("Read failed: {err}"));
                            cx.notify();
                        })
                        .ok();
                    }
                }
            })
            .detach();
    }

    fn set_status(&mut self, message: impl Into<SharedString>) {
        self.status = message.into();
    }

    fn selected_config(&self) -> Option<&TunnelConfig> {
        self.selected.and_then(|idx| self.configs.get(idx))
    }
}

impl Render for WgApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_editors(window, cx);

        let name_editor = self
            .name_editor
            .as_ref()
            .expect("name editor should be initialized");
        let config_editor = self
            .config_editor
            .as_ref()
            .expect("config editor should be initialized");

        let input_style = Self::editor_style(window);
        let config_style = Self::editor_style(window);

        let list_items = self
            .configs
            .iter()
            .enumerate()
            .map(|(idx, config)| {
                let selected = self.selected == Some(idx);
                let mut row = div()
                    .w_full()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .text_sm()
                    .cursor_pointer()
                    .child(config.label())
                    .bg(if selected { rgb(0x2d3640) } else { rgb(0x22272e) })
                    .id(idx);

                row = row.on_click(cx.listener(move |this, _event, window, cx| {
                    this.selected = Some(idx);
                    this.load_config_into_editors(idx, window, cx);
                    this.set_status("Loaded tunnel");
                    cx.notify();
                }));

                row
            })
            .collect::<Vec<_>>();

        let list_block = if list_items.is_empty() {
            div()
                .w_full()
                .text_sm()
                .text_color(rgb(0x8a939c))
                .child("No tunnels yet")
                .into_any_element()
        } else {
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap_1()
                .children(list_items)
                .into_any_element()
        };

        let can_start = !self.busy && self.selected.is_some() && !self.running;
        let can_stop = !self.busy && self.running;
        let can_import = !self.busy;
        let can_save = !self.busy;

        let mut import_button =
            action_button("import-button", "Import File", can_import, ButtonTone::Neutral);
        if can_import {
            import_button = import_button.on_click(cx.listener(|this, _event, window, cx| {
                this.set_status("Opening file dialog...");
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
                                    this.set_status(format!("File dialog failed: {err}"));
                                    cx.notify();
                                })
                                .ok();
                                return;
                            }
                            Err(err) => {
                                view.update(cx, |this, cx| {
                                    this.set_status(format!("File dialog closed: {err}"));
                                    cx.notify();
                                })
                                .ok();
                                return;
                            }
                        };

                        let Some(path) = paths.into_iter().next() else {
                            view.update(cx, |this, cx| {
                                this.set_status("No file selected");
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
            }));
        }

        let mut save_button =
            action_button("save-button", "Save From Paste", can_save, ButtonTone::Neutral);
        if can_save {
            save_button = save_button.on_click(cx.listener(|this, _event, window, cx| {
                this.ensure_editors(window, cx);
                let Some(name_editor) = this.name_editor.as_ref() else {
                    this.set_status("Name input not ready");
                    cx.notify();
                    return;
                };
                let Some(config_editor) = this.config_editor.as_ref() else {
                    this.set_status("Config input not ready");
                    cx.notify();
                    return;
                };

                let name = name_editor.read(cx).text(cx);
                let name = name.trim();
                if name.is_empty() {
                    this.set_status("Tunnel name is required");
                    cx.notify();
                    return;
                }

                let text = config_editor.read(cx).text(cx);
                if text.trim().is_empty() {
                    this.set_status("Config is empty");
                    cx.notify();
                    return;
                }

                if let Err(err) = config::parse_config(&text) {
                    this.set_status(format!("Invalid config: {err}"));
                    cx.notify();
                    return;
                }

                this.upsert_config(
                    TunnelConfig {
                        name: name.to_string(),
                        text,
                        source: ConfigSource::Paste,
                    },
                    window,
                    cx,
                );
                this.set_status("Saved tunnel");
                cx.notify();
            }));
        }

        let start_label = if self.running { "Stop" } else { "Start" };
        let start_tone = if self.running {
            ButtonTone::Danger
        } else {
            ButtonTone::Accent
        };
        let start_enabled = if self.running { can_stop } else { can_start };

        let mut start_button =
            action_button("start-button", start_label, start_enabled, start_tone);
        if start_enabled {
            start_button = start_button.on_click(cx.listener(|this, _event, window, cx| {
                let Some(selected) = this.selected_config().cloned() else {
                    this.set_status("Select a tunnel first");
                    cx.notify();
                    return;
                };

                if !this.running {
                    if let Some(message) = start_permission_message() {
                        this.set_status(message);
                        cx.notify();
                        return;
                    }
                }

                this.busy = true;
                if this.running {
                    this.set_status("Stopping...");
                } else {
                    this.set_status(format!("Starting {}...", selected.name));
                }
                cx.notify();

                let engine = this.engine.clone();
                let view = cx.weak_entity();

                if this.running {
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
                                    }
                                    Err(err) => {
                                        this.set_status(format!("Stop failed: {err}"));
                                    }
                                }
                                cx.notify();
                            })
                            .ok();
                        })
                        .detach();
                    return;
                }

                window
                    .spawn(cx, async move |cx| {
                        let request =
                            StartRequest::new(selected.name.clone(), selected.text.clone());
                        let start_task = cx.background_spawn(async move { engine.start(request) });
                        let result = start_task.await;
                        view.update(cx, |this, cx| {
                            this.busy = false;
                            match result {
                                Ok(()) => {
                                    this.running = true;
                                    this.running_name = Some(selected.name.clone());
                                    this.set_status(format!("Running {}", selected.name));
                                }
                                Err(err) => {
                                    this.set_status(format!("Start failed: {err}"));
                                }
                            }
                            cx.notify();
                        })
                        .ok();
                    })
                    .detach();
            }));
        }

        let running_label = match &self.running_name {
            Some(name) => format!("Running: {name}"),
            None => "Idle".to_string(),
        };

        div()
            .size_full()
            .flex()
            .flex_row()
            .bg(rgb(0x101418))
            .text_color(rgb(0xe6e6e6))
            .child(
                div()
                    .w(px(280.0))
                    .h_full()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .bg(rgb(0x151a1f))
                    .child(div().text_lg().child("Tunnels"))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .flex_grow()
                            .id("tunnel-list-scroll")
                            .overflow_y_scroll()
                            .child(list_block),
                    )
                    .child(div().text_sm().text_color(rgb(0x8a939c)).child(running_label))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(import_button)
                            .child(save_button)
                            .child(start_button),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .flex_grow()
                    .p_4()
                    .child(div().text_xl().child("Configuration"))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().text_sm().text_color(rgb(0x8a939c)).child("Tunnel Name"))
                            .child(
                                div()
                                    .w_full()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .bg(rgb(0x1a2026))
                                    .child(EditorElement::new(name_editor, input_style)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .flex_grow()
                            .child(div().text_sm().text_color(rgb(0x8a939c)).child("Config"))
                            .child(
                                div()
                                    .w_full()
                                    .flex_grow()
                                    .min_h(px(220.0))
                                    .p_2()
                                    .rounded_md()
                                    .bg(rgb(0x1a2026))
                                    .child(EditorElement::new(config_editor, config_style)),
                            ),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0x8a939c))
                            .child(self.status.clone()),
                    ),
            )
    }
}

enum ButtonTone {
    Neutral,
    Accent,
    Danger,
}

fn action_button(id: &'static str, label: &str, enabled: bool, tone: ButtonTone) -> Stateful<Div> {
    let base = match tone {
        ButtonTone::Neutral => rgb(0x2a3138),
        ButtonTone::Accent => rgb(0x2b6f55),
        ButtonTone::Danger => rgb(0x7a2f2f),
    };

    let mut button = div()
        .w_full()
        .px_3()
        .py_2()
        .rounded_md()
        .text_sm()
        .bg(if enabled { base } else { rgb(0x1d2228) })
        .text_color(if enabled { rgb(0xe6e6e6) } else { rgb(0x6f7882) })
        .child(label.to_string())
        .id(id);

    if enabled {
        button = button.cursor_pointer();
    }

    button
}

fn name_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|name| name.to_str())
        .map(|name| name.trim())
        .filter(|name| !name.is_empty())
        .unwrap_or("tunnel")
        .to_string()
}
