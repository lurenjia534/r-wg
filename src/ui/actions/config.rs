use gpui::{AppContext, ClipboardItem, Context, Window};
use gpui_component::input::InputState;
use r_wg::backend::wg::config;

use super::super::state::{ConfigSource, TunnelConfig, WgApp};

impl WgApp {
    /// 确保输入控件已创建。
    ///
    /// 说明：InputState 需要 Window 上下文才能初始化，因此这里采用懒创建，
    /// 避免在 WgApp::new 阶段就触发窗口依赖。
    pub(crate) fn ensure_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.name_input.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Tunnel name"));
            self.name_input = Some(input);
        }

        if self.config_input.is_none() {
            let placeholder = "[Interface]\nPrivateKey = ...\nAddress = 10.0.0.2/32\n\n[Peer]\nPublicKey = ...\nAllowedIPs = 0.0.0.0/0\nEndpoint = example.com:51820";
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .code_editor("toml")
                    .rows(16)
                    .placeholder(placeholder)
            });
            self.config_input = Some(input);
        }
    }

    pub(crate) fn ensure_proxy_search_input(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Proxies 页搜索框输入状态：用于在大列表中快速过滤。
        // 这里同样采用懒创建，避免在应用启动时就绑定窗口上下文。
        if self.proxy_search_input.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search nodes"));
            self.proxy_search_input = Some(input);
        }
    }

    /// 插入或覆盖配置，并自动选中。
    ///
    /// 说明：以名称为主键，若同名则覆盖，保证列表里不会出现重复名称。
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

    /// 将选中的配置写入输入框。
    ///
    /// 说明：这是 UI 和模型之间的同步点，避免直接从输入框去驱动数据模型。
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

    /// 选中指定隧道并刷新输入框。
    ///
    /// 说明：选中行为既更新模型状态，也触发输入框内容同步。
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

    /// 从剪贴板粘贴配置，并进行基础校验。
    ///
    /// 说明：粘贴路径不依赖文件系统，仍需要 parse 校验以避免写入无效配置。
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

    /// 保存当前输入框内容到配置列表。
    ///
    /// 说明：包含必填校验、格式校验与重名校验，避免异常状态进入模型。
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

    /// 仅修改配置名称，不改内容。
    ///
    /// 说明：重命名时同步更新运行中的隧道名称，避免 UI 状态与引擎名称不一致。
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

    /// 删除当前选中的配置。
    ///
    /// 说明：运行中的配置禁止删除，避免状态错乱和用户误操作。
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

    /// 将当前配置复制到剪贴板。
    ///
    /// 说明：该操作不会改变模型，仅提供快速复制能力。
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

    /// 获取当前选中的配置引用。
    ///
    /// 说明：统一入口避免到处直接访问 self.configs。
    pub(crate) fn selected_config(&self) -> Option<&TunnelConfig> {
        self.selected.and_then(|idx| self.configs.get(idx))
    }

    /// 清空输入框内容。
    ///
    /// 说明：用于删除最后一个配置等场景，防止残留旧值。
    pub(crate) fn clear_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(name_input) = self.name_input.as_ref() {
            name_input.update(cx, |input, cx| input.set_value("", window, cx));
        }
        if let Some(config_input) = self.config_input.as_ref() {
            config_input.update(cx, |input, cx| input.set_value("", window, cx));
        }
    }

    /// 生成一个不会与现有配置重名的名称。
    ///
    /// 说明：用于粘贴或导入场景的自动命名。
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
