use gpui::{AppContext, Context, Window};
use gpui_component::input::InputState;

use super::super::state::WgApp;

impl WgApp {
    /// 确保日志输入框已创建，避免在没有窗口时初始化。
    pub(crate) fn ensure_log_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ui.log_input.is_some() {
            return;
        }

        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor("text")
                .line_number(false)
                .soft_wrap(true)
                .searchable(false)
                .placeholder("No logs captured")
        });
        self.ui.log_input = Some(input);
    }
}
