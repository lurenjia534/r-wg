//! Linux 网络配置入口模块。
//!
//! 负责地址/路由/DNS/policy routing 的系统配置，
//! WireGuard 设备本身由 gotatun 创建与维护。

mod dns;
mod logging;
mod netlink;
mod policy;
mod routes;

use rtnetlink::{LinkMessageBuilder, LinkUnspec};

use crate::backend::wg::config::{
    AllowedIp, InterfaceAddress, InterfaceConfig, PeerConfig, RouteTable,
};

use dns::{apply_dns, cleanup_dns, DnsState};
use logging::{log_default_routes, log_privileges};
use netlink::{build_route_message, delete_address, delete_route, link_index, netlink_handle};
use policy::{
    apply_policy_rules, cleanup_policy_rules_for_state, cleanup_policy_rules_once,
    cleanup_stale_default_routes_once, PolicyRoutingState,
};
use routes::{collect_allowed_routes, detect_full_tunnel, route_table_for};

use crate::log::events::{dns as log_dns, net as log_net};

// 默认 policy routing 表号：不干扰 main table，且便于排查。
const DEFAULT_POLICY_TABLE: u32 = 200;

#[derive(Debug)]
pub struct AppliedNetworkState {
    tun_name: String,
    addresses: Vec<InterfaceAddress>,
    routes: Vec<AllowedIp>,
    table: Option<RouteTable>,
    dns: Option<DnsState>,
    policy: Option<PolicyRoutingState>,
}

#[derive(Debug)]
pub enum NetworkError {
    Io(std::io::Error),
    Netlink(rtnetlink::Error),
    CommandFailed {
        command: String,
        status: Option<i32>,
        stderr: String,
    },
    DnsVerifyFailed(String),
    DnsNotSupported,
    LinkNotFound(String),
    MissingFwmark,
}

impl std::fmt::Display for NetworkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetworkError::Io(err) => write!(f, "io error: {err}"),
            NetworkError::CommandFailed {
                command,
                status,
                stderr,
            } => write!(f, "command failed: {command} (status={status:?}) {stderr}"),
            NetworkError::DnsVerifyFailed(message) => {
                write!(f, "dns verification failed: {message}")
            }
            NetworkError::DnsNotSupported => write!(f, "no supported DNS backend found"),
            NetworkError::Netlink(err) => write!(f, "netlink error: {err}"),
            NetworkError::LinkNotFound(name) => write!(f, "link not found: {name}"),
            NetworkError::MissingFwmark => write!(f, "missing fwmark for policy routing"),
        }
    }
}

impl std::error::Error for NetworkError {}

impl From<std::io::Error> for NetworkError {
    fn from(err: std::io::Error) -> Self {
        NetworkError::Io(err)
    }
}

impl From<rtnetlink::Error> for NetworkError {
    fn from(err: rtnetlink::Error) -> Self {
        NetworkError::Netlink(err)
    }
}

