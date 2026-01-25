use gpui::SharedString;

#[cfg(target_os = "linux")]
pub(crate) fn start_permission_message() -> Option<SharedString> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let euid = parse_status_uid(&status)?;
    if euid == 0 {
        return None;
    }

    let cap_eff = parse_status_cap_eff(&status)?;
    let cap_net_admin = 1u64 << 12;
    if cap_eff & cap_net_admin != 0 {
        return None;
    }

    let exe = std::env::current_exe()
        .ok()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "target/debug/r-wg".to_string());
    Some(
        format!("需要 cap_net_admin 才能配置网络。请运行：sudo setcap cap_net_admin+ep {exe}")
            .into(),
    )
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn start_permission_message() -> Option<SharedString> {
    None
}

#[cfg(target_os = "linux")]
fn parse_status_uid(status: &str) -> Option<u32> {
    status
        .lines()
        .find(|line| line.starts_with("Uid:"))
        .and_then(|line| line.split_whitespace().nth(2))
        .and_then(|value| value.parse().ok())
}

#[cfg(target_os = "linux")]
fn parse_status_cap_eff(status: &str) -> Option<u64> {
    status
        .lines()
        .find(|line| line.starts_with("CapEff:"))
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| u64::from_str_radix(value, 16).ok())
}
