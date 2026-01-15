//! Policy routing 逻辑（全隧道模式）。
//!
//! 设计思路：
//! - 使用 fwmark 将“引擎流量”与“业务流量”区分开。
//! - 业务流量走独立路由表（table_id），避免污染 main table。
//! - 通过 suppress main 规则实现“默认不走主表”的全隧道效果。

use std::fs;
use std::net::{Ipv4Addr, Ipv6Addr};

use futures_util::stream::TryStreamExt;
use netlink_packet_route::route::{RouteHeader, RouteMessage};
use netlink_packet_route::rule::{RuleAction, RuleAttribute, RuleFlags, RuleMessage};
use rtnetlink::{Handle, IpVersion, RouteMessageBuilder};

use super::logging::log_net;
use super::netlink::{route_message_oif, route_message_table_id};
use super::NetworkError;

// 规则优先级：越小优先级越高，保持稳定顺序，便于排错。
const RULE_PRIORITY_FWMARK: u32 = 10000;
const RULE_PRIORITY_TUNNEL: u32 = 10001;
const RULE_PRIORITY_SUPPRESS: u32 = 10002;

#[derive(Debug)]
pub(super) struct PolicyRoutingState {
    pub(super) table_id: u32,
    pub(super) fwmark: u32,
    pub(super) v4: bool,
    pub(super) v6: bool,
}

/// 写入 policy rules（v4/v6）。
///
/// 说明：
/// - fwmark -> main：保证与 endpoint 相关的流量仍能走主路由。
/// - not fwmark -> table：业务流量进入隧道路由表。
/// - suppress main：避免业务流量“回退”到主路由。
pub(super) async fn apply_policy_rules(
    handle: &Handle,
    fwmark: u32,
    table_id: u32,
    v4: bool,
    v6: bool,
) -> Result<(), NetworkError> {
    if v4 {
        apply_policy_rules_family(handle, false, fwmark, table_id).await?;
    }
    if v6 {
        apply_policy_rules_family(handle, true, fwmark, table_id).await?;
    }
    Ok(())
}

async fn apply_policy_rules_family(
    handle: &Handle,
    v6: bool,
    fwmark: u32,
    table_id: u32,
) -> Result<(), NetworkError> {
    let rule = handle.rule();
    let main_table = RouteHeader::RT_TABLE_MAIN as u32;

    if v6 {
        log_net(format!(
            "policy rule add: v6 fwmark=0x{fwmark:x} table=main pref={RULE_PRIORITY_FWMARK}"
        ));
        rule.add()
            .v6()
            .fw_mark(fwmark)
            .action(RuleAction::ToTable)
            .priority(RULE_PRIORITY_FWMARK)
            .table_id(main_table)
            .execute()
            .await?;

        log_net(format!(
            "policy rule add: v6 not fwmark=0x{fwmark:x} table={table_id} pref={RULE_PRIORITY_TUNNEL}"
        ));
        let mut tunnel_rule = rule
            .add()
            .v6()
            .fw_mark(fwmark)
            .action(RuleAction::ToTable)
            .priority(RULE_PRIORITY_TUNNEL)
            .table_id(table_id);
        // 反转匹配：not fwmark -> table
        tunnel_rule.message_mut().header.flags |= RuleFlags::Invert;
        tunnel_rule.execute().await?;

        log_net(format!(
            "policy rule add: v6 suppress main pref={RULE_PRIORITY_SUPPRESS}"
        ));
        let mut suppress_rule = rule
            .add()
            .v6()
            .action(RuleAction::ToTable)
            .priority(RULE_PRIORITY_SUPPRESS)
            .table_id(main_table);
        // suppress_prefixlen=0：抑制 main 表的默认路由。
        suppress_rule
            .message_mut()
            .attributes
            .push(RuleAttribute::SuppressPrefixLen(0));
        suppress_rule.execute().await?;
    } else {
        log_net(format!(
            "policy rule add: v4 fwmark=0x{fwmark:x} table=main pref={RULE_PRIORITY_FWMARK}"
        ));
        rule.add()
            .v4()
            .fw_mark(fwmark)
            .action(RuleAction::ToTable)
            .priority(RULE_PRIORITY_FWMARK)
            .table_id(main_table)
            .execute()
            .await?;

        log_net(format!(
            "policy rule add: v4 not fwmark=0x{fwmark:x} table={table_id} pref={RULE_PRIORITY_TUNNEL}"
        ));
        let mut tunnel_rule = rule
            .add()
            .v4()
            .fw_mark(fwmark)
            .action(RuleAction::ToTable)
            .priority(RULE_PRIORITY_TUNNEL)
            .table_id(table_id);
        // 反转匹配：not fwmark -> table
        tunnel_rule.message_mut().header.flags |= RuleFlags::Invert;
        tunnel_rule.execute().await?;

        log_net(format!(
            "policy rule add: v4 suppress main pref={RULE_PRIORITY_SUPPRESS}"
        ));
        let mut suppress_rule = rule
            .add()
            .v4()
            .action(RuleAction::ToTable)
            .priority(RULE_PRIORITY_SUPPRESS)
            .table_id(main_table);
        // suppress_prefixlen=0：抑制 main 表的默认路由。
        suppress_rule
            .message_mut()
            .attributes
            .push(RuleAttribute::SuppressPrefixLen(0));
        suppress_rule.execute().await?;
    }

    Ok(())
}

