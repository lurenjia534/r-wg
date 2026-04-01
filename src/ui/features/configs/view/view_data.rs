use crate::ui::features::configs::state::{
    ConfigDraftState, DraftValidationState, EditorOperation,
};
use crate::ui::state::{ConfigSource, TunnelConfig, WgApp};
use crate::ui::view::shared::ViewData;

/// Configs 页的局部渲染快照。
///
/// 这个 ViewModel 的目标不是减少字段数量，而是把 “Configs 页需要的 editor 状态”
/// 从 `ConfigsWorkspace` 的局部状态里显式整理成一份视图快照：
/// - 视图层只依赖这份快照，不再散落读取页面状态；
/// - 页面级状态和共享派生信息也在这里清楚分层，避免渲染逻辑回退到根状态。
pub(crate) struct ConfigsViewData {
    pub(crate) shared: ViewData,
    pub(crate) draft: ConfigDraftState,
    pub(crate) has_selection: bool,
    pub(crate) title_editing: bool,
    pub(crate) is_busy: bool,
    pub(crate) has_saved_source: bool,
    pub(crate) is_running_draft: bool,
    pub(crate) can_save: bool,
    pub(crate) can_restart: bool,
    pub(crate) title: String,
    pub(crate) source_summary: String,
    pub(crate) runtime_note: String,
}

impl ConfigsViewData {
    pub(crate) fn from_editor(
        app: &WgApp,
        draft: ConfigDraftState,
        operation: Option<EditorOperation>,
        has_selection: bool,
        title_editing: bool,
    ) -> Self {
        let shared = ViewData::from_editor(app, &draft, operation.as_ref());
        let has_saved_source = draft.source_id.is_some();
        let is_busy = operation.is_some();
        let is_running_draft = app.runtime.running && app.runtime.running_id == draft.source_id;
        let source_summary = draft
            .source_id
            .and_then(|id| app.configs.get_by_id(id))
            .map(config_origin_summary)
            .unwrap_or_else(|| "Unsaved draft".to_string());
        let title = current_draft_title(&draft);
        let runtime_note = if is_running_draft && draft.is_dirty() {
            "Editing a running tunnel. Saved changes take effect after restart.".to_string()
        } else if is_running_draft {
            "This tunnel is currently running.".to_string()
        } else {
            "Changes affect the saved config after you save them.".to_string()
        };
        let can_save = !is_busy
            && !matches!(draft.validation, DraftValidationState::Idle)
            && (draft.is_dirty() || !has_saved_source);
        let can_restart = !is_busy
            && is_running_draft
            && draft.is_dirty()
            && matches!(draft.validation, DraftValidationState::Valid { .. });

        Self {
            shared,
            draft,
            has_selection,
            title_editing,
            is_busy,
            has_saved_source,
            is_running_draft,
            can_save,
            can_restart,
            title,
            source_summary,
            runtime_note,
        }
    }
}

fn current_draft_title(draft: &ConfigDraftState) -> String {
    let name = draft.name.as_ref().trim();
    if !name.is_empty() {
        return name.to_string();
    }
    if draft.source_id.is_none() {
        return "New Draft".to_string();
    }
    "Untitled Config".to_string()
}

fn config_origin_summary(config: &TunnelConfig) -> String {
    match &config.source {
        ConfigSource::File { origin_path } => origin_path
            .as_ref()
            .map(|path| format!("Imported from {}", path.display()))
            .unwrap_or_else(|| "Imported config".to_string()),
        ConfigSource::Paste => "Created in app storage".to_string(),
    }
}
