use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use gpui::*;
use gpui_component::input::{Input, InputState};
use gpui_component::Root;
use r_wg::backend::wg::{config, Engine, EngineStats, PeerStats, StartRequest};

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
        gpui_component::init(cx);

        let engine = engine.clone();
        cx.open_window(WindowOptions::default(), move |window, cx| {
            let view = cx.new(|_cx| WgApp::new(engine));
            cx.new(|cx| Root::new(view, window, cx))
        })
        .unwrap();
    });
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
    name_input: Option<Entity<InputState>>,
    config_input: Option<Entity<InputState>>,
    status: SharedString,
    running: bool,
    busy: bool,
    running_name: Option<String>,
    peer_stats: Vec<PeerStats>,
    stats_note: SharedString,
    stats_generation: u64,
}

impl WgApp {
    fn new(engine: Engine) -> Self {
        Self {
            engine,
            configs: Vec::new(),
            selected: None,
            name_input: None,
            config_input: None,
            status: "Ready".into(),
            running: false,
            busy: false,
            running_name: None,
            peer_stats: Vec::new(),
            stats_note: "Peer stats unavailable".into(),
            stats_generation: 0,
        }
    }

    fn ensure_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
        self.load_config_into_inputs(idx, window, cx);
    }

    fn load_config_into_inputs(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
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

    fn start_stats_polling(&mut self, cx: &mut Context<Self>) {
        self.stats_generation = self.stats_generation.wrapping_add(1);
        let generation = self.stats_generation;
        let engine = self.engine.clone();
        let poll_interval = Duration::from_secs(2);

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

    fn apply_stats(&mut self, stats: EngineStats) {
        self.peer_stats = stats.peers;
        if self.peer_stats.is_empty() {
            self.stats_note = "No peers reported".into();
        } else {
            self.stats_note = format!("Peers: {}", self.peer_stats.len()).into();
        }
    }

    fn clear_stats(&mut self) {
        self.peer_stats.clear();
        self.stats_note = "Peer stats unavailable".into();
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
        self.ensure_inputs(window, cx);

        let name_input = self
            .name_input
            .as_ref()
            .expect("name input should be initialized");
        let config_input = self
            .config_input
            .as_ref()
            .expect("config input should be initialized");

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
                    this.load_config_into_inputs(idx, window, cx);
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

        let mut stats_items = Vec::new();
        stats_items.push(
            div()
                .text_sm()
                .text_color(rgb(0x8a939c))
                .child(self.stats_note.clone())
                .into_any_element(),
        );
        if self.peer_stats.is_empty() {
            stats_items.push(
                div()
                    .text_sm()
                    .text_color(rgb(0x8a939c))
                    .child("No peer stats yet")
                    .into_any_element(),
            );
        } else {
            stats_items.extend(self.peer_stats.iter().map(|peer| {
                div()
                    .text_sm()
                    .child(format_peer_line(peer))
                    .into_any_element()
            }));
        }
        let stats_block = div().flex().flex_col().gap_1().children(stats_items);

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
                this.ensure_inputs(window, cx);
                let Some(name_input) = this.name_input.as_ref() else {
                    this.set_status("Name input not ready");
                    cx.notify();
                    return;
                };
                let Some(config_input) = this.config_input.as_ref() else {
                    this.set_status("Config input not ready");
                    cx.notify();
                    return;
                };

                let name = name_input.read(cx).value().to_string();
                let name = name.trim();
                if name.is_empty() {
                    this.set_status("Tunnel name is required");
                    cx.notify();
                    return;
                }

                let text = config_input.read(cx).value().to_string();
                if text.trim().is_empty() {
                    this.set_status("Config text is required");
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
                                        this.clear_stats();
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
                                        this.stats_note = "Fetching peer stats...".into();
                                        this.start_stats_polling(cx);
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
                                    .child(Input::new(name_input)),
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
                                    .child(Input::new(config_input)),
                            ),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0x8a939c))
                            .child(self.status.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().text_sm().text_color(rgb(0x8a939c)).child("Peer Stats"))
                            .child(stats_block),
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

fn format_peer_line(peer: &PeerStats) -> String {
    let key = format_public_key(&peer.public_key);
    let (handshake, handshake_suffix) = match peer.last_handshake {
        Some(duration) => (format_duration(duration), " ago"),
        None => ("never".to_string(), ""),
    };
    let rx = format_bytes(peer.rx_bytes);
    let tx = format_bytes(peer.tx_bytes);
    let endpoint = peer
        .endpoint
        .map(|addr| addr.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    format!(
        "{key}  handshake: {handshake}{handshake_suffix}  rx: {rx}  tx: {tx}  endpoint: {endpoint}"
    )
}

fn format_public_key(key: &[u8; 32]) -> String {
    let encoded = STANDARD.encode(key);
    let short = encoded.get(0..8).unwrap_or(&encoded);
    format!("{short}...")
}

fn format_duration(duration: Duration) -> String {
    let secs = duration.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        let minutes = secs / 60;
        let seconds = secs % 60;
        format!("{minutes}m{seconds}s")
    } else {
        let hours = secs / 3600;
        let minutes = (secs % 3600) / 60;
        format!("{hours}h{minutes}m")
    }
}

fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * KB;
    const GB: f64 = 1024.0 * MB;

    let value = bytes as f64;
    if value >= GB {
        format!("{:.1}GiB", value / GB)
    } else if value >= MB {
        format!("{:.1}MiB", value / MB)
    } else if value >= KB {
        format!("{:.1}KiB", value / KB)
    } else {
        format!("{bytes}B")
    }
}

fn name_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|name| name.to_str())
        .map(|name| name.trim())
        .filter(|name| !name.is_empty())
        .unwrap_or("tunnel")
        .to_string()
}
