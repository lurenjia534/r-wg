//! Policy routing 逻辑（全隧道模式）。
//!
//! 设计思路：
//! - 使用 fwmark 将“引擎流量”与“业务流量”区分开。
//! - 业务流量走独立路由表（table_id），避免污染 main table。
//! - 通过 suppress main 规则实现“默认不走主表”的全隧道效果。

use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::sync::atomic::{AtomicU8, Ordering};

use futures_util::stream::TryStreamExt;
use netlink_packet_route::link::{InfoKind, LinkAttribute, LinkInfo, LinkLayerType, LinkMessage};
use netlink_packet_route::route::{RouteHeader, RouteMessage};
use netlink_packet_route::rule::{RuleAction, RuleAttribute, RuleFlags, RuleMessage};
use netlink_packet_route::AddressFamily;
use rtnetlink::{Handle, IpVersion, RouteMessageBuilder};

use super::netlink::{route_message_oif, route_message_table_id};
use super::NetworkError;
use crate::log::events::net as log_net;

// 规则优先级：越小优先级越高，保持稳定顺序，便于排错。
const RULE_PRIORITY_FWMARK: u32 = 10000;
const RULE_PRIORITY_TUNNEL: u32 = 10001;
const RULE_PRIORITY_SUPPRESS: u32 = 10002;

const CLEANUP_STATE_IDLE: u8 = 0;
const CLEANUP_STATE_IN_PROGRESS: u8 = 1;
const CLEANUP_STATE_DONE: u8 = 2;

static CLEANUP_POLICY_RULES_STATE: AtomicU8 = AtomicU8::new(CLEANUP_STATE_IDLE);
static CLEANUP_STALE_DEFAULT_ROUTES_STATE: AtomicU8 = AtomicU8::new(CLEANUP_STATE_IDLE);

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
    let family = if v6 {
        AddressFamily::Inet6
    } else {
        AddressFamily::Inet
    };
    let main_table = RouteHeader::RT_TABLE_MAIN as u32;

    log_policy_rule_add_fwmark(v6, fwmark, main_table);
    execute_rule_add(handle, v6, fwmark_rule_message(family, fwmark, main_table)).await?;

    log_policy_rule_add_tunnel(v6, fwmark, table_id);
    execute_rule_add(handle, v6, tunnel_rule_message(family, fwmark, table_id)).await?;

    log_policy_rule_add_suppress(v6);
    execute_rule_add(handle, v6, suppress_rule_message(family, main_table)).await?;

    Ok(())
}

pub(super) async fn cleanup_policy_rules(
    handle: &Handle,
    policy: Option<&PolicyRoutingState>,
    v4: bool,
    v6: bool,
) -> Result<(), NetworkError> {
    // 已知当前策略状态时走精确匹配；否则退回到保守的结构匹配。
    if v4 {
        cleanup_policy_rules_family(handle, IpVersion::V4, policy).await?;
    }
    if v6 {
        cleanup_policy_rules_family(handle, IpVersion::V6, policy).await?;
    }
    Ok(())
}

pub(super) async fn cleanup_policy_rules_once(
    handle: &Handle,
    policy: Option<&PolicyRoutingState>,
) -> Result<(), NetworkError> {
    if !begin_cleanup_once(&CLEANUP_POLICY_RULES_STATE) {
        return Ok(());
    }
    let result = cleanup_policy_rules(handle, policy, true, true).await;
    finish_cleanup_once(&CLEANUP_POLICY_RULES_STATE, result.is_ok());
    result
}

async fn cleanup_policy_rules_family(
    handle: &Handle,
    ip_version: IpVersion,
    policy: Option<&PolicyRoutingState>,
) -> Result<(), NetworkError> {
    let mut rules = handle.rule().get(ip_version).execute();
    while let Some(rule) = rules.try_next().await? {
        if is_policy_rule(&rule, policy) {
            handle.rule().del(rule).execute().await?;
        }
    }
    Ok(())
}

fn is_policy_rule(rule: &RuleMessage, policy: Option<&PolicyRoutingState>) -> bool {
    match policy {
        Some(policy) => is_exact_policy_rule(rule, policy),
        None => is_managed_policy_rule_signature(rule),
    }
}

