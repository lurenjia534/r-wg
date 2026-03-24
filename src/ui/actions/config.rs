use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;

mod endpoint_family;
mod naming;

use gpui::{
    div, AppContext, ClipboardItem, Context, Entity, IntoElement, ParentElement, SharedString,
    Styled, Window,
};
use gpui_component::{
    button::{Button, ButtonVariant, ButtonVariants as _},
    dialog::DialogButtonProps,
    input::{InputEvent, InputState, TabSize},
    ActiveTheme as _, WindowExt,
};
use r_wg::backend::wg::config;

use super::super::persistence;
use super::super::state::{
    ConfigDraftState, ConfigSource, ConfigsPrimaryPane, ConfigsWorkspace, DraftValidationState,
    EditorOperation, EndpointFamily, LoadedConfigState, PendingDraftAction, SidebarItem,
    TunnelConfig, WgApp,
};
pub(crate) use endpoint_family::{
    endpoint_family_hint_from_config, resolve_endpoint_family_from_text,
};
pub(crate) use naming::reserve_unique_name;
use naming::next_available_name;

const CONFIG_TEXT_CACHE_LIMIT: usize = 32;
const DRAFT_VALIDATION_DEBOUNCE: Duration = Duration::from_millis(180);

/// 删除策略：遇到运行中配置时的处理方式。
///
/// 说明：
/// - BlockRunning：遇到运行中配置直接阻止删除；
/// - SkipRunning：跳过运行中配置，继续删除其余项。
#[derive(Clone, Copy)]
enum DeletePolicy {
    BlockRunning,
    SkipRunning,
}

impl WgApp {
    pub(crate) fn upsert_configs_workspace_library_row(
        &mut self,
        config: &TunnelConfig,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.ui.configs_workspace.clone() else {
            return;
        };
        let config = config.clone();
        let running_id = self.runtime.running_id;
        let running_name = self.runtime.running_name.clone();
        workspace.update(cx, |workspace, cx| {
            if workspace.upsert_library_row(&config, running_id, running_name.as_deref()) {
                cx.notify();
            }
        });
    }

    pub(crate) fn remove_configs_workspace_library_rows(
        &mut self,
        ids: &HashSet<u64>,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.ui.configs_workspace.clone() else {
            return;
        };
        let ids = ids.clone();
        workspace.update(cx, |workspace, cx| {
            if workspace.remove_library_rows(&ids) {
                cx.notify();
            }
        });
    }

    pub(crate) fn append_configs_workspace_library_rows(
        &mut self,
        configs: &[TunnelConfig],
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.ui.configs_workspace.clone() else {
            return;
        };
        let configs = configs.to_vec();
        let running_id = self.runtime.running_id;
        let running_name = self.runtime.running_name.clone();
        workspace.update(cx, |workspace, cx| {
            if workspace.append_library_rows(&configs, running_id, running_name.as_deref()) {
                cx.notify();
            }
        });
    }

    pub(crate) fn refresh_configs_workspace_row_flags(&mut self, cx: &mut Context<Self>) {
        let Some(workspace) = self.ui.configs_workspace.clone() else {
            return;
        };
        let running_id = self.runtime.running_id;
        let running_name = self.runtime.running_name.clone();
        workspace.update(cx, |workspace, cx| {
            if workspace.refresh_library_row_flags(running_id, running_name.as_deref()) {
                cx.notify();
            }
        });
    }

    pub(crate) fn set_selected_config_id(
        &mut self,
        selected_id: Option<u64>,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.selection.selected_id == selected_id {
            return false;
        }
        self.selection.selected_id = selected_id;
        self.selection.selection_revision = self.selection.selection_revision.wrapping_add(1);
        self.sync_configs_selection_snapshot(cx);
        true
    }

    pub(crate) fn sync_configs_selection_snapshot(&mut self, cx: &mut Context<Self>) {
        let Some(workspace) = self.ui.configs_workspace.clone() else {
            return;
        };
        let has_selection = self.selection.selected_id.is_some();
        workspace.update(cx, |workspace, cx| {
            if workspace.has_selection != has_selection {
                workspace.has_selection = has_selection;
                cx.notify();
            }
        });
    }

    pub(crate) fn set_editor_operation(
        &mut self,
        operation: Option<EditorOperation>,
        cx: &mut Context<Self>,
    ) {
        let workspace = self.ensure_configs_workspace(cx);
        workspace.update(cx, |workspace, cx| {
            workspace.operation = operation;
            cx.notify();
        });
    }

    pub(crate) fn configs_draft_snapshot(&self, cx: &mut Context<Self>) -> ConfigDraftState {
        self.ui
            .configs_workspace
            .as_ref()
            .map(|workspace| workspace.read(cx).draft.clone())
            .unwrap_or_else(ConfigDraftState::new)
    }

