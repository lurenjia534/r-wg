use gpui::{Context, SharedString};

use r_wg::backend::wg::PrivilegedServiceAction;

use super::super::features::daita_resources::controller as daita_resources_controller;
use super::super::features::backend_admin::controller;
use super::super::state::WgApp;

impl WgApp {
    /// 更新状态栏提示。
    ///
    /// 说明：状态栏用于展示当前流程的“阶段性信息”，不会阻断交互。
    pub(crate) fn set_status(&mut self, message: impl Into<SharedString>) -> bool {
        self.ui.set_status(message)
    }

    /// 写入错误并同步状态栏。
    ///
    /// 说明：错误会覆盖状态栏文字，确保用户能立即看到失败原因。
    pub(crate) fn set_error(&mut self, message: impl Into<SharedString>) -> bool {
        self.ui.set_error(message)
    }

    pub(crate) fn refresh_daita_resources_status(&mut self, cx: &mut Context<Self>) {
        daita_resources_controller::refresh_daita_resources_status(self, cx);
    }

    pub(crate) fn refresh_daita_resources_cache(&mut self, cx: &mut Context<Self>) {
        daita_resources_controller::refresh_daita_resources_cache(self, cx);
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    pub(crate) fn refresh_privileged_backend_status(&mut self, cx: &mut Context<Self>) {
        controller::refresh_privileged_backend_status(self, cx);
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    pub(crate) fn run_privileged_backend_action(
        &mut self,
        action: PrivilegedServiceAction,
        cx: &mut Context<Self>,
    ) {
        controller::run_privileged_backend_action(self, action, cx);
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    pub(crate) fn refresh_privileged_backend_status(&mut self, cx: &mut Context<Self>) {
        controller::refresh_privileged_backend_status(self, cx);
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    pub(crate) fn run_privileged_backend_action(
        &mut self,
        action: PrivilegedServiceAction,
        cx: &mut Context<Self>,
    ) {
        controller::run_privileged_backend_action(self, action, cx);
    }
}
