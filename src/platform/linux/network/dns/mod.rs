//! DNS 配置与回滚逻辑。
//!
//! 设计目标：
//! - 优先使用系统原生后端（systemd-resolved / resolvconf / NetworkManager），最后兜底写入 `/etc/resolv.conf`。
//! - 能按“接口级 DNS”处理时不改全局文件；仅当 resolv.conf 是普通文件时才会备份并写入。
//! - 严格验证解析器配置，避免多余 DNS（例如 RDNSS 注入）导致泄漏。
//! - NM 路径在必要时触发 down/up 重连以清掉残留 DNS。
//! - 失败向上抛错，由上层决定是否允许继续启动。

mod backend;
mod command;
mod probe;
mod types;
mod verify;

use std::net::IpAddr;

use self::backend::{
    apply_network_manager, apply_resolv_conf_file, apply_resolvconf, apply_resolved,
    cleanup_network_manager, cleanup_resolv_conf_file, cleanup_resolvconf, cleanup_resolved,
};
use self::command::resolve_command;
use self::probe::{dns_backend_order, log_resolv_conf_info, read_resolv_conf_info};
pub(super) use self::types::DnsState;
use self::types::{DnsBackend, DnsBackendKind};
use super::NetworkError;
use crate::log::events::dns as log_dns;

#[cfg(test)]
use self::backend::build_resolv_conf_contents;
#[cfg(test)]
use self::probe::{detect_preferred_backend, dns_backend_order_from_probes};
#[cfg(test)]
use self::verify::read_resolv_conf_servers;

/// 应用 DNS 配置。
///
/// 逻辑说明：
/// 1) 先检测 resolv.conf 当前状态，推断“优先后端”。
/// 2) 依序尝试 resolved/resolvconf/nmcli/直接写文件。
/// 3) 任一后端成功则返回；失败则记录并继续下一后端。
pub(super) async fn apply_dns(
    tun_name: &str,
    servers: &[IpAddr],
    search: &[String],
) -> Result<DnsState, NetworkError> {
    let info = read_resolv_conf_info();
    log_resolv_conf_info(&info);

    let backends = dns_backend_order(&info).await;
    let backend_order = backends
        .iter()
        .map(|backend| backend.as_str())
        .collect::<Vec<_>>()
        .join(" -> ");
    log_dns::backend_order(&backend_order);

    let mut last_error: Option<NetworkError> = None;

    for backend in backends {
        match backend {
            DnsBackendKind::Resolved => {
                let Some(resolvectl) = resolve_command("resolvectl") else {
                    log_dns::backend_not_found("resolvectl");
                    continue;
                };
                log_dns::backend_selected("resolvectl", &resolvectl);
                match apply_resolved(&resolvectl, tun_name, servers, search).await {
                    Ok(()) => {
                        return Ok(DnsState {
                            backend: DnsBackend::Resolved,
                        });
                    }
                    Err(err) => {
                        log_dns::backend_failed("resolvectl", &err);
                        last_error = Some(err);
                    }
                }
            }
            DnsBackendKind::Resolvconf => {
                let Some(resolvconf) = resolve_command("resolvconf") else {
                    log_dns::backend_not_found("resolvconf");
                    continue;
                };
                log_dns::backend_selected("resolvconf", &resolvconf);
                match apply_resolvconf(&resolvconf, tun_name, servers, search).await {
                    Ok(()) => {
                        return Ok(DnsState {
                            backend: DnsBackend::Resolvconf,
                        });
                    }
                    Err(err) => {
                        log_dns::backend_failed("resolvconf", &err);
                        last_error = Some(err);
                    }
                }
            }
            DnsBackendKind::NetworkManager => {
                let Some(nmcli) = resolve_command("nmcli") else {
                    log_dns::backend_not_found("nmcli");
                    continue;
                };
                log_dns::backend_selected("nmcli", &nmcli);
                match apply_network_manager(&nmcli, servers, search).await {
                    Ok(state) => {
                        return Ok(state);
                    }
                    Err(err) => {
                        log_dns::backend_failed("nmcli", &err);
                        last_error = Some(err);
                    }
                }
            }
            DnsBackendKind::ResolvConf => match apply_resolv_conf_file(&info, servers, search) {
                Ok(state) => {
                    return Ok(state);
                }
                Err(err) => {
                    log_dns::resolv_conf_failed(&err);
                    last_error = Some(err);
                }
            },
        }
    }

    // 所有后端都失败时，返回最后一个错误或“不支持”错误。
    Err(last_error.unwrap_or(NetworkError::DnsNotSupported))
}

