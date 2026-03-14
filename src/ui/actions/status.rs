use gpui::{AppContext, Context, SharedString};

#[cfg(target_os = "linux")]
use r_wg::backend::wg::{
    manage_privileged_service, probe_privileged_service, PrivilegedServiceAction,
    PrivilegedServiceStatus,
};

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

    #[cfg(target_os = "linux")]
    pub(crate) fn refresh_privileged_backend_status(&mut self, cx: &mut Context<Self>) {
        self.ui
            .set_backend_status("Checking...", "Probing Linux privileged backend...", false);
        cx.notify();

        cx.spawn(async move |view, cx| {
            let status = cx.background_spawn(async move { probe_privileged_service() }).await;
            let _ = view.update(cx, |this, cx| {
                match status {
                    PrivilegedServiceStatus::Running => this.ui.set_backend_status(
                        "Running",
                        "Linux privileged backend is currently running. It will be started on demand and can exit again after becoming idle.",
                        true,
                    ),
                    PrivilegedServiceStatus::Installed => this.ui.set_backend_status(
                        "Installed",
                        "The privileged backend is installed and socket-activated. It will start automatically when you connect a tunnel.",
                        false,
                    ),
                    PrivilegedServiceStatus::NotInstalled => this.ui.set_backend_status(
                        "Not installed",
                        "Install the privileged backend to enable tunnel control from the unprivileged UI.",
                        false,
                    ),
                    PrivilegedServiceStatus::AccessDenied => this.ui.set_backend_status(
                        "Access denied",
                        "The backend socket is reachable, but this user cannot access it. Check /run/r-wg/control.sock ownership and backend access group membership.",
                        false,
                    ),
                    PrivilegedServiceStatus::VersionMismatch { expected, actual } => {
                        this.ui.set_backend_status(
                            "Version mismatch",
                            format!(
                                "GUI expects protocol v{expected}, but the running service reports v{actual}. Repair the backend installation."
                            ),
                            false,
                        )
                    }
                    PrivilegedServiceStatus::Unreachable(message) => {
                        this.ui
                            .set_backend_status("Unreachable", message, false)
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    #[cfg(target_os = "linux")]
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
        self.set_status(format!("{verb} privileged backend..."));
        self.ui.set_backend_status(
            "Working...",
            format!("{verb} the Linux privileged backend via pkexec..."),
            false,
        );
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
                    Err(err) => this.set_error(format!("Backend action failed: {err}")),
                }
                this.refresh_privileged_backend_status(cx);
            });
        })
        .detach();
    }

    #[cfg(not(target_os = "linux"))]
    pub(crate) fn refresh_privileged_backend_status(&mut self, _cx: &mut Context<Self>) {}

    #[cfg(not(target_os = "linux"))]
    pub(crate) fn run_privileged_backend_action(&mut self, _action: (), _cx: &mut Context<Self>) {}
}
