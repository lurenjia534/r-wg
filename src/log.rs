pub fn enabled() -> bool {
    std::env::var_os("RWG_LOG").is_some()
}

pub fn log(scope: &str, message: String) {
    if enabled() {
        eprintln!("[r-wg][{scope}] {message}");
    }
}