/// 应用 Linux 网络配置。
///
/// 只负责系统地址/路由/DNS，WireGuard 隧道本身由 gotatun 负责。
pub async fn apply_network_config(
    tun_name: &str,
    interface: &InterfaceConfig,
    peers: &[PeerConfig],
) -> Result<AppliedNetworkState, NetworkError> {
    log_net::apply_linux(
        tun_name,
        interface.mtu,
        interface.addresses.len(),
        interface.table,
        interface.dns_servers.len(),
        interface.dns_search.len(),
    );
    log_privileges();

    // 建立 netlink 连接并查询接口索引，后续所有操作都基于 ifindex。
    let netlink = netlink_handle()?;
    let handle = netlink.handle();

    let result = async {
        let link_index = link_index(handle, tun_name).await?;
        log_net::link_index(link_index);
        // 清理历史遗留的 TUN 默认路由，避免新隧道误复用旧出口。
        if let Err(err) = cleanup_stale_default_routes_once(handle, tun_name, link_index).await {
            log_net::stale_default_route_cleanup_failed(&err);
        }

        // 设置 MTU 与 up 状态，确保隧道可用。
        if let Some(mtu) = interface.mtu {
            let message = LinkMessageBuilder::<LinkUnspec>::default()
                .index(link_index)
                .mtu(mtu.into())
                .build();
            handle.link().set(message).execute().await?;
        }

        let message = LinkMessageBuilder::<LinkUnspec>::default()
            .index(link_index)
            .up()
            .build();
        handle.link().set(message).execute().await?;

        // 写入接口地址（IPv4/IPv6）。
        for address in &interface.addresses {
            log_net::address_add(address.addr, address.cidr);
            handle
                .address()
                .add(link_index, address.addr, address.cidr)
                .execute()
                .await?;
        }

        // 根据 AllowedIPs 生成路由列表，并判断是否全隧道。
        let routes = collect_allowed_routes(peers);
        let (full_v4, full_v6) = detect_full_tunnel(&routes);
        // 全隧道场景下启用 policy routing，避免污染 main table。
        let policy = if interface.table != Some(RouteTable::Off) && (full_v4 || full_v6) {
            let table_id = match interface.table {
                Some(RouteTable::Id(value)) => value,
                _ => DEFAULT_POLICY_TABLE,
            };
            let fwmark = interface.fwmark.ok_or(NetworkError::MissingFwmark)?;
            if let Err(err) = cleanup_policy_rules_once(handle).await {
                log_net::stale_policy_rule_cleanup_failed(&err);
            }
            apply_policy_rules(handle, fwmark, table_id, full_v4, full_v6).await?;
            Some(PolicyRoutingState {
                table_id,
                fwmark,
                v4: full_v4,
                v6: full_v6,
            })
        } else {
            None
        };

        // 根据策略路由决策把路由写入 main 或自定义表。
        if interface.table != Some(RouteTable::Off) {
            for route in &routes {
                let table =
                    route_table_for(route, interface.table, policy.as_ref(), full_v4, full_v6);
                log_net::route_add(route.addr, route.cidr, table);
                let message = build_route_message(link_index, route, table);
                handle.route().add(message).execute().await?;
            }
        }

        // 输出当前默认路由，便于诊断主表/策略表走向。
        log_default_routes();

        let mut state = AppliedNetworkState {
            tun_name: tun_name.to_string(),
            addresses: interface.addresses.clone(),
            routes,
            table: interface.table,
            policy,
            dns: None,
        };

        // DNS 失败视为致命错误，避免全隧道场景出现 DNS 泄漏。
        if !interface.dns_servers.is_empty() || !interface.dns_search.is_empty() {
            log_dns::apply_summary(interface.dns_servers.len(), interface.dns_search.len());
            match apply_dns(tun_name, &interface.dns_servers, &interface.dns_search).await {
                Ok(dns_state) => {
                    state.dns = Some(dns_state);
                }
                Err(err) => {
                    log_dns::apply_failed(&err);
                    let _ = cleanup_network_config(state).await;
                    return Err(err);
                }
            }
        }

        Ok(state)
    }
    .await;

    netlink.shutdown().await;
    result
}

/// 清理之前应用的网络配置。
pub async fn cleanup_network_config(state: AppliedNetworkState) -> Result<(), NetworkError> {
    log_net::cleanup_linux(
        &state.tun_name,
        state.addresses.len(),
        state.routes.len(),
        state.table,
        state.dns.is_some(),
    );
    // 清理阶段同样需要 netlink handle。
    let netlink = netlink_handle()?;
    let handle = netlink.handle();

    let result = async {
        let link_index = match link_index(handle, &state.tun_name).await {
            Ok(index) => index,
            Err(err) => {
                log_net::link_lookup_failed(&err);
                return Ok(());
            }
        };

        // 先删除接口地址，避免残留地址影响后续路由决策。
        for address in &state.addresses {
            log_net::address_del(address.addr, address.cidr);
            if let Err(err) = delete_address(handle, link_index, address).await {
                log_net::address_del_failed(&err);
            }
        }

        // 删除该隧道添加的路由（包含策略表或主表路由）。
        if state.table != Some(RouteTable::Off) {
            let policy = state.policy.as_ref();
            let (full_v4, full_v6) = policy
                .map(|state| (state.v4, state.v6))
                .unwrap_or((false, false));
            for route in &state.routes {
                let table = route_table_for(route, state.table, policy, full_v4, full_v6);
                log_net::route_del(route.addr, route.cidr, table);
                if let Err(err) = delete_route(handle, link_index, route, table).await {
                    log_net::route_del_failed(&err);
                }
            }
        }

        // 按记录的 DNS 状态回滚。
        if let Some(dns) = state.dns {
            log_dns::revert_start();
            let _ = cleanup_dns(state.tun_name.as_str(), dns).await;
        }

        // 清理 policy rule，恢复系统默认路由策略。
        if let Some(policy) = state.policy {
            if let Err(err) = cleanup_policy_rules_for_state(handle, &policy).await {
                log_net::policy_rule_cleanup_failed(&err);
            }
        }

        Ok(())
    }
    .await;

    netlink.shutdown().await;
    result
}