/// 清理 DNS 配置。
///
/// 注意：必须与 apply_dns 使用的后端匹配，避免误删或破坏用户系统配置。
pub(super) async fn cleanup_dns(tun_name: &str, state: DnsState) -> Result<(), NetworkError> {
    match state.backend {
        DnsBackend::Resolved => {
            if let Some(resolvectl) = resolve_command("resolvectl") {
                cleanup_resolved(&resolvectl, tun_name).await?;
            }
        }
        DnsBackend::Resolvconf => {
            if let Some(resolvconf) = resolve_command("resolvconf") {
                cleanup_resolvconf(&resolvconf, tun_name).await?;
            }
        }
        DnsBackend::NetworkManager { connections } => {
            if let Some(nmcli) = resolve_command("nmcli") {
                cleanup_network_manager(&nmcli, &connections).await;
            }
        }
        DnsBackend::ResolvConf { path, original } => {
            cleanup_resolv_conf_file(&path, &original)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::types::ResolvConfInfo;
    use super::verify::{verify_resolv_conf_servers, wait_for_resolv_conf_servers};
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration as StdDuration, SystemTime, UNIX_EPOCH};

    static TEST_FILE_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn resolv_conf_info(
        is_symlink: bool,
        target: Option<&str>,
        contents: Option<&str>,
    ) -> ResolvConfInfo {
        ResolvConfInfo {
            path: PathBuf::from("/etc/resolv.conf"),
            is_symlink,
            target: target.map(PathBuf::from),
            contents: contents.map(str::to_string),
        }
    }

    #[test]
    fn backend_order_promotes_preferred_backend() {
        let info = resolv_conf_info(
            true,
            Some("/run/NetworkManager/resolv.conf"),
            Some("# Generated by NetworkManager"),
        );

        let order =
            dns_backend_order_from_probes(&info, detect_preferred_backend(&info), true, true, true);

        assert_eq!(
            order,
            vec![
                DnsBackendKind::NetworkManager,
                DnsBackendKind::Resolved,
                DnsBackendKind::Resolvconf,
            ]
        );
    }

    #[test]
    fn backend_order_keeps_resolv_conf_as_regular_file_fallback() {
        let info = resolv_conf_info(false, None, Some("nameserver 1.1.1.1"));

        let order = dns_backend_order_from_probes(
            &info,
            detect_preferred_backend(&info),
            false,
            true,
            false,
        );

        assert_eq!(
            order,
            vec![DnsBackendKind::ResolvConf, DnsBackendKind::Resolvconf]
        );
    }

    #[test]
    fn build_resolv_conf_contents_renders_search_then_nameservers() {
        let contents = build_resolv_conf_contents(
            &[
                "1.1.1.1".parse().expect("valid IPv4"),
                "2606:4700:4700::1111".parse().expect("valid IPv6"),
            ],
            &["corp.example".to_string(), "lan.example".to_string()],
        );

        assert_eq!(
            contents,
            concat!(
                "# Generated by r-wg\n",
                "search corp.example lan.example\n",
                "nameserver 1.1.1.1\n",
                "nameserver 2606:4700:4700::1111\n"
            )
        );
    }

    #[test]
    fn detect_preferred_backend_identifies_systemd_resolved() {
        let info = resolv_conf_info(
            true,
            Some("/run/systemd/resolve/stub-resolv.conf"),
            Some("# This file is managed by systemd-resolved"),
        );

        assert_eq!(
            detect_preferred_backend(&info),
            Some(DnsBackendKind::Resolved)
        );
    }

    #[test]
    fn detect_preferred_backend_identifies_resolvconf() {
        let info = resolv_conf_info(
            true,
            Some("/run/resolvconf/resolv.conf"),
            Some("# Generated by resolvconf"),
        );

        assert_eq!(
            detect_preferred_backend(&info),
            Some(DnsBackendKind::Resolvconf)
        );
    }

    #[test]
    fn detect_preferred_backend_identifies_regular_resolv_conf() {
        let info = resolv_conf_info(false, None, Some("nameserver 9.9.9.9"));

        assert_eq!(
            detect_preferred_backend(&info),
            Some(DnsBackendKind::ResolvConf)
        );
    }

    #[test]
    fn read_resolv_conf_servers_ignores_comments_and_strips_zone_index() {
        let fixture = write_temp_resolv_conf(
            "comments-zone",
            concat!(
                "# comment\n",
                "\n",
                "search corp.example\n",
                "nameserver 1.1.1.1\n",
                "nameserver fe80::1%wg0\n"
            ),
        );

        let (v4, v6) =
            read_resolv_conf_servers(fixture.path()).expect("resolv.conf contents should parse");

        assert_eq!(v4, vec![ip("1.1.1.1")]);
        assert_eq!(v6, vec![ip("fe80::1")]);
    }

    #[test]
    fn read_resolv_conf_servers_rejects_invalid_nameserver() {
        let fixture = write_temp_resolv_conf("invalid-entry", "nameserver not-an-ip\n");

        let err = read_resolv_conf_servers(fixture.path()).expect_err("invalid entry should fail");
        assert!(matches!(
            err,
            NetworkError::DnsVerifyFailed(ref message)
                if message == "unparseable DNS entry: not-an-ip"
        ));
    }

    #[test]
    fn verify_resolv_conf_servers_accepts_matching_dual_stack_entries() {
        let fixture = write_temp_resolv_conf(
            "verify-ok",
            concat!(
                "# Generated by test\n",
                "nameserver 1.1.1.1\n",
                "nameserver 2606:4700:4700::1111\n"
            ),
        );
        let servers = vec![ip("1.1.1.1"), ip("2606:4700:4700::1111")];

        verify_resolv_conf_servers(fixture.path(), &servers).expect("matching DNS should verify");
    }

    #[test]
    fn verify_resolv_conf_servers_reports_missing_ipv4() {
        let fixture = write_temp_resolv_conf("missing-ipv4", "nameserver 2606:4700:4700::1111\n");
        let servers = vec![ip("1.1.1.1"), ip("2606:4700:4700::1111")];

        let err = verify_resolv_conf_servers(fixture.path(), &servers)
            .expect_err("missing IPv4 must fail");
        assert!(matches!(
            err,
            NetworkError::DnsVerifyFailed(ref message) if message == "missing IPv4 DNS"
        ));
    }

    #[test]
    fn verify_resolv_conf_servers_rejects_unexpected_ipv4() {
        let fixture = write_temp_resolv_conf(
            "extra-ipv4",
            concat!("nameserver 1.1.1.1\n", "nameserver 2606:4700:4700::1111\n"),
        );
        let servers = vec![ip("2606:4700:4700::1111")];

        let err = verify_resolv_conf_servers(fixture.path(), &servers)
            .expect_err("unexpected IPv4 must fail");
        assert!(matches!(
            err,
            NetworkError::DnsVerifyFailed(ref message) if message == "unexpected IPv4 DNS: 1.1.1.1"
        ));
    }

    #[test]
    fn verify_resolv_conf_servers_rejects_unexpected_ipv6() {
        let fixture = write_temp_resolv_conf(
            "extra-ipv6",
            concat!("nameserver 1.1.1.1\n", "nameserver 2606:4700:4700::1111\n"),
        );
        let servers = vec![ip("1.1.1.1")];

        let err = verify_resolv_conf_servers(fixture.path(), &servers)
            .expect_err("unexpected IPv6 must fail");
        assert!(matches!(
            err,
            NetworkError::DnsVerifyFailed(ref message)
                if message == "unexpected IPv6 DNS: 2606:4700:4700::1111"
        ));
    }

    #[test]
    fn wait_for_resolv_conf_servers_retries_until_success() {
        let fixture = write_temp_resolv_conf("wait-success", "nameserver 9.9.9.9\n");
        let path_for_writer = fixture.path().to_path_buf();
        let servers = vec![ip("1.1.1.1")];

        let writer = std::thread::spawn(move || {
            std::thread::sleep(StdDuration::from_millis(100));
            fs::write(path_for_writer, "nameserver 1.1.1.1\n")
                .expect("should update resolv.conf fixture");
        });

        test_runtime().block_on(async {
            wait_for_resolv_conf_servers(fixture.path(), &servers)
                .await
                .expect("verify should succeed after retry");
        });

        writer.join().expect("writer thread should complete");
    }

    #[test]
    fn wait_for_resolv_conf_servers_returns_last_verify_error_on_timeout() {
        let fixture = write_temp_resolv_conf("wait-timeout", "nameserver 9.9.9.9\n");
        let servers = vec![ip("1.1.1.1")];

        let err = test_runtime().block_on(async {
            wait_for_resolv_conf_servers(fixture.path(), &servers)
                .await
                .expect_err("verify should time out with last DNS error")
        });

        assert!(matches!(
            err,
            NetworkError::DnsVerifyFailed(ref message)
                if message == "unexpected IPv4 DNS: 9.9.9.9"
        ));
    }

    fn write_temp_resolv_conf(label: &str, contents: &str) -> TestResolvConf {
        let path = temp_resolv_conf_path(label);
        fs::write(&path, contents).expect("should create temp resolv.conf fixture");
        TestResolvConf { path }
    }

    fn temp_resolv_conf_path(label: &str) -> PathBuf {
        let unique = TEST_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "r-wg-dns-test-{label}-{}-{nanos}-{unique}.conf",
            std::process::id()
        ))
    }

    fn test_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime should build")
    }

    fn ip(value: &str) -> IpAddr {
        value.parse().expect("test IP should be valid")
    }

    struct TestResolvConf {
        path: PathBuf,
    }

    impl TestResolvConf {
        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestResolvConf {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
        }
    }
}
