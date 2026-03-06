/// 托盘线程向 UI 主循环发送的命令。
///
/// 设计目标：
/// - 平台层（Windows/Linux）只负责产生命令，不直接改 UI 状态；
/// - controller 在 UI 线程里统一消费命令，避免跨线程操作 UI。
#[derive(Clone, Copy, Debug)]
pub(super) enum TrayCommand {
    /// 显示并激活主窗口。
    ShowWindow,
    /// 启动隧道（对应菜单「Open Tunnel」）。
    StartTunnel,
    /// 停止隧道（对应菜单「Close Tunnel」）。
    StopTunnel,
    /// 退出应用（会先尝试停止隧道）。
    QuitApp,
}
