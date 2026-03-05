// Windows 下将入口切换为 GUI 子系统，避免主进程启动时额外弹出控制台黑窗。
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
#![deny(clippy::print_stdout, clippy::print_stderr)]

mod ui;

fn main() {
    let _mtu = gotatun::tun::MtuWatcher::new(1500);
    r_wg::log::init();
    ui::run();
}
