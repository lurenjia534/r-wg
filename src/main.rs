// Windows 下把入口切到 GUI 子系统，避免额外弹出控制台黑窗。
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
#![deny(clippy::print_stdout, clippy::print_stderr)]

mod ui;

fn main() {
    // Windows helper / Linux privileged service 都必须在创建 UI 前尽早分流。
    if r_wg::backend::wg::maybe_run_privileged_backend() {
        return;
    }

    r_wg::log::init();
    ui::run();
}
