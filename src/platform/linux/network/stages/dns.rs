use crate::core::config::InterfaceConfig;
use crate::core::route_plan::{RouteApplyFailureKind, RouteApplyKind, RouteApplyReport};
use crate::log::events::dns as log_dns;

use super::super::dns::{apply_dns, DnsState};
use super::super::NetworkError;

pub(in crate::platform::linux::network) async fn apply_dns_stage(
    tun_name: &str,
    interface: &InterfaceConfig,
    report: &mut RouteApplyReport,
) -> Result<Option<DnsState>, NetworkError> {
    if interface.dns_servers.is_empty() && interface.dns_search.is_empty() {
        return Ok(None);
    }

    // DNS 失败视为致命错误，避免全隧道场景出现 DNS 泄漏。
    log_dns::apply_summary(interface.dns_servers.len(), interface.dns_search.len());
    match apply_dns(tun_name, &interface.dns_servers, &interface.dns_search).await {
        Ok(dns_state) => {
            report.push_applied_kind(
                "apply:dns",
                RouteApplyKind::Dns,
                vec![format!(
                    "Applied Linux tunnel DNS with {} server(s) and {} search domain(s).",
                    interface.dns_servers.len(),
                    interface.dns_search.len()
                )],
            );
            Ok(Some(dns_state))
        }
        Err(err) => {
            log_dns::apply_failed(&err);
            report.push_failed_kind(
                "apply:dns",
                RouteApplyKind::Dns,
                Some(classify_linux_failure(&err)),
                vec![format!("Failed to apply Linux tunnel DNS: {err}")],
            );
            report.mark_failed();
            Err(err)
        }
    }
}

fn classify_linux_failure(error: &NetworkError) -> RouteApplyFailureKind {
    match error {
        NetworkError::MissingFwmark => RouteApplyFailureKind::Precondition,
        NetworkError::DnsVerifyFailed(_) => RouteApplyFailureKind::Verification,
        NetworkError::CommandFailed { .. }
        | NetworkError::DnsNotSupported
        | NetworkError::KillSwitchUnavailable(_) => RouteApplyFailureKind::System,
        NetworkError::Io(_) => RouteApplyFailureKind::Persistence,
        NetworkError::Netlink(_) | NetworkError::LinkNotFound(_) => RouteApplyFailureKind::Lookup,
    }
}
