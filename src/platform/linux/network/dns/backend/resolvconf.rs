use std::net::IpAddr;
use std::path::Path;

use super::super::super::NetworkError;
use super::super::command::{run_cmd, run_cmd_with_input};
use crate::log::events::dns as log_dns;

pub(in crate::platform::linux::network::dns) async fn apply_resolvconf(
    resolvconf: &Path,
    tun_name: &str,
    servers: &[IpAddr],
    search: &[String],
) -> Result<(), NetworkError> {
    // resolvconf/openresolv 通过 stdin 写入临时接口条目。
    let mut content = String::new();
    for server in servers {
        content.push_str("nameserver ");
        content.push_str(&server.to_string());
        content.push('\n');
    }
    if !search.is_empty() {
        content.push_str("search ");
        content.push_str(&search.join(" "));
        content.push('\n');
    }

    let args = vec!["-a".to_string(), tun_name.to_string()];
    run_cmd_with_input(resolvconf, &args, &content).await
}

pub(in crate::platform::linux::network::dns) async fn cleanup_resolvconf(
    resolvconf: &Path,
    tun_name: &str,
) -> Result<(), NetworkError> {
    // resolvconf -d 删除该接口的 DNS 记录。
    log_dns::resolvconf_revert(resolvconf);
    run_cmd(resolvconf, &["-d".to_string(), tun_name.to_string()]).await
}
