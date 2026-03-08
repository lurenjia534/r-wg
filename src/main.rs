// Windows 下把入口切到 GUI 子系统，避免额外弹出控制台黑窗。
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
#![deny(clippy::print_stdout, clippy::print_stderr)]

mod ui;

fn main() {
    // Windows helper 模式必须在创建 UI 前尽早分流；
    // 一旦当前进程接管为管理员 helper，就不再继续走主界面初始化。
    if r_wg::backend::wg::maybe_run_elevated_helper() {
        return;
    }

    // 普通 UI / 已提权本地模式仍然保留原有 MtuWatcher。
    let _mtu = gotatun::tun::MtuWatcher::new(1500);
    r_wg::log::init();
    ui::run();
}