fn rule_priority(rule: &RuleMessage) -> Option<u32> {
    rule.attributes.iter().find_map(|attr| match attr {
        RuleAttribute::Priority(value) => Some(*value),
        _ => None,
    })
}

fn rule_table_id(rule: &RuleMessage) -> u32 {
    rule.attributes
        .iter()
        .find_map(|attr| match attr {
            RuleAttribute::Table(value) => Some(*value),
            _ => None,
        })
        .unwrap_or(rule.header.table as u32)
}

fn rule_fwmark(rule: &RuleMessage) -> Option<u32> {
    rule.attributes.iter().find_map(|attr| match attr {
        RuleAttribute::FwMark(value) => Some(*value),
        _ => None,
    })
}

fn rule_suppress_prefix_len(rule: &RuleMessage) -> Option<u32> {
    rule.attributes.iter().find_map(|attr| match attr {
        RuleAttribute::SuppressPrefixLen(value) => Some(*value),
        _ => None,
    })
}

fn rule_has_invert(rule: &RuleMessage) -> bool {
    rule.header.flags.contains(RuleFlags::Invert)
}

fn is_exact_policy_rule(rule: &RuleMessage, policy: &PolicyRoutingState) -> bool {
    let main_table = RouteHeader::RT_TABLE_MAIN as u32;
    match rule_priority(rule) {
        Some(RULE_PRIORITY_FWMARK) => {
            rule.header.action == RuleAction::ToTable
                && rule_table_id(rule) == main_table
                && rule_fwmark(rule) == Some(policy.fwmark)
                && !rule_has_invert(rule)
                && rule_suppress_prefix_len(rule).is_none()
        }
        Some(RULE_PRIORITY_TUNNEL) => {
            rule.header.action == RuleAction::ToTable
                && rule_table_id(rule) == policy.table_id
                && rule_fwmark(rule) == Some(policy.fwmark)
                && rule_has_invert(rule)
                && rule_suppress_prefix_len(rule).is_none()
        }
        Some(RULE_PRIORITY_SUPPRESS) => {
            rule.header.action == RuleAction::ToTable
                && rule_table_id(rule) == main_table
                && rule_fwmark(rule).is_none()
                && !rule_has_invert(rule)
                && rule_suppress_prefix_len(rule) == Some(0)
        }
        _ => false,
    }
}

fn is_managed_policy_rule_signature(rule: &RuleMessage) -> bool {
    let main_table = RouteHeader::RT_TABLE_MAIN as u32;
    match rule_priority(rule) {
        Some(RULE_PRIORITY_FWMARK) => {
            rule.header.action == RuleAction::ToTable
                && rule_table_id(rule) == main_table
                && rule_fwmark(rule).is_some()
                && !rule_has_invert(rule)
                && rule_suppress_prefix_len(rule).is_none()
        }
        Some(RULE_PRIORITY_TUNNEL) => {
            rule.header.action == RuleAction::ToTable
                && rule_fwmark(rule).is_some()
                && rule_has_invert(rule)
                && rule_suppress_prefix_len(rule).is_none()
        }
        Some(RULE_PRIORITY_SUPPRESS) => {
            rule.header.action == RuleAction::ToTable
                && rule_table_id(rule) == main_table
                && rule_fwmark(rule).is_none()
                && !rule_has_invert(rule)
                && rule_suppress_prefix_len(rule) == Some(0)
        }
        _ => false,
    }
}

pub(super) async fn cleanup_stale_default_routes(
    handle: &Handle,
    tun_name: &str,
    tun_index: u32,
) -> Result<(), NetworkError> {
    let link_metadata = link_metadata_by_index(handle).await?;
    // 清理“历史遗留的 tun 默认路由”，避免新隧道启动时走错出口。
    cleanup_stale_default_routes_v4(handle, tun_name, tun_index, &link_metadata).await?;
    cleanup_stale_default_routes_v6(handle, tun_name, tun_index, &link_metadata).await?;
    Ok(())
}