    pub(crate) fn configs_operation_snapshot(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<EditorOperation> {
        self.ui
            .configs_workspace
            .as_ref()
            .map(|workspace| workspace.read(cx).operation.clone())
            .unwrap_or(None)
    }

    pub(crate) fn configs_is_busy(&self, cx: &mut Context<Self>) -> bool {
        self.configs_operation_snapshot(cx).is_some()
    }

    pub(crate) fn set_configs_pending_action(
        &mut self,
        action: Option<PendingDraftAction>,
        cx: &mut Context<Self>,
    ) {
        if self.ui.configs_workspace.is_none() {
            let _ = self.ensure_configs_workspace(cx);
        }
        if let Some(workspace) = self.ui.configs_workspace.clone() {
            workspace.update(cx, |workspace, cx| {
                workspace.pending_action = action;
                cx.notify();
            });
        }
    }

    pub(crate) fn take_configs_pending_action(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<PendingDraftAction> {
        if let Some(workspace) = self.ui.configs_workspace.clone() {
            let mut action = None;
            workspace.update(cx, |workspace, cx| {
                action = workspace.pending_action.take();
                cx.notify();
            });
            return action;
        }
        None
    }

    fn configs_name_input(&self, cx: &mut Context<Self>) -> Option<Entity<InputState>> {
        self.ui
            .configs_workspace
            .as_ref()
            .and_then(|workspace| workspace.read(cx).name_input.clone())
    }

    fn configs_config_input(&self, cx: &mut Context<Self>) -> Option<Entity<InputState>> {
        self.ui
            .configs_workspace
            .as_ref()
            .and_then(|workspace| workspace.read(cx).config_input.clone())
    }

    fn configs_inputs(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<(Entity<InputState>, Entity<InputState>)> {
        Some((self.configs_name_input(cx)?, self.configs_config_input(cx)?))
    }

    /// 确保输入控件已创建。
    ///
    /// 说明：InputState 需要 Window 上下文才能初始化，因此这里采用懒创建，
    /// 避免在 WgApp::new 阶段就触发窗口依赖。
    pub(crate) fn ensure_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let workspace = self.ensure_configs_workspace(cx);
        let has_inputs = workspace.read(cx).has_inputs();
        if has_inputs {
            return;
        }
        workspace.update(cx, |workspace, cx| {
            workspace.ensure_inputs(window, cx);
        });
    }

    fn sync_draft_from_values(
        &mut self,
        name: SharedString,
        text: SharedString,
        cx: &mut Context<Self>,
    ) {
        let workspace = self.ensure_configs_workspace(cx);
        workspace.update(cx, |workspace, cx| {
            if workspace.sync_draft_from_values(name, text, self.runtime.running_id) {
                cx.notify();
            }
        });
        self.refresh_configs_workspace_row_flags(cx);
    }
}

impl ConfigsWorkspace {
    fn schedule_draft_validation(&mut self, cx: &mut Context<Self>) {
        if self.draft.text.as_ref().trim().is_empty() || self.operation.is_some() {
            return;
        }

        self.validation_generation = self.validation_generation.wrapping_add(1);
        let generation = self.validation_generation;
        cx.spawn(async move |view, cx| {
            cx.background_executor()
                .timer(DRAFT_VALIDATION_DEBOUNCE)
                .await;
            let _ = view.update(cx, |this, cx| {
                if this.validation_generation != generation || this.operation.is_some() {
                    return;
                }
                let running_id = this.app.read(cx).runtime.running_id;
                let running_name = this.app.read(cx).runtime.running_name.clone();
                this.apply_draft_validation(running_id);
                this.refresh_library_row_flags(running_id, running_name.as_deref());
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn ensure_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.library_search_input.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search configs"));
            let subscription = cx.subscribe(
                &input,
                |_, _, event: &InputEvent, cx: &mut Context<Self>| {
                    if matches!(event, InputEvent::Change) {
                        cx.notify();
                    }
                },
            );
            self.library_search_input = Some(input);
            self.library_search_subscription = Some(subscription);
        }

        if self.name_input.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Config title"));
            let subscription = cx.subscribe(
                &input,
                |this, _, event: &InputEvent, cx: &mut Context<Self>| match event {
                    InputEvent::Change => {
                        let Some(name_input) = this.name_input.as_ref() else {
                            return;
                        };
                        let Some(config_input) = this.config_input.as_ref() else {
                            return;
                        };
                        let name = name_input.read(cx).value();
                        let text = config_input.read(cx).value();
                        let running_id = this.app.read(cx).runtime.running_id;
                        let has_selection = this.app.read(cx).selection.selected_id.is_some();
                        this.sync_draft_from_values(name, text, running_id);
                        this.has_selection = has_selection;
                        this.schedule_draft_validation(cx);
                        cx.notify();
                    }
                    InputEvent::Focus => {
                        if this.set_title_editing(true) {
                            cx.notify();
                        }
                    }
                    InputEvent::Blur | InputEvent::PressEnter { .. } => {
                        if this.set_title_editing(false) {
                            cx.notify();
                        }
                    }
                },
            );
            self.name_input = Some(input);
            self.name_input_subscription = Some(subscription);
        }

        if self.config_input.is_none() {
            let placeholder = "[Interface]\nPrivateKey = ...\nAddress = 10.0.0.2/32\n\n[Peer]\nPublicKey = ...\nAllowedIPs = 0.0.0.0/0\nEndpoint = example.com:51820";
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .code_editor("toml")
                    .line_number(true)
                    .searchable(true)
                    .soft_wrap(false)
                    .tab_size(TabSize {
                        hard_tabs: false,
                        tab_size: 4,
                    })
                    .rows(16)
                    .placeholder(placeholder)
            });
            let subscription = cx.subscribe(
                &input,
                |this, _, event: &InputEvent, cx: &mut Context<Self>| {
                    if matches!(event, InputEvent::Change) {
                        let Some(name_input) = this.name_input.as_ref() else {
                            return;
                        };
                        let Some(config_input) = this.config_input.as_ref() else {
                            return;
                        };
                        let name = name_input.read(cx).value();
                        let text = config_input.read(cx).value();
                        let running_id = this.app.read(cx).runtime.running_id;
                        let has_selection = this.app.read(cx).selection.selected_id.is_some();
                        this.sync_draft_from_values(name, text, running_id);
                        this.has_selection = has_selection;
                        this.schedule_draft_validation(cx);
                        cx.notify();
                    }
                },
            );
            self.config_input = Some(input);
            self.config_input_subscription = Some(subscription);
        }
    }
}

impl WgApp {
    pub(crate) fn ensure_proxy_search_input(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Proxies 页搜索框输入状态：用于在大列表中快速过滤。
        // 这里同样采用懒创建，避免在应用启动时就绑定窗口上下文。
        if self.ui.proxy_search_input.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search nodes"));
            self.ui.proxy_search_input = Some(input);
        }
    }

    /// 将配置文本写入缓存，并维护简单的 LRU 顺序。
    ///
    /// 说明：
    /// - 只缓存最近使用的配置文本，避免导入上千条时占用过多内存；
    /// - 同一路径重复写入会刷新位置，保证“最近用过”的优先保留。
    pub(crate) fn cache_config_text(&mut self, path: PathBuf, text: SharedString) {
        self.selection.config_text_cache.insert(path.clone(), text);
        self.selection
            .config_text_cache_order
            .retain(|entry| entry != &path);
        self.selection.config_text_cache_order.push_back(path);
        while self.selection.config_text_cache_order.len() > CONFIG_TEXT_CACHE_LIMIT {
            if let Some(evicted) = self.selection.config_text_cache_order.pop_front() {
                self.selection.config_text_cache.remove(&evicted);
            }
        }
        self.selection.selection_revision = self.selection.selection_revision.wrapping_add(1);
    }

    pub(crate) fn cached_config_text(&mut self, path: &Path) -> Option<SharedString> {
        let text = self.selection.config_text_cache.get(path).cloned();
        if text.is_some() {
            self.selection
                .config_text_cache_order
                .retain(|entry| entry != path);
            self.selection
                .config_text_cache_order
                .push_back(path.to_path_buf());
        }
        text
    }

    pub(crate) fn peek_cached_config_text(&self, path: &Path) -> Option<SharedString> {
        self.selection.config_text_cache.get(path).cloned()
    }

    fn apply_draft_validation(&mut self, cx: &mut Context<Self>) {
        let workspace = self.ensure_configs_workspace(cx);
        workspace.update(cx, |workspace, cx| {
            workspace.apply_draft_validation(self.runtime.running_id);
            cx.notify();
        });
        self.refresh_configs_workspace_row_flags(cx);
    }

    fn set_saved_draft(
        &mut self,
        source_id: u64,
        name: SharedString,
        text: SharedString,
        cx: &mut Context<Self>,
    ) {
        let workspace = self.ensure_configs_workspace(cx);
        let workspace_name = name.clone();
        let workspace_text = text.clone();
        workspace.update(cx, |workspace, cx| {
            workspace.set_saved_draft(source_id, workspace_name, workspace_text);
            cx.notify();
        });
        self.refresh_configs_workspace_row_flags(cx);
        self.apply_draft_validation(cx);
    }

    fn set_unsaved_draft(
        &mut self,
        name: SharedString,
        text: SharedString,
        cx: &mut Context<Self>,
    ) {
        let workspace = self.ensure_configs_workspace(cx);
        let workspace_name = name.clone();
        let workspace_text = text.clone();
        workspace.update(cx, |workspace, cx| {
            workspace.set_unsaved_draft(workspace_name, workspace_text);
            cx.notify();
        });
        self.refresh_configs_workspace_row_flags(cx);
        self.apply_draft_validation(cx);
    }

    pub(crate) fn sync_draft_from_inputs(&mut self, cx: &mut Context<Self>) {
        let Some((name_input, config_input)) = self.configs_inputs(cx) else {
            return;
        };
        let name = name_input.read(cx).value();
        let text = config_input.read(cx).value();
        self.sync_draft_from_values(name, text, cx);
    }

    fn discard_current_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let draft = self.configs_draft_snapshot(cx);
        if let Some(source_id) = draft.source_id.or(self.selection.selected_id) {
            self.set_selected_config_id(Some(source_id), cx);
            self.load_config_into_inputs(source_id, window, cx);
        } else {
            self.set_selected_config_id(None, cx);
            self.clear_inputs(window, cx);
        }
    }

