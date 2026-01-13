mod ui;

fn main() {
    let _mtu = gotatun::tun::MtuWatcher::new(1500);
    ui::run();
}