pub(super) async fn cleanup_stale_default_routes_once(
    handle: &Handle,
    tun_name: &str,
    tun_index: u32,
) -> Result<(), NetworkError> {
    if !begin_cleanup_once(&CLEANUP_STALE_DEFAULT_ROUTES_STATE) {
        return Ok(());
    }
    let result = cleanup_stale_default_routes(handle, tun_name, tun_index).await;
    finish_cleanup_once(&CLEANUP_STALE_DEFAULT_ROUTES_STATE, result.is_ok());
    result
}

async fn cleanup_stale_default_routes_v4(
    handle: &Handle,
    tun_name: &str,
    tun_index: u32,
    link_metadata: &HashMap<u32, LinkMetadata>,
) -> Result<(), NetworkError> {
    let filter = RouteMessageBuilder::<Ipv4Addr>::default().build();
    cleanup_stale_default_routes_family(handle, tun_name, tun_index, link_metadata, filter).await
}

async fn cleanup_stale_default_routes_v6(
    handle: &Handle,
    tun_name: &str,
    tun_index: u32,
    link_metadata: &HashMap<u32, LinkMetadata>,
) -> Result<(), NetworkError> {
    let filter = RouteMessageBuilder::<Ipv6Addr>::default().build();
    cleanup_stale_default_routes_family(handle, tun_name, tun_index, link_metadata, filter).await
}

