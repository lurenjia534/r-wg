#![deny(clippy::print_stdout, clippy::print_stderr)]

mod ui;

fn main() {
    let _mtu = gotatun::tun::MtuWatcher::new(1500);
    r_wg::log::init();
    ui::run();
}
