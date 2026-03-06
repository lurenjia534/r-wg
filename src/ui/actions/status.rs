use gpui::SharedString;

use super::super::state::WgApp;

impl WgApp {
    /// 更新状态栏提示。
    ///
    /// 说明：状态栏用于展示当前流程的“阶段性信息”，不会阻断交互。
    pub(crate) fn set_status(&mut self, message: impl Into<SharedString>) {
        self.ui.set_status(message);
    }

    /// 写入错误并同步状态栏。
    ///
    /// 说明：错误会覆盖状态栏文字，确保用户能立即看到失败原因。
    pub(crate) fn set_error(&mut self, message: impl Into<SharedString>) {
        self.ui.set_error(message);
    }
}
