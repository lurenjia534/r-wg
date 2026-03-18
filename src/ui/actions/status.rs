use gpui::{AppContext, Context, SharedString};

use r_wg::backend::wg::{
    manage_privileged_service, probe_privileged_service, PrivilegedServiceAction,
};

use super::super::state::{BackendDiagnostic, WgApp};

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

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    pub(crate) fn refresh_privileged_backend_status(&mut self, cx: &mut Context<Self>) {
        // 探测放到后台线程：systemctl / socket 探测虽然不重，但它们都属于同步系统调用，
        // 不应该阻塞 UI 渲染线程。
        let last_checked = self.ui.backend.checked_at;
        self.ui
            .set_backend_diagnostic(BackendDiagnostic::checking().with_checked_at(last_checked));
        cx.notify();

        cx.spawn(async move |view, cx| {
            let status = cx
                .background_spawn(async move { probe_privileged_service() })
                .await;
            let _ = view.update(cx, |this, cx| {
                this.ui
                    .set_backend_diagnostic(BackendDiagnostic::from_probe_status(status));
                cx.notify();
            });
        })
        .detach();
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    pub(crate) fn run_privileged_backend_action(
        &mut self,
        action: PrivilegedServiceAction,
        cx: &mut Context<Self>,
    ) {
        let verb = match action {
            PrivilegedServiceAction::Install => "Installing",
            PrivilegedServiceAction::Repair => "Repairing",
            PrivilegedServiceAction::Remove => "Removing",
        };
        let last_checked = self.ui.backend.checked_at;
        self.set_status(format!("{verb} privileged backend..."));
        self.ui.set_backend_diagnostic(
            BackendDiagnostic::working(action).with_checked_at(last_checked),
        );
        // 安装/修复/移除都可能触发授权弹窗与 systemd 操作，必须异步执行；
        // 这里先把设置页状态切到 Working，避免用户误以为按钮没有响应。
        cx.notify();

        cx.spawn(async move |view, cx| {
            let result = cx
                .background_spawn(async move { manage_privileged_service(action) })
                .await;
            let _ = view.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        let done = match action {
                            PrivilegedServiceAction::Install => "Privileged backend installed",
                            PrivilegedServiceAction::Repair => "Privileged backend repaired",
                            PrivilegedServiceAction::Remove => "Privileged backend removed",
                        };
                        this.set_status(done);
                    }
                    Err(err) => {
                        let message = format!("Backend action failed: {err}");
                        this.ui.set_backend_last_error(message.clone());
                        this.set_error(message);
                    }
                }
                this.refresh_privileged_backend_status(cx);
            });
        })
        .detach();
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    pub(crate) fn refresh_privileged_backend_status(&mut self, cx: &mut Context<Self>) {
        self.ui
            .set_backend_diagnostic(BackendDiagnostic::unsupported());
        cx.notify();
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    pub(crate) fn run_privileged_backend_action(
        &mut self,
        _action: PrivilegedServiceAction,
        cx: &mut Context<Self>,
    ) {
        self.ui
            .set_backend_diagnostic(BackendDiagnostic::unsupported());
        cx.notify();
    }
}