async fn cleanup_stale_default_routes_family(
    handle: &Handle,
    tun_name: &str,
    tun_index: u32,
    link_metadata: &HashMap<u32, LinkMetadata>,
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
        let Some(link) = link_metadata.get(&oif) else {
            continue;
        };
        if link.name == tun_name {
            continue;
        }
        if !link.is_tun {
            continue;
        }
        log_net::stale_default_route_del(&link.name);
        handle.route().del(message).execute().await?;
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct LinkMetadata {
    name: String,
    is_tun: bool,
}

async fn link_metadata_by_index(
    handle: &Handle,
) -> Result<HashMap<u32, LinkMetadata>, NetworkError> {
    let mut links = handle.link().get().execute();
    let mut metadata = HashMap::new();
    while let Some(link) = links.try_next().await? {
        let Some(name) = link_name(&link) else {
            continue;
        };
        metadata.insert(
            link.header.index,
            LinkMetadata {
                name,
                is_tun: is_tun_link(&link),
            },
        );
    }
    Ok(metadata)
}

fn link_name(link: &LinkMessage) -> Option<String> {
    link.attributes.iter().find_map(|attr| match attr {
        LinkAttribute::IfName(name) => Some(name.clone()),
        _ => None,
    })
}

fn is_tun_link(link: &LinkMessage) -> bool {
    link.header.link_layer_type == LinkLayerType::None
        || link_info_kind(link) == Some(InfoKind::Tun)
}

fn link_info_kind(link: &LinkMessage) -> Option<InfoKind> {
    link.attributes.iter().find_map(|attr| match attr {
        LinkAttribute::LinkInfo(infos) => infos.iter().find_map(|info| match info {
            LinkInfo::Kind(kind) => Some(kind.clone()),
            _ => None,
        }),
        _ => None,
    })
}

fn fwmark_rule_message(family: AddressFamily, fwmark: u32, table_id: u32) -> RuleMessage {
    let mut rule = base_policy_rule_message(family, RULE_PRIORITY_FWMARK, table_id);
    rule.attributes.push(RuleAttribute::FwMark(fwmark));
    rule
}

fn tunnel_rule_message(family: AddressFamily, fwmark: u32, table_id: u32) -> RuleMessage {
    let mut rule = base_policy_rule_message(family, RULE_PRIORITY_TUNNEL, table_id);
    rule.attributes.push(RuleAttribute::FwMark(fwmark));
    rule.header.flags |= RuleFlags::Invert;
    rule
}

fn suppress_rule_message(family: AddressFamily, table_id: u32) -> RuleMessage {
    let mut rule = base_policy_rule_message(family, RULE_PRIORITY_SUPPRESS, table_id);
    rule.attributes.push(RuleAttribute::SuppressPrefixLen(0));
    rule
}

fn base_policy_rule_message(family: AddressFamily, priority: u32, table_id: u32) -> RuleMessage {
    let mut rule = RuleMessage::default();
    rule.header.family = family;
    rule.header.action = RuleAction::ToTable;
    set_rule_table_id(&mut rule, table_id);
    rule.attributes.push(RuleAttribute::Priority(priority));
    rule
}

fn set_rule_table_id(rule: &mut RuleMessage, table_id: u32) {
    if table_id > u8::MAX as u32 {
        rule.attributes.push(RuleAttribute::Table(table_id));
        rule.header.table = RouteHeader::RT_TABLE_MAIN;
    } else {
        rule.header.table = table_id as u8;
    }
}

async fn execute_rule_add(
    handle: &Handle,
    v6: bool,
    message: RuleMessage,
) -> Result<(), NetworkError> {
    if v6 {
        let mut request = handle.rule().add().v6();
        *request.message_mut() = message;
        request.execute().await?;
    } else {
        let mut request = handle.rule().add().v4();
        *request.message_mut() = message;
        request.execute().await?;
    }
    Ok(())
}

fn log_policy_rule_add_fwmark(v6: bool, fwmark: u32, main_table: u32) {
    if v6 {
        log_net::policy_rule_add_v6_fwmark(fwmark, main_table, RULE_PRIORITY_FWMARK);
    } else {
        log_net::policy_rule_add_v4_fwmark(fwmark, main_table, RULE_PRIORITY_FWMARK);
    }
}

fn log_policy_rule_add_tunnel(v6: bool, fwmark: u32, table_id: u32) {
    if v6 {
        log_net::policy_rule_add_v6_not_fwmark(fwmark, table_id, RULE_PRIORITY_TUNNEL);
    } else {
        log_net::policy_rule_add_v4_not_fwmark(fwmark, table_id, RULE_PRIORITY_TUNNEL);
    }
}

fn log_policy_rule_add_suppress(v6: bool) {
    if v6 {
        log_net::policy_rule_add_v6_suppress(RULE_PRIORITY_SUPPRESS);
    } else {
        log_net::policy_rule_add_v4_suppress(RULE_PRIORITY_SUPPRESS);
    }
}

fn begin_cleanup_once(state: &AtomicU8) -> bool {
    state
        .compare_exchange(
            CLEANUP_STATE_IDLE,
            CLEANUP_STATE_IN_PROGRESS,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_ok()
}

fn finish_cleanup_once(state: &AtomicU8, success: bool) {
    state.store(
        if success {
            CLEANUP_STATE_DONE
        } else {
            CLEANUP_STATE_IDLE
        },
        Ordering::Release,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use netlink_packet_route::link::LinkLayerType;
    use netlink_packet_route::AddressFamily;
    use std::sync::atomic::AtomicU8;

    fn make_rule(
        family: AddressFamily,
        priority: u32,
        table_id: u32,
        fwmark: Option<u32>,
        invert: bool,
        suppress_prefix_len: Option<u32>,
    ) -> RuleMessage {
        let mut rule = RuleMessage::default();
        rule.header.family = family;
        rule.header.action = RuleAction::ToTable;
        rule.header.table = table_id as u8;
        if table_id > u8::MAX as u32 {
            rule.attributes.push(RuleAttribute::Table(table_id));
        }
        rule.attributes.push(RuleAttribute::Priority(priority));
        if let Some(fwmark) = fwmark {
            rule.attributes.push(RuleAttribute::FwMark(fwmark));
        }
        if let Some(suppress_prefix_len) = suppress_prefix_len {
            rule.attributes
                .push(RuleAttribute::SuppressPrefixLen(suppress_prefix_len));
        }
        if invert {
            rule.header.flags |= RuleFlags::Invert;
        }
        rule
    }

    #[test]
    fn exact_policy_match_rejects_foreign_fwmark() {
        let policy = PolicyRoutingState {
            table_id: 51820,
            fwmark: 0x1234,
            v4: true,
            v6: true,
        };
        let own_rule = make_rule(
            AddressFamily::Inet,
            RULE_PRIORITY_TUNNEL,
            policy.table_id,
            Some(policy.fwmark),
            true,
            None,
        );
        let foreign_rule = make_rule(
            AddressFamily::Inet,
            RULE_PRIORITY_TUNNEL,
            policy.table_id,
            Some(0x9999),
            true,
            None,
        );

        assert!(is_exact_policy_rule(&own_rule, &policy));
        assert!(!is_exact_policy_rule(&foreign_rule, &policy));
    }

    #[test]
    fn heuristic_match_requires_expected_rule_shape() {
        let mut malformed = RuleMessage::default();
        malformed.header.family = AddressFamily::Inet;
        malformed
            .attributes
            .push(RuleAttribute::Priority(RULE_PRIORITY_FWMARK));

        let candidate = make_rule(
            AddressFamily::Inet,
            RULE_PRIORITY_FWMARK,
            RouteHeader::RT_TABLE_MAIN as u32,
            Some(0x1234),
            false,
            None,
        );

        assert!(!is_managed_policy_rule_signature(&malformed));
        assert!(is_managed_policy_rule_signature(&candidate));
    }

    #[test]
    fn rule_table_id_prefers_explicit_attribute() {
        let rule = make_rule(
            AddressFamily::Inet6,
            RULE_PRIORITY_TUNNEL,
            51820,
            Some(0x1234),
            true,
            None,
        );

        assert_eq!(rule_table_id(&rule), 51820);
    }

    fn make_link(
        name: Option<&str>,
        link_layer_type: LinkLayerType,
        info_kind: Option<InfoKind>,
    ) -> LinkMessage {
        let mut link = LinkMessage::default();
        link.header.link_layer_type = link_layer_type;
        if let Some(name) = name {
            link.attributes
                .push(LinkAttribute::IfName(name.to_string()));
        }
        if let Some(info_kind) = info_kind {
            link.attributes
                .push(LinkAttribute::LinkInfo(vec![LinkInfo::Kind(info_kind)]));
        }
        link
    }

    #[test]
    fn cleanup_once_state_allows_retry_after_failure() {
        let state = AtomicU8::new(CLEANUP_STATE_IDLE);

        assert!(begin_cleanup_once(&state));
        finish_cleanup_once(&state, false);
        assert!(begin_cleanup_once(&state));
        finish_cleanup_once(&state, true);
        assert!(!begin_cleanup_once(&state));
    }

    #[test]
    fn tun_detection_accepts_none_link_layer_or_tun_kind() {
        let header_none = make_link(Some("tun0"), LinkLayerType::None, None);
        let info_tun = make_link(Some("tun1"), LinkLayerType::Ether, Some(InfoKind::Tun));
        let ethernet = make_link(Some("eth0"), LinkLayerType::Ether, None);

        assert_eq!(link_name(&header_none).as_deref(), Some("tun0"));
        assert!(is_tun_link(&header_none));
        assert!(is_tun_link(&info_tun));
        assert!(!is_tun_link(&ethernet));
    }

    #[test]
    fn rule_builders_preserve_family_and_large_table_ids() {
        let family = AddressFamily::Inet6;
        let fwmark = 0x1234;
        let table_id = 51820;
        let main_table = RouteHeader::RT_TABLE_MAIN as u32;

        let fwmark_rule = fwmark_rule_message(family, fwmark, main_table);
        let tunnel_rule = tunnel_rule_message(family, fwmark, table_id);
        let suppress_rule = suppress_rule_message(family, main_table);

        assert_eq!(fwmark_rule.header.family, family);
        assert_eq!(tunnel_rule.header.family, family);
        assert_eq!(suppress_rule.header.family, family);
        assert_eq!(rule_table_id(&tunnel_rule), table_id);
        assert_eq!(rule_fwmark(&fwmark_rule), Some(fwmark));
        assert_eq!(rule_fwmark(&tunnel_rule), Some(fwmark));
        assert!(rule_has_invert(&tunnel_rule));
        assert_eq!(rule_suppress_prefix_len(&suppress_rule), Some(0));
    }
}
