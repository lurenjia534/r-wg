// Configs workspace bootstrap, snapshots, and responsive page selection.

const CONFIGS_DESKTOP_BREAKPOINT: f32 = 1420.0;
const CONFIGS_COMPACT_BREAKPOINT: f32 = 1040.0;
const CONFIGS_LIBRARY_ROW_HEIGHT: f32 = 76.0;
const CONFIGS_LIBRARY_SCROLL_STATE_ID: &str = "configs-library-scroll";
const CONFIGS_MEDIUM_INSPECTOR_HEIGHT: f32 = 328.0;

struct ConfigsRuntimeView {
    selected_id: Option<u64>,
    latest_status: String,
    last_error: String,
    running_name: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConfigsLayoutMode {
    Desktop,
    Medium,
    Compact,
}

impl WgApp {
    pub(crate) fn ensure_configs_workspace(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Entity<ConfigsWorkspace> {
        if let Some(workspace) = self.ui.configs_workspace.clone() {
            return workspace;
        }
        let app = cx.entity();
        let workspace = cx.new(|_| ConfigsWorkspace::new(app));
        self.ui.configs_workspace = Some(workspace.clone());
        workspace
    }
}

impl Render for ConfigsWorkspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let workspace_handle = cx.entity();
        self.ensure_inputs(window, cx);
        let app_handle = self.app.clone();
        let (data, runtime) = {
            let app = app_handle.read(cx);
            self.initialize_from_app(app);
            (
                ConfigsViewData::from_editor(
                    app,
                    self.draft.clone(),
                    self.operation.clone(),
                    self.has_selection,
                    self.title_editing,
                ),
                ConfigsRuntimeView {
                    selected_id: app.selection.selected_id,
                    latest_status: app.ui.status.to_string(),
                    last_error: app
                        .ui
                        .last_error
                        .clone()
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "None".to_string()),
                    running_name: app
                        .runtime
                        .running_name
                        .clone()
                        .unwrap_or_else(|| "-".to_string()),
                },
            )
        };
        let name_input = self
            .name_input
            .clone()
            .expect("name input should be initialized");
        let library_search_input = self
            .library_search_input
            .clone()
            .expect("library search input should be initialized");
        let config_input = self
            .config_input
            .clone()
            .expect("config input should be initialized");

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .child(render_configs_page(
                &app_handle,
                &workspace_handle,
                &runtime,
                self.primary_pane,
                self.inspector_tab,
                &self.library_rows,
                &library_search_input,
                self.library_width,
                self.inspector_width,
                &data,
                &name_input,
                &config_input,
                window,
                cx,
            ))
    }
}

#[allow(clippy::too_many_arguments)]
fn render_configs_page(
    app_handle: &Entity<WgApp>,
    workspace: &Entity<ConfigsWorkspace>,
    runtime: &ConfigsRuntimeView,
    primary_pane: ConfigsPrimaryPane,
    inspector_tab: ConfigInspectorTab,
    library_rows: &Arc<Vec<ConfigsLibraryRow>>,
    library_search_input: &Entity<InputState>,
    library_width: f32,
    inspector_width: f32,
    data: &ConfigsViewData,
    name_input: &Entity<InputState>,
    config_input: &Entity<InputState>,
    window: &mut Window,
    cx: &mut Context<ConfigsWorkspace>,
) -> Div {
    let mode = if window.viewport_size().width >= px(CONFIGS_DESKTOP_BREAKPOINT) {
        ConfigsLayoutMode::Desktop
    } else if window.viewport_size().width >= px(CONFIGS_COMPACT_BREAKPOINT) {
        ConfigsLayoutMode::Medium
    } else {
        ConfigsLayoutMode::Compact
    };
    let workspace = match mode {
        ConfigsLayoutMode::Desktop => render_configs_desktop_layout(
            app_handle,
            workspace,
            runtime,
            inspector_tab,
            library_rows,
            library_search_input,
            library_width,
            inspector_width,
            data,
            name_input,
            config_input,
            window,
            cx,
        ),
        ConfigsLayoutMode::Medium => render_configs_medium_layout(
            app_handle,
            workspace,
            runtime,
            inspector_tab,
            library_rows,
            library_search_input,
            library_width,
            inspector_width,
            data,
            name_input,
            config_input,
            window,
            cx,
        ),
        ConfigsLayoutMode::Compact => render_configs_compact_layout(
            app_handle,
            workspace,
            runtime,
            inspector_tab,
            primary_pane,
            library_rows,
            library_search_input,
            data,
            name_input,
            config_input,
            window,
            cx,
        ),
    };

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.0))
        .rounded_lg()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().tiles)
        .overflow_hidden()
        .child(render_configs_shell_header(data, cx))
        .child(workspace)
}
