use std::net::IpAddr;
use std::path::Path;

use super::super::super::NetworkError;
use super::super::command::run_cmd;
use crate::log::events::dns as log_dns;

pub(in crate::platform::linux::network::dns) async fn apply_resolved(
    resolvectl: &Path,
    tun_name: &str,
    servers: &[IpAddr],
    search: &[String],
) -> Result<(), NetworkError> {
    // systemd-resolved 的 per-link DNS：仅作用于指定接口。
    if !servers.is_empty() {
        let mut args = vec!["dns".to_string(), tun_name.to_string()];
        for server in servers {
            args.push(server.to_string());
        }
        run_cmd(resolvectl, &args).await?;
    }

    if !servers.is_empty() || !search.is_empty() {
        // 追加 ~. 代表全局路由域，确保默认走该接口解析。
        let mut domain_args = vec!["domain".to_string(), tun_name.to_string()];
        domain_args.extend(search.iter().cloned());
        if !servers.is_empty() && !search.iter().any(|domain| domain == "~.") {
            domain_args.push("~.".to_string());
        }
        run_cmd(resolvectl, &domain_args).await?;
    }

    Ok(())
}

pub(in crate::platform::linux::network::dns) async fn cleanup_resolved(
    resolvectl: &Path,
    tun_name: &str,
) -> Result<(), NetworkError> {
    // resolvectl revert 会撤销该接口的 DNS 配置。
    log_dns::resolvectl_revert(resolvectl);
    run_cmd(resolvectl, &["revert".to_string(), tun_name.to_string()]).await
}
