mod controller;
mod platform;
mod types;

use gpui::{AnyWindowHandle, App, Window};
use r_wg::application::TunnelSessionService;

use super::single_instance::PrimaryInstance;
use super::state::WgApp;

/// 初始化托盘。
///
/// 行为说明：
/// - 仅在平台层成功创建托盘线程时进入“有托盘模式”；
/// - 命令消费、退出编排等逻辑交给 controller；
/// - 对外继续保持 `tray::init(...)` 入口不变。
pub(crate) fn init(
    primary: PrimaryInstance,
    window_handle: AnyWindowHandle,
    view: gpui::WeakEntity<WgApp>,
    tunnel_session: TunnelSessionService,
    cx: &mut App,
) {
    controller::init(primary, window_handle, view, tunnel_session, cx);
}

/// 判断关闭窗口时是否应拦截为“最小化到托盘”。
pub(crate) fn should_minimize_on_close(cx: &App) -> bool {
    controller::should_minimize_on_close(cx)
}

/// 隐藏窗口（平台相关）。
pub(crate) fn hide_window(window: &mut Window) {
    platform::hide_window(window);
}

/// 发送一条系统通知（隧道开/关成功与失败都会调用这个入口）。
pub(crate) fn notify_system(title: &str, message: &str, is_error: bool) {
    platform::notify_system(title, message, is_error);
}