pub(super) async fn cleanup_policy_rules(
    handle: &Handle,
    v4: bool,
    v6: bool,
) -> Result<(), NetworkError> {
    // 仅清理由本模块写入的规则（根据优先级识别）。
    if v4 {
        cleanup_policy_rules_family(handle, IpVersion::V4).await?;
    }
    if v6 {
        cleanup_policy_rules_family(handle, IpVersion::V6).await?;
    }
    Ok(())
}

async fn cleanup_policy_rules_family(
    handle: &Handle,
    ip_version: IpVersion,
) -> Result<(), NetworkError> {
    let mut rules = handle.rule().get(ip_version).execute();
    while let Some(rule) = rules.try_next().await? {
        if is_policy_rule(&rule) {
            handle.rule().del(rule).execute().await?;
        }
    }
    Ok(())
}

fn is_policy_rule(rule: &RuleMessage) -> bool {
    // 通过优先级筛选，避免误删系统或其他组件的规则。
    match rule_priority(rule) {
        Some(priority)
            if priority == RULE_PRIORITY_FWMARK
                || priority == RULE_PRIORITY_TUNNEL
                || priority == RULE_PRIORITY_SUPPRESS =>
        {
            true
        }
        _ => false,
    }
}

fn rule_priority(rule: &RuleMessage) -> Option<u32> {
    rule.attributes.iter().find_map(|attr| match attr {
        RuleAttribute::Priority(value) => Some(*value),
        _ => None,
    })
}

pub(super) async fn cleanup_stale_default_routes(
    handle: &Handle,
    tun_name: &str,
    tun_index: u32,
) -> Result<(), NetworkError> {
    // 清理“历史遗留的 tun 默认路由”，避免新隧道启动时走错出口。
    cleanup_stale_default_routes_v4(handle, tun_name, tun_index).await?;
    cleanup_stale_default_routes_v6(handle, tun_name, tun_index).await?;
    Ok(())
}

async fn cleanup_stale_default_routes_v4(
    handle: &Handle,
    tun_name: &str,
    tun_index: u32,
) -> Result<(), NetworkError> {
    let filter = RouteMessageBuilder::<Ipv4Addr>::default().build();
    cleanup_stale_default_routes_family(handle, tun_name, tun_index, filter).await
}

async fn cleanup_stale_default_routes_v6(
    handle: &Handle,
    tun_name: &str,
    tun_index: u32,
) -> Result<(), NetworkError> {
    let filter = RouteMessageBuilder::<Ipv6Addr>::default().build();
    cleanup_stale_default_routes_family(handle, tun_name, tun_index, filter).await
}

async fn cleanup_stale_default_routes_family(
    handle: &Handle,
    tun_name: &str,
    tun_index: u32,
    filter: RouteMessage,
) -> Result<(), NetworkError> {
    let mut routes = handle.route().get(filter).execute();
    while let Some(message) = routes.try_next().await? {
        if message.header.destination_prefix_length != 0 {
            continue;
        }
        if route_message_table_id(&message) != RouteHeader::RT_TABLE_MAIN as u32 {
            continue;
        }
        let Some(oif) = route_message_oif(&message) else {
            continue;
        };
        if oif == tun_index {
            continue;
        }
        let Some(name) = interface_name_by_index(oif) else {
            continue;
        };
        if name == tun_name {
            continue;
        }
        if !is_tun_interface(&name) {
            continue;
        }
        log_net(format!("stale default route del: iface={name}"));
        handle.route().del(message).execute().await?;
    }
    Ok(())
}

fn interface_name_by_index(index: u32) -> Option<String> {
    // 通过 /sys/class/net 映射 ifindex -> 接口名。
    let entries = fs::read_dir("/sys/class/net").ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let path = format!("/sys/class/net/{name}/ifindex");
        let value = fs::read_to_string(path).ok()?;
        if value.trim().parse::<u32>().ok()? == index {
            return Some(name);
        }
    }
    None
}

fn is_tun_interface(name: &str) -> bool {
    // /sys/class/net/<if>/type == 65534 表示 TUN/TAP。
    let path = format!("/sys/class/net/{name}/type");
    let value = fs::read_to_string(path).ok();
    let Some(value) = value else {
        return false;
    };
    value.trim().parse::<u32>().ok() == Some(65534)
}