    fn run_pending_draft_action(
        &mut self,
        action: PendingDraftAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            PendingDraftAction::SelectConfig(config_id) => {
                if self.selection.selected_id == Some(config_id) {
                    return;
                }
                self.set_selected_config_id(Some(config_id), cx);
                self.load_config_into_inputs(config_id, window, cx);
                self.persist_state_async(cx);
                self.set_status("Loaded tunnel");
                cx.notify();
            }
            PendingDraftAction::ActivateSidebar(item) => {
                self.set_sidebar_active(item, cx);
                self.close_sidebar_overlay(cx);
            }
            PendingDraftAction::NewDraft => {
                self.set_selected_config_id(None, cx);
                self.clear_inputs(window, cx);
                let workspace = self.ensure_configs_workspace(cx);
                workspace.update(cx, |workspace, cx| {
                    if workspace.set_primary_pane(ConfigsPrimaryPane::Editor) {
                        cx.notify();
                    }
                });
                self.set_status("New draft");
                cx.notify();
            }
            PendingDraftAction::Import => {
                self.handle_import_click(window, cx);
            }
            PendingDraftAction::Paste => {
                self.handle_paste_click(window, cx);
            }
            PendingDraftAction::DeleteCurrent => {
                self.open_delete_current_config_dialog(window, cx);
            }
            PendingDraftAction::RestartTunnel => {
                self.runtime.queue_pending_start(
                    self.selection
                        .build_pending_start(&self.configs, &self.runtime),
                );
                self.handle_start_stop_core(cx);
            }
        }
    }

    pub(crate) fn confirm_discard_or_save(
        &mut self,
        action: PendingDraftAction,
        window: &mut Window,
        cx: &mut Context<Self>,
        title: impl Into<SharedString>,
        body: impl Into<SharedString>,
    ) {
        if !self.configs_draft_snapshot(cx).is_dirty() {
            self.run_pending_draft_action(action, window, cx);
            return;
        }

        let app_handle = cx.entity();
        let title = title.into();
        let body = body.into();

        window.open_dialog(cx, move |dialog, _window, _cx| {
            let app_handle_save = app_handle.clone();
            let app_handle_discard = app_handle.clone();
            let app_handle_ok = app_handle.clone();
            dialog
                .title(div().text_lg().child(title.clone()))
                .confirm()
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("Save")
                        .ok_variant(ButtonVariant::Primary)
                        .cancel_text("Cancel"),
                )
                .child(div().text_sm().child(body.clone()))
                .child(
                    div()
                        .text_xs()
                        .text_color(_cx.theme().muted_foreground)
                        .child("Save your edits, discard them, or cancel this action."),
                )
                .footer(move |_ok, _cancel, _window, _cx| {
                    let save_handle = app_handle_save.clone();
                    let discard_handle = app_handle_discard.clone();
                    let save_button = Button::new("draft-dialog-save").label("Save").on_click(
                        move |_, window, cx| {
                            save_handle.update(cx, |app, cx| {
                                app.set_configs_pending_action(Some(action), cx);
                                app.save_draft(false, window, cx);
                            });
                            window.close_dialog(cx);
                        },
                    );
                    let discard_button = Button::new("draft-dialog-discard")
                        .label("Discard")
                        .danger()
                        .on_click(move |_, window, cx| {
                            window.on_next_frame({
                                let discard_handle = discard_handle.clone();
                                move |window, cx| {
                                    discard_handle.update(cx, |app, cx| {
                                        app.set_configs_pending_action(None, cx);
                                        app.discard_current_draft(window, cx);
                                        app.run_pending_draft_action(action, window, cx);
                                    });
                                }
                            });
                            window.close_dialog(cx);
                        });
                    let cancel_button = Button::new("draft-dialog-cancel")
                        .label("Cancel")
                        .outline()
                        .on_click(|_, window, cx| {
                            window.close_dialog(cx);
                        });
                    vec![
                        cancel_button.into_any_element(),
                        discard_button.into_any_element(),
                        save_button.into_any_element(),
                    ]
                })
                .on_ok(move |_, window, cx| {
                    app_handle_ok.update(cx, |app, cx| {
                        app.set_configs_pending_action(Some(action), cx);
                        app.save_draft(false, window, cx);
                    });
                    true
                })
        });
    }

    pub(crate) fn request_sidebar_active(
        &mut self,
        item: SidebarItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.ui_session.sidebar_active == item {
            return;
        }
        self.confirm_discard_or_save(
            PendingDraftAction::ActivateSidebar(item),
            window,
            cx,
            "Leave Configs?",
            "You have unsaved changes in the current config draft.",
        );
    }

    pub(crate) fn handle_new_draft_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_discard_or_save(
            PendingDraftAction::NewDraft,
            window,
            cx,
            "Create new draft?",
            "Creating a new draft will replace the current unsaved draft.",
        );
    }

    fn open_delete_current_config_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(config_id) = self.selection.selected_id else {
            self.set_error("Select a tunnel first");
            cx.notify();
            return;
        };
        let config_name = self
            .configs
            .get_by_id(config_id)
            .map(|cfg| cfg.name.clone())
            .unwrap_or_else(|| "this config".to_string());
        let app_handle = cx.entity();

        window.open_dialog(cx, move |dialog, _window, _cx| {
            let delete_handle = app_handle.clone();
            dialog
                .title(div().text_lg().child("Delete config?"))
                .confirm()
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("Delete")
                        .ok_variant(ButtonVariant::Danger)
                        .cancel_text("Cancel"),
                )
                .child(
                    div()
                        .text_sm()
                        .child(format!("Delete \"{config_name}\"? This cannot be undone.")),
                )
                .on_ok(move |_, window, cx| {
                    delete_handle.update(cx, |app, cx| {
                        app.handle_confirmed_delete_current(window, cx);
                    });
                    true
                })
        });
    }

    fn handle_confirmed_delete_current(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(config_id) = self.selection.selected_id else {
            self.set_error("Select a tunnel first");
            cx.notify();
            return;
        };
        self.delete_configs_blocking_running(&[config_id], window, cx);
    }

    /// 按 ID 插入或更新配置，并保持选中状态与 draft 基线一致。
    pub(crate) fn insert_or_update_config(
        &mut self,
        config: TunnelConfig,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let idx = match self.configs.find_index_by_id(config.id) {
            Some(idx) => {
                self.configs[idx] = config;
                idx
            }
            None => {
                self.configs.push(config);
                self.configs.len() - 1
            }
        };

        let config_id = self.configs[idx].id;
        self.set_selected_config_id(Some(config_id), cx);
        let updated_config = self.configs[idx].clone();
        self.upsert_configs_workspace_library_row(&updated_config, cx);
        if let Some(text) = self.configs[idx].text.clone() {
            self.set_saved_draft(config_id, self.configs[idx].name.clone().into(), text, cx);
        }
        self.load_config_into_inputs(config_id, window, cx);
        if self.configs[idx].endpoint_family == EndpointFamily::Unknown {
            let text = self.configs[idx].text.clone();
            let path = self.configs[idx].storage_path.clone();
            self.schedule_endpoint_family_refresh(config_id, text, path, cx);
        }
    }

    /// 将选中的配置写入输入框。
    ///
    /// 说明：这是 UI 和模型之间的同步点，避免直接从输入框去驱动数据模型。
    pub(crate) fn load_config_into_inputs(
        &mut self,
        config_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 将模型数据灌入输入控件。
        self.ensure_inputs(window, cx);

        let Some((name_input, config_input)) = self.configs_inputs(cx) else {
            return;
        };

        let Some(config) = self.configs.get_by_id(config_id) else {
            return;
        };
        let name = config.name.clone();
        let text = config.text.clone();
        let path = config.storage_path.clone();
        let endpoint_family = config.endpoint_family;

        // 优先走内存：如果 text 已经存在，直接写入输入框。
        if let Some(text) = text {
            let text_hash = text_hash(text.as_ref());
            if let Some(loaded) = &self.selection.loaded_config {
                if loaded.name == name && loaded.text_hash == text_hash {
                    return;
                }
            }

            self.set_editor_operation(Some(EditorOperation::LoadingConfig), cx);
            self.set_saved_draft(config_id, name.clone().into(), text.clone(), cx);

            name_input.update(cx, |input, cx| {
                input.set_value(name.clone(), window, cx);
            });
            config_input.update(cx, |input, cx| {
                input.set_value(text.clone(), window, cx);
            });
            self.set_editor_operation(None, cx);
            self.selection.loading_config_id = None;
            self.selection.loading_config_path = None;
            self.selection.loaded_config = Some(LoadedConfigState { name, text_hash });
            if endpoint_family == EndpointFamily::Unknown {
                self.schedule_endpoint_family_refresh(config_id, Some(text), path, cx);
            }
            return;
        }

        // 如果缓存里有文本，直接复用缓存。
        if let Some(text) = self.cached_config_text(&path) {
            let text_hash = text_hash(text.as_ref());
            if let Some(loaded) = &self.selection.loaded_config {
                if loaded.name == name && loaded.text_hash == text_hash {
                    return;
                }
            }

            self.set_editor_operation(Some(EditorOperation::LoadingConfig), cx);
            self.set_saved_draft(config_id, name.clone().into(), text.clone(), cx);

            name_input.update(cx, |input, cx| {
                input.set_value(name.clone(), window, cx);
            });
            config_input.update(cx, |input, cx| {
                input.set_value(text.clone(), window, cx);
            });
            self.set_editor_operation(None, cx);
            self.selection.loading_config_id = None;
            self.selection.loading_config_path = None;
            self.selection.loaded_config = Some(LoadedConfigState { name, text_hash });
            if endpoint_family == EndpointFamily::Unknown {
                self.schedule_endpoint_family_refresh(config_id, Some(text), path, cx);
            }
            return;
        }

        // 最后才走磁盘 IO：异步读取文件。
        // 注意：这里会把 loading_config_path 记录下来，避免索引复用导致错写。
        self.selection.loading_config_id = Some(config_id);
        self.selection.loading_config_path = Some(path.clone());
        self.set_editor_operation(Some(EditorOperation::LoadingConfig), cx);
        if endpoint_family == EndpointFamily::Unknown {
            self.selection.endpoint_family_loading.insert(config_id);
        }
        self.selection.loaded_config = None;
        name_input.update(cx, |input, cx| {
            input.set_value(name.clone(), window, cx);
        });
        config_input.update(cx, |input, cx| {
            if input.text().len() > 0 {
                input.set_value("", window, cx);
            }
        });
        self.set_status("Loading config...");
        cx.notify();

        let view = cx.weak_entity();
        window
            .spawn(cx, async move |cx| {
                let path_for_match = path.clone();
                let path_for_cache = path.clone();
                let read_task = cx.background_spawn(async move {
                    let text = std::fs::read_to_string(&path)?;
                    let family = resolve_endpoint_family_from_text(text.clone()).await;
                    Ok::<_, std::io::Error>((text, family))
                });
                let result = read_task.await;
                view.update_in(cx, |this, window, cx| {
                    let Some(config) = this.configs.get_by_id(config_id) else {
                        this.selection.endpoint_family_loading.remove(&config_id);
                        if this.selection.loading_config_id == Some(config_id) {
                            this.selection.loading_config_id = None;
                            this.selection.loading_config_path = None;
                        }
                        return;
                    };
                    if config.storage_path != path_for_match {
                        this.selection.endpoint_family_loading.remove(&config_id);
                        if this.selection.loading_config_id == Some(config_id)
                            && this.selection.loading_config_path.as_ref() == Some(&path_for_match)
                        {
                            this.selection.loading_config_id = None;
                            this.selection.loading_config_path = None;
                        }
                        return;
                    }
                    let should_write_ui = this.selection.selected_id == Some(config_id)
                        && this.selection.loading_config_id == Some(config_id)
                        && this.selection.loading_config_path.as_ref() == Some(&path_for_match);

                    match result {
                        Ok((text, family)) => {
                            let text: SharedString = text.into();
                            this.cache_config_text(path_for_cache, text.clone());
                            if let Some(config) = this.configs.get_mut_by_id(config_id) {
                                config.endpoint_family = family;
                            }
                            this.selection.endpoint_family_loading.remove(&config_id);
                            if should_write_ui {
                                this.selection.loading_config_id = None;
                                this.selection.loading_config_path = None;
                                this.set_saved_draft(
                                    config_id,
                                    name.clone().into(),
                                    text.clone(),
                                    cx,
                                );
                                if let Some(config_input) = this.configs_config_input(cx) {
                                    config_input.update(cx, |input, cx| {
                                        input.set_value(text.clone(), window, cx);
                                    });
                                }
                                if let Some(name_input) = this.configs_name_input(cx) {
                                    name_input.update(cx, |input, cx| {
                                        input.set_value(name.clone(), window, cx);
                                    });
                                }
                                let text_hash = text_hash(text.as_ref());
                                this.selection.loaded_config =
                                    Some(LoadedConfigState { name, text_hash });
                                this.set_editor_operation(None, cx);
                                this.set_status("Loaded config");
                            }
                            cx.notify();
                        }
                        Err(err) => {
                            this.selection.endpoint_family_loading.remove(&config_id);
                            if should_write_ui {
                                this.selection.loading_config_id = None;
                                this.selection.loading_config_path = None;
                                this.set_editor_operation(None, cx);
                                this.set_error(format!("Read failed: {err}"));
                                cx.notify();
                            }
                        }
                    }
                })
                .ok();
            })
            .detach();
    }

    /// 选中指定隧道并刷新输入框。
    ///
    /// 说明：选中行为既更新模型状态，也触发输入框内容同步。
    pub(crate) fn select_tunnel(
        &mut self,
        config_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selection.selected_id == Some(config_id) {
            return;
        }
        self.confirm_discard_or_save(
            PendingDraftAction::SelectConfig(config_id),
            window,
            cx,
            "Switch config?",
            "You have unsaved changes in the current config draft.",
        );
    }

    /// 从剪贴板粘贴配置，并进行基础校验。
    ///
    /// 说明：粘贴路径不依赖文件系统，仍需要 parse 校验以避免写入无效配置。
    pub(crate) fn handle_paste_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.configs_draft_snapshot(cx).is_dirty() {
            self.confirm_discard_or_save(
                PendingDraftAction::Paste,
                window,
                cx,
                "Replace draft?",
                "Pasting a config will replace the current unsaved draft.",
            );
            return;
        }
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
        let text: SharedString = text.into();

        let suggested_name = self
            .configs_name_input(cx)
            .map(|input| input.read(cx).value().to_string())
            .unwrap_or_default();
        let suggested_name = suggested_name.trim();
        let name = if suggested_name.is_empty() {
            self.next_config_name("pasted")
        } else if self.configs.iter().any(|cfg| cfg.name == suggested_name) {
            self.next_config_name(suggested_name)
        } else {
            suggested_name.to_string()
        };

        self.set_selected_config_id(None, cx);
        self.selection.loading_config_id = None;
        self.selection.loading_config_path = None;
        self.selection.loaded_config = None;
        self.set_unsaved_draft(name.clone().into(), text.clone(), cx);
        if let Some(name_input) = self.configs_name_input(cx) {
            name_input.update(cx, |input, cx| {
                input.set_value(name.clone(), window, cx);
            });
        }
        if let Some(config_input) = self.configs_config_input(cx) {
            config_input.update(cx, |input, cx| {
                input.set_value(text, window, cx);
            });
        }
        self.set_status("Pasted config into draft");
        cx.notify();
    }

    fn save_draft(&mut self, force_new: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.ensure_inputs(window, cx);
        self.sync_draft_from_inputs(cx);
        self.apply_draft_validation(cx);
        let draft = self.configs_draft_snapshot(cx);

        let name = draft.name.to_string();
        let name = name.trim();
        if name.is_empty() {
            self.set_error("Tunnel name is required");
            cx.notify();
            return;
        }

        let text = draft.text.clone();
        if text.as_ref().trim().is_empty() {
            self.set_error("Config text is required");
            cx.notify();
            return;
        }

        let endpoint_family = match &draft.validation {
            DraftValidationState::Valid {
                endpoint_family, ..
            } => *endpoint_family,
            DraftValidationState::Invalid { line, message, .. } => {
                self.set_error(match line {
                    Some(line) => format!("Invalid config: line {line}: {message}"),
                    None => format!("Invalid config: {message}"),
                });
                cx.notify();
                return;
            }
            DraftValidationState::Idle => {
                self.set_error("Config text is required");
                cx.notify();
                return;
            }
        };

        let source_id = if force_new { None } else { draft.source_id };

        if self
            .configs
            .iter()
            .any(|entry| entry.name == name && Some(entry.id) != source_id)
        {
            self.set_error("Tunnel name already exists");
            cx.notify();
            return;
        }

        let name = name.to_string();
        let storage = match self.configs.ensure_storage() {
            Ok(storage) => storage,
            Err(err) => {
                self.set_error(err);
                cx.notify();
                return;
            }
        };

        let (id, storage_path, source) = match source_id.and_then(|id| self.configs.find_by_id(id))
        {
            Some(cfg) => (cfg.id, cfg.storage_path, cfg.source),
            None => {
                let id = self.configs.alloc_config_id();
                let storage_path = persistence::config_path(&storage, id);
                (id, storage_path, ConfigSource::Paste)
            }
        };

        let name_lower = name.to_lowercase();
        let text_for_write = text.to_string();
        let text_for_state = text.clone();

        self.set_editor_operation(Some(EditorOperation::Saving), cx);
        self.set_status("Saving config...");
        cx.notify();

        let storage_path_for_write = storage_path.clone();
        let view = cx.weak_entity();
        window
            .spawn(cx, async move |cx| {
                let write_task = cx.background_spawn(async move {
                    persistence::write_config_text(&storage_path_for_write, &text_for_write)
                });
                let result = write_task.await;
                view.update_in(cx, |this, window, cx| {
                    this.set_editor_operation(None, cx);
                    match result {
                        Ok(()) => {
                            this.insert_or_update_config(
                                TunnelConfig {
                                    id,
                                    name: name.to_string(),
                                    name_lower,
                                    text: Some(text_for_state),
                                    source,
                                    storage_path,
                                    endpoint_family,
                                },
                                window,
                                cx,
                            );
                            this.persist_state_async(cx);
                            if force_new {
                                this.set_status(format!("Saved {name} as a new config"));
                            } else {
                                this.set_status("Saved tunnel");
                            }
                            if let Some(action) = this.take_configs_pending_action(cx) {
                                this.run_pending_draft_action(action, window, cx);
                            }
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

    /// 保存当前 draft 到当前选中的配置 ID。
    pub(crate) fn handle_save_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_draft(false, window, cx);
    }

    pub(crate) fn handle_save_and_restart_click(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_configs_pending_action(Some(PendingDraftAction::RestartTunnel), cx);
        self.save_draft(false, window, cx);
    }

    /// 将当前 draft 另存为新的配置条目。
    pub(crate) fn handle_save_as_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_draft(true, window, cx);
    }

    /// 仅修改配置名称，不改内容。
    ///
    /// 说明：重命名时同步更新运行中的隧道名称，避免 UI 状态与引擎名称不一致。
    pub(crate) fn handle_rename_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // 仅更新名称，不修改配置文本。
        self.ensure_inputs(window, cx);
        self.sync_draft_from_inputs(cx);
        self.apply_draft_validation(cx);
        let draft = self.configs_draft_snapshot(cx);
        let new_name = draft.name.to_string();
        let new_name = new_name.trim();
        if new_name.is_empty() {
            self.set_error("Tunnel name is required");
            cx.notify();
            return;
        }

        let Some(config_id) = draft.source_id.or(self.selection.selected_id) else {
            self.set_error("Select a tunnel first");
            cx.notify();
            return;
        };
        let Some(idx) = self.configs.find_index_by_id(config_id) else {
            self.set_error("Selected tunnel no longer exists");
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
        self.configs[idx].name_lower = new_name.to_lowercase();
        let updated_config = self.configs[idx].clone();
        self.upsert_configs_workspace_library_row(&updated_config, cx);
        if let Some(loaded) = &mut self.selection.loaded_config {
            if loaded.name == old_name {
                loaded.name = new_name.to_string();
            }
        }
        if draft.source_id == Some(config_id) {
            let workspace = self.ensure_configs_workspace(cx);
            let base_name: SharedString = new_name.to_string().into();
            workspace.update(cx, |workspace, cx| {
                workspace.draft.base_name = base_name;
                cx.notify();
            });
            self.apply_draft_validation(cx);
        }
        if self.runtime.running_name.as_deref() == Some(old_name.as_str()) {
            self.runtime.running_name = Some(new_name.to_string());
            self.runtime.runtime_revision = self.runtime.runtime_revision.wrapping_add(1);
        }
        self.set_status(format!("Renamed to {new_name}"));
        self.persist_state_async(cx);
        cx.notify();
    }

    /// 删除当前选中的配置。
    ///
    /// 说明：运行中的配置禁止删除，避免状态错乱和用户误操作。
    pub(crate) fn handle_delete_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(_config_id) = self.selection.selected_id else {
            self.set_error("Select a tunnel first");
            cx.notify();
            return;
        };
        if self.configs_draft_snapshot(cx).is_dirty() {
            self.confirm_discard_or_save(
                PendingDraftAction::DeleteCurrent,
                window,
                cx,
                "Delete config?",
                "You have unsaved changes in the current config draft.",
            );
            return;
        }
        self.open_delete_current_config_dialog(window, cx);
    }

    /// 删除指定配置：遇到运行中则阻止删除。
    ///
    /// 说明：用于单个删除或严格保护运行中隧道的场景。
    pub(crate) fn delete_configs_blocking_running(
        &mut self,
        ids: &[u64],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.delete_configs_internal(ids, DeletePolicy::BlockRunning, window, cx);
    }

    /// 删除指定配置：遇到运行中则跳过。
    ///
    /// 说明：用于批量删除场景，避免“一条运行中配置”阻断整批操作。
    pub(crate) fn delete_configs_skip_running(
        &mut self,
        ids: &[u64],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.delete_configs_internal(ids, DeletePolicy::SkipRunning, window, cx);
    }

    /// 通用删除入口：负责执行删除、清理缓存与状态同步。
    ///
    /// 说明：
    /// - ids 以配置 ID 为准，避免索引变动导致误删；
    /// - 删除成功后会更新列表、缓存与持久化；
    /// - 删除文件在后台执行，失败仅提示不阻断 UI。
    fn delete_configs_internal(
        &mut self,
        ids: &[u64],
        policy: DeletePolicy,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if ids.is_empty() {
            self.set_error("Select a tunnel first");
            cx.notify();
            return;
        }

        let ids: HashSet<u64> = ids.iter().copied().collect();
        let running_id = self.runtime.running_id;
        let running_name = self.runtime.running_name.clone();

        let mut to_delete_ids = HashSet::new();
        let mut deleted_names = Vec::new();
        let mut deleted_paths = Vec::new();
        let mut skipped_running = Vec::new();

        for cfg in &self.configs {
            if !ids.contains(&cfg.id) {
                continue;
            }
            let is_running =
                running_id == Some(cfg.id) || running_name.as_deref() == Some(cfg.name.as_str());
            if is_running {
                match policy {
                    DeletePolicy::BlockRunning => {
                        self.set_error("Stop the tunnel before deleting");
                        cx.notify();
                        return;
                    }
                    DeletePolicy::SkipRunning => {
                        skipped_running.push(cfg.name.clone());
                        continue;
                    }
                }
            }

            to_delete_ids.insert(cfg.id);
            deleted_names.push(cfg.name.clone());
            deleted_paths.push(cfg.storage_path.clone());
        }

        if to_delete_ids.is_empty() {
            if !skipped_running.is_empty() {
                self.set_status(format_delete_status(&[], skipped_running.len()));
            } else {
                self.set_error("No configs selected");
            }
            cx.notify();
            return;
        }

        self.set_editor_operation(Some(EditorOperation::Deleting), cx);
        let prev_selected_id = self.selection.selected_id;
        let prev_selected_idx = prev_selected_id.and_then(|id| self.configs.find_index_by_id(id));

        for id in &to_delete_ids {
            self.stats.traffic.remove_config(*id);
        }

        self.configs.retain(|cfg| !to_delete_ids.contains(&cfg.id));
        self.remove_configs_workspace_library_rows(&to_delete_ids, cx);

        let deleted_paths_set: HashSet<PathBuf> = deleted_paths.iter().cloned().collect();
        self.selection
            .config_text_cache
            .retain(|path, _| !deleted_paths_set.contains(path));
        self.selection
            .config_text_cache_order
            .retain(|path| !deleted_paths_set.contains(path));
        self.selection
            .proxy_selected_ids
            .retain(|id| !to_delete_ids.contains(id));
        self.selection
            .endpoint_family_loading
            .retain(|id| !to_delete_ids.contains(id));
        self.selection.loading_config_id = None;
        self.selection.loading_config_path = None;

        if self.configs.is_empty() {
            self.set_selected_config_id(None, cx);
            self.clear_inputs(window, cx);
        } else if let Some(prev_id) = prev_selected_id {
            if self.configs.get_by_id(prev_id).is_some() {
                self.set_selected_config_id(Some(prev_id), cx);
            } else if let Some(prev_idx) = prev_selected_idx {
                let idx = prev_idx.min(self.configs.len() - 1);
                let fallback_id = self.configs[idx].id;
                self.set_selected_config_id(Some(fallback_id), cx);
                self.load_config_into_inputs(fallback_id, window, cx);
            } else {
                self.set_selected_config_id(None, cx);
                self.clear_inputs(window, cx);
            }
        } else {
            self.set_selected_config_id(None, cx);
            self.clear_inputs(window, cx);
        }

        self.set_status(format_delete_status(&deleted_names, skipped_running.len()));
        self.persist_state_async(cx);
        self.set_editor_operation(None, cx);
        cx.notify();

        cx.spawn(async move |view, cx| {
            // 后台删除磁盘文件：避免阻塞 UI，
            // 同时允许文件不存在的情况（已经手动删除）。
            let delete_task = cx.background_spawn(async move {
                let mut first_error: Option<std::io::Error> = None;
                for path in deleted_paths {
                    match std::fs::remove_file(&path) {
                        Ok(()) => {}
                        Err(err) if err.kind() == ErrorKind::NotFound => {}
                        Err(err) => {
                            first_error = Some(err);
                            break;
                        }
                    }
                }
                first_error
            });
            if let Some(err) = delete_task.await {
                view.update(cx, |this, cx| {
                    this.set_error(format!("Remove file failed: {err}"));
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// 将当前配置复制到剪贴板。
    ///
    /// 说明：该操作不会改变模型，仅提供快速复制能力。
    pub(crate) fn handle_copy_click(&mut self, cx: &mut Context<Self>) {
        // 直接复制配置文本到剪贴板。
        let Some(selected) = self.selected_config().cloned() else {
            self.set_error("Select a tunnel first");
            cx.notify();
            return;
        };
        // 优先取内存/缓存，避免无谓 IO。
        let cached_text = self.cached_config_text(&selected.storage_path);
        let text = selected.text.clone().or(cached_text);
        if let Some(text) = text {
            cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
            self.set_status("Config copied to clipboard");
            cx.notify();
            return;
        }

        self.set_status("Loading config...");
        cx.notify();

        cx.spawn(async move |view, cx| {
            let path_for_cache = selected.storage_path.clone();
            let read_task =
                cx.background_spawn(async move { std::fs::read_to_string(&selected.storage_path) });
            let result = read_task.await;
            view.update(cx, |this, cx| {
                // 注意：复制场景不改变选中项，因此只需检查是否仍选中同一配置。
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
                        this.set_error(format!("Read failed: {err}"));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 获取当前选中的配置引用。
    ///
    /// 说明：统一入口避免到处直接访问 self.configs。
    pub(crate) fn selected_config(&self) -> Option<&TunnelConfig> {
        self.selection
            .selected_id
            .and_then(|id| self.configs.get_by_id(id))
    }

    /// 清空输入框内容。
    ///
    /// 说明：用于删除最后一个配置等场景，防止残留旧值。
    pub(crate) fn clear_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.selection.loaded_config = None;
        self.selection.loading_config_id = None;
        self.selection.loading_config_path = None;
        let workspace = self.ensure_configs_workspace(cx);
        workspace.update(cx, |workspace, cx| {
            workspace.draft = ConfigDraftState::new();
            cx.notify();
        });
        self.refresh_configs_workspace_row_flags(cx);
        self.set_editor_operation(None, cx);
        if let Some(name_input) = self.configs_name_input(cx) {
            name_input.update(cx, |input, cx| input.set_value("", window, cx));
        }
        if let Some(config_input) = self.configs_config_input(cx) {
            config_input.update(cx, |input, cx| input.set_value("", window, cx));
        }
        self.sync_configs_selection_snapshot(cx);
    }

    /// 生成一个不会与现有配置重名的名称。
    ///
    /// 说明：用于粘贴或导入场景的自动命名。
    pub(crate) fn next_config_name(&self, base: &str) -> String {
        next_available_name(self.configs.iter().map(|cfg| cfg.name.as_str()), base)
    }

    pub(crate) fn schedule_endpoint_family_refresh(
        &mut self,
        config_id: u64,
        text: Option<SharedString>,
        storage_path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        if self.selection.endpoint_family_loading.contains(&config_id) {
            return;
        }
        let Some(config) = self.configs.get_mut_by_id(config_id) else {
            return;
        };
        config.endpoint_family = EndpointFamily::Unknown;
        self.selection.endpoint_family_loading.insert(config_id);

        cx.spawn(async move |view, cx| {
            let refresh_task = cx.background_spawn(async move {
                let text = match text {
                    Some(text) => Some(text.to_string()),
                    None => std::fs::read_to_string(&storage_path).ok(),
                };
                let text = text?;
                Some(resolve_endpoint_family_from_text(text).await)
            });
            let family = refresh_task.await;
            view.update(cx, |this, cx| {
                this.selection.endpoint_family_loading.remove(&config_id);
                let Some(family) = family else {
                    return;
                };
                let Some(config) = this.configs.get_mut_by_id(config_id) else {
                    return;
                };
                if config.endpoint_family != family {
                    config.endpoint_family = family;
                    let updated_config = config.clone();
                    this.upsert_configs_workspace_library_row(&updated_config, cx);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }
}

pub(crate) fn text_hash(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

/// 格式化删除后的状态提示文案。
///
/// 说明：尽量简洁，同时覆盖“仅跳过/仅删除/删除+跳过”三类场景。
fn format_delete_status(deleted_names: &[String], skipped_running: usize) -> String {
    let deleted_count = deleted_names.len();
    if deleted_count == 0 && skipped_running > 0 {
        if skipped_running == 1 {
            return "Skipped 1 running config".to_string();
        }
        return format!("Skipped {skipped_running} running configs");
    }
    if deleted_count == 1 && skipped_running == 0 {
        return format!("Deleted {}", deleted_names[0]);
    }
    let config_word = if deleted_count == 1 {
        "config"
    } else {
        "configs"
    };
    if skipped_running > 0 {
        return format!("Deleted {deleted_count} {config_word}, skipped {skipped_running} running");
    }
    format!("Deleted {deleted_count} {config_word}")
}
