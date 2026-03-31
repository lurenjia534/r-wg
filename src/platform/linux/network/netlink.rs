//! Netlink 相关的低层封装。
//!
//! 目标：
//! - 统一 netlink 细节（handle 初始化、查询、删除）。
//! - 将匹配逻辑集中在一处，减少上层的协议细节耦合。

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use futures_util::stream::TryStreamExt;
use netlink_packet_route::route::{RouteAddress, RouteAttribute, RouteHeader, RouteMessage};
use rtnetlink::{new_connection, Handle, RouteMessageBuilder};
use tokio::task::JoinHandle;

use crate::core::config::{AllowedIp, InterfaceAddress};

use super::NetworkError;

pub(super) struct NetlinkConnection {
    handle: Handle,
    task: JoinHandle<()>,
}

impl NetlinkConnection {
    pub(super) fn handle(&self) -> &Handle {
        &self.handle
    }

    pub(super) async fn shutdown(self) {
        let NetlinkConnection { handle, task } = self;
        drop(handle);
        task.abort();
        let _ = task.await;
    }
}

pub(super) fn netlink_handle() -> Result<NetlinkConnection, NetworkError> {
    // netlink 连接需要在 tokio 中驻留，避免请求悬挂。
    let (connection, handle, _) = new_connection()?;
    let task = tokio::spawn(connection);
    Ok(NetlinkConnection { handle, task })
}

pub(super) async fn link_index(handle: &Handle, tun_name: &str) -> Result<u32, NetworkError> {
    // 通过接口名查询 ifindex，后续所有操作都以 ifindex 为准。
    let mut links = handle
        .link()
        .get()
        .match_name(tun_name.to_string())
        .execute();
    match links.try_next().await? {
        Some(link) => Ok(link.header.index),
        None => Err(NetworkError::LinkNotFound(tun_name.to_string())),
    }
}

pub(super) async fn delete_address(
    handle: &Handle,
    link_index: u32,
    address: &InterfaceAddress,
) -> Result<(), NetworkError> {
    // 精确匹配地址与前缀长度，避免误删同网段其他地址。
    let mut addresses = handle
        .address()
        .get()
        .set_link_index_filter(link_index)
        .set_address_filter(address.addr)
        .set_prefix_length_filter(address.cidr)
        .execute();
    if let Some(message) = addresses.try_next().await? {
        handle.address().del(message).execute().await?;
    }
    Ok(())
}

pub(super) async fn delete_route(
    handle: &Handle,
    link_index: u32,
    route: &AllowedIp,
    table: Option<u32>,
) -> Result<(), NetworkError> {
    let message = build_route_message(link_index, route, table);
    if handle.route().del(message.clone()).execute().await.is_ok() {
        return Ok(());
    }

    // 回退到全量遍历，确保删除到目标路由。
    let filter = match route.addr {
        IpAddr::V4(_) => RouteMessageBuilder::<Ipv4Addr>::default().build(),
        IpAddr::V6(_) => RouteMessageBuilder::<Ipv6Addr>::default().build(),
    };
    let mut routes = handle.route().get(filter).execute();
    while let Some(message) = routes.try_next().await? {
        if route_message_matches(&message, link_index, route, table) {
            handle.route().del(message).execute().await?;
            break;
        }
    }
    Ok(())
}

pub(super) fn build_route_message(
    link_index: u32,
    route: &AllowedIp,
    table: Option<u32>,
) -> RouteMessage {
    match route.addr {
        IpAddr::V4(addr) => {
            let mut request = RouteMessageBuilder::<Ipv4Addr>::default()
                .destination_prefix(addr, route.cidr)
                .output_interface(link_index);
            if let Some(table) = table {
                request = request.table_id(table);
            }
            request.build()
        }
        IpAddr::V6(addr) => {
            let mut request = RouteMessageBuilder::<Ipv6Addr>::default()
                .destination_prefix(addr, route.cidr)
                .output_interface(link_index);
            if let Some(table) = table {
                request = request.table_id(table);
            }
            request.build()
        }
    }
}

fn route_message_matches(
    message: &RouteMessage,
    link_index: u32,
    route: &AllowedIp,
    table: Option<u32>,
) -> bool {
    // 仅匹配目标前缀、输出接口与路由表三项，避免误删其它规则。
    if message.header.destination_prefix_length != route.cidr {
        return false;
    }

    if route_message_oif(message) != Some(link_index) {
        return false;
    }

    let expected_table = match table {
        Some(value) => value,
        None => RouteHeader::RT_TABLE_MAIN as u32,
    };
    if route_message_table_id(message) != expected_table {
        return false;
    }

    match (route.addr, route_message_destination(message)) {
        (IpAddr::V4(addr), Some(IpAddr::V4(dst))) => addr == dst,
        (IpAddr::V6(addr), Some(IpAddr::V6(dst))) => addr == dst,
        _ => false,
    }
}

pub(super) fn route_message_table_id(message: &RouteMessage) -> u32 {
    // Table 属性存在时优先使用，否则回退到 header.table。
    for attr in &message.attributes {
        if let RouteAttribute::Table(value) = attr {
            return *value;
        }
    }
    message.header.table as u32
}

pub(super) fn route_message_oif(message: &RouteMessage) -> Option<u32> {
    // 从 RouteAttribute 解析输出接口 ifindex。
    message.attributes.iter().find_map(|attr| {
        if let RouteAttribute::Oif(value) = attr {
            Some(*value)
        } else {
            None
        }
    })
}

fn route_message_destination(message: &RouteMessage) -> Option<IpAddr> {
    // 仅关心 Destination 字段，对 Source 等其它属性不做匹配。
    message.attributes.iter().find_map(|attr| match attr {
        RouteAttribute::Destination(RouteAddress::Inet(addr)) => Some(IpAddr::V4(*addr)),
        RouteAttribute::Destination(RouteAddress::Inet6(addr)) => Some(IpAddr::V6(*addr)),
        _ => None,
    })
}
