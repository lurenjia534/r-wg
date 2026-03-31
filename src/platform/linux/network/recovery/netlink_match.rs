use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use netlink_packet_route::route::{RouteAddress, RouteAttribute, RouteHeader, RouteMessage};
use netlink_packet_route::rule::{RuleAction, RuleAttribute, RuleFlags, RuleMessage};
use netlink_packet_route::AddressFamily;
use rtnetlink::RouteMessageBuilder;

use super::{RecoveryPolicySnapshot, RecoveryRouteSnapshot};
use crate::core::config::AllowedIp;

const RULE_PRIORITY_FWMARK: u32 = 10000;
const RULE_PRIORITY_TUNNEL: u32 = 10001;
const RULE_PRIORITY_SUPPRESS: u32 = 10002;

pub(crate) fn build_route_message_without_oif(
    route: &AllowedIp,
    table_id: Option<u32>,
) -> RouteMessage {
    match route.addr {
        IpAddr::V4(addr) => {
            let mut request =
                RouteMessageBuilder::<Ipv4Addr>::default().destination_prefix(addr, route.cidr);
            if let Some(table_id) = table_id {
                request = request.table_id(table_id);
            }
            request.build()
        }
        IpAddr::V6(addr) => {
            let mut request =
                RouteMessageBuilder::<Ipv6Addr>::default().destination_prefix(addr, route.cidr);
            if let Some(table_id) = table_id {
                request = request.table_id(table_id);
            }
            request.build()
        }
    }
}

pub(crate) fn route_message_matches_snapshot(
    message: &RouteMessage,
    snapshot: &RecoveryRouteSnapshot,
) -> bool {
    if message.header.destination_prefix_length != snapshot.cidr {
        return false;
    }

    if route_message_table_id(message)
        != snapshot
            .table_id
            .unwrap_or(RouteHeader::RT_TABLE_MAIN as u32)
    {
        return false;
    }

    match (snapshot.addr, route_message_destination(message)) {
        (IpAddr::V4(addr), Some(IpAddr::V4(dst))) => addr == dst,
        (IpAddr::V6(addr), Some(IpAddr::V6(dst))) => addr == dst,
        _ => false,
    }
}

pub(crate) fn policy_rule_messages(snapshot: &RecoveryPolicySnapshot) -> Vec<RuleMessage> {
    let mut messages = Vec::new();
    let main_table = RouteHeader::RT_TABLE_MAIN;
    if snapshot.v4 {
        push_policy_rule_messages(
            &mut messages,
            AddressFamily::Inet,
            snapshot.fwmark,
            snapshot.table_id,
            main_table,
        );
    }
    if snapshot.v6 {
        push_policy_rule_messages(
            &mut messages,
            AddressFamily::Inet6,
            snapshot.fwmark,
            snapshot.table_id,
            main_table,
        );
    }
    messages
}

fn route_message_table_id(message: &RouteMessage) -> u32 {
    for attr in &message.attributes {
        if let RouteAttribute::Table(value) = attr {
            return *value;
        }
    }
    message.header.table as u32
}

fn route_message_destination(message: &RouteMessage) -> Option<IpAddr> {
    message.attributes.iter().find_map(|attr| match attr {
        RouteAttribute::Destination(RouteAddress::Inet(addr)) => Some(IpAddr::V4(*addr)),
        RouteAttribute::Destination(RouteAddress::Inet6(addr)) => Some(IpAddr::V6(*addr)),
        _ => None,
    })
}

fn push_policy_rule_messages(
    messages: &mut Vec<RuleMessage>,
    family: AddressFamily,
    fwmark: u32,
    table_id: u32,
    main_table: u8,
) {
    let mut fwmark_rule = RuleMessage::default();
    fwmark_rule.header.family = family;
    fwmark_rule.header.action = RuleAction::ToTable;
    fwmark_rule.header.table = main_table;
    fwmark_rule
        .attributes
        .push(RuleAttribute::Priority(RULE_PRIORITY_FWMARK));
    fwmark_rule.attributes.push(RuleAttribute::FwMark(fwmark));
    messages.push(fwmark_rule);

    let mut tunnel_rule = RuleMessage::default();
    tunnel_rule.header.family = family;
    tunnel_rule.header.action = RuleAction::ToTable;
    if table_id > 255 {
        tunnel_rule.attributes.push(RuleAttribute::Table(table_id));
        tunnel_rule.header.table = main_table;
    } else {
        tunnel_rule.header.table = table_id as u8;
    }
    tunnel_rule
        .attributes
        .push(RuleAttribute::Priority(RULE_PRIORITY_TUNNEL));
    tunnel_rule.attributes.push(RuleAttribute::FwMark(fwmark));
    tunnel_rule.header.flags |= RuleFlags::Invert;
    messages.push(tunnel_rule);

    let mut suppress_rule = RuleMessage::default();
    suppress_rule.header.family = family;
    suppress_rule.header.action = RuleAction::ToTable;
    suppress_rule.header.table = main_table;
    suppress_rule
        .attributes
        .push(RuleAttribute::Priority(RULE_PRIORITY_SUPPRESS));
    suppress_rule
        .attributes
        .push(RuleAttribute::SuppressPrefixLen(0));
    messages.push(suppress_rule);
}
