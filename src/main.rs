//! r-wg 程序入口点
//!
//! 本文件是应用程序的起点，负责：
//! 1. 平台特定配置（Windows GUI 子系统）
//! 2. 特权后端服务模式检测与分流
//! 3. 日志系统初始化
//! 4. 单实例检测与主窗口创建

// Windows 下把入口切到 GUI 子系统，避免额外弹出控制台黑窗。
// 这对于 GUI 应用是必须的，否则会在运行时创建一个难看的控制台窗口。
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
// 禁止使用 print!/println!，强制使用 tracing 进行日志输出，
// 确保所有输出都经过日志系统的过滤和格式化。
#![deny(clippy::print_stdout, clippy::print_stderr)]

/// UI 模块
///
/// 包含 GPUI 应用的所有代码：
/// - 视图层 (view/)
/// - 状态管理 (state/)
/// - 功能控制器 (features/)
/// - 系统托盘 (tray/)
/// - 持久化 (persistence/)
mod ui;

/// 应用程序主入口点
///
/// 初始化流程：
/// ```text
/// main()
///   ├── maybe_run_privileged_backend()  // 检测是否应以服务模式运行
///   │   ├── Linux: 检查 systemd 服务状态
///   │   ├── Windows: 检查 SCM 服务状态
///   │   └── 若为服务模式: 执行服务逻辑后退出
///   │
///   ├── log::init()  // 初始化 tracing 日志系统
///   │
///   └── ui::single_instance::startup()  // 单实例检测
///       ├── Primary(instance): 创建主窗口并运行 GPUI
///       ├── Secondary: 已有实例在运行，发送信号后退出
///       └── Error: 启动失败，报告错误
/// ```
fn main() {
    // --------------------------------------------------------------------
    // 步骤 1: 特权后端模式检测
    // --------------------------------------------------------------------
    // Windows helper / Linux privileged service 都必须在创建 UI 前尽早分流。
    // 这是因为：
    // - 特权服务需要监听特定的 IPC 端点
    // - 如果先创建了 UI，后续再分流会导致端口冲突
    // - 服务模式和 UI 模式是互斥的，只能运行其中一个
    //
    // 在 Linux 上，这会检查是否应作为 systemd 服务运行
    // 在 Windows 上，这会检查是否应作为 SCM 服务运行
    // 如果返回 true，说明当前进程应以服务模式运行，main() 直接返回
    if r_wg::backend::wg::maybe_run_privileged_backend() {
        return;
    }

    // --------------------------------------------------------------------
    // 步骤 2: 初始化日志系统
    // --------------------------------------------------------------------
    // 日志必须在任何其他操作之前初始化，
    // 确保所有模块都能正确地输出日志。
    r_wg::log::init();

    // --------------------------------------------------------------------
    // 步骤 3: 单实例检测与 UI 启动
    // --------------------------------------------------------------------
    // 使用单实例机制确保同一时间只有一个 r-wg 进程运行。
    // 如果已有实例在运行，新实例会向现有实例发送信号后退出。
    match ui::single_instance::startup() {
        // 主实例：成功获取锁，可以创建 UI 窗口
        Ok(ui::single_instance::StartupDecision::Primary(primary)) => ui::run(primary),
        // 次实例：已有实例在运行，新实例直接退出
        // 这是正常行为，不需要显示错误
        Ok(ui::single_instance::StartupDecision::Secondary) => return,
        // 启动失败：可能是锁文件损坏或其他错误
        Err(err) => {
            tracing::error!("ui single-instance startup failed: {err}");
            ui::single_instance::report_startup_error(&err);
        }
    }
}
