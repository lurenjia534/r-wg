//! Linux 崩溃恢复日志与启动期修复。
//!
//! 设计目标：
//! - 在修改系统网络状态前，把“足够回滚”的信息持久化到 root-owned journal；
//! - clean stop 成功后删除 journal，异常退出则保留，供下次 service 启动或 boot-time repair 使用；
//! - 启动期先做无状态清理（stale route / policy rule），再做有状态回滚（DNS）。
//!
//! 当前 journal 主要覆盖：
//! - TUN 名称；
//! - phase（applying / running）；
//! - route / policy snapshot（用于精确 cleanup）；
//! - DNS 回滚所需快照（NM / resolv.conf / 其它后端状态）；
//! - 最近一次 RouteApplyReport（用于失败/恢复诊断）。
use std::env;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::PathBuf;

use futures_util::stream::TryStreamExt;
use netlink_packet_route::route::{RouteAddress, RouteAttribute, RouteHeader, RouteMessage};
use netlink_packet_route::rule::{RuleAction, RuleAttribute, RuleFlags, RuleMessage};
use netlink_packet_route::AddressFamily;
use rtnetlink::{Handle, RouteMessageBuilder};
use serde::{Deserialize, Serialize};

use super::dns::{cleanup_dns, DnsState};
use super::netlink::{link_index, netlink_handle};
use super::policy::{
    cleanup_policy_rules_once, cleanup_stale_default_routes_once, PolicyRoutingState,
};
use super::NetworkError;
use crate::backend::wg::config::AllowedIp;
use crate::backend::wg::route_plan::{
    RouteApplyReport, RoutePlan, RoutePlanRouteKind, RoutePlanRouteOp,
};

const RECOVERY_JOURNAL_FILE: &str = "recovery.json";
const LAST_APPLY_REPORT_FILE: &str = "last-apply-report.json";
const RULE_PRIORITY_FWMARK: u32 = 10000;
const RULE_PRIORITY_TUNNEL: u32 = 10001;
const RULE_PRIORITY_SUPPRESS: u32 = 10002;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum RecoveryPhase {
    Applying,
    Running,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RecoveryRouteSnapshot {
    pub(super) addr: IpAddr,
    pub(super) cidr: u8,
    pub(super) table_id: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RecoveryPolicySnapshot {
    pub(super) table_id: u32,
    pub(super) fwmark: u32,
    pub(super) v4: bool,
    pub(super) v6: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RecoveryJournal {
    tun_name: String,
    phase: RecoveryPhase,
    routes: Vec<RecoveryRouteSnapshot>,
    policy: Option<RecoveryPolicySnapshot>,
    dns: Option<DnsState>,
    report: Option<RouteApplyReport>,
}

pub(super) fn write_applying_journal(
    tun_name: &str,
    route_plan: &RoutePlan,
    policy: Option<&PolicyRoutingState>,
    report: &RouteApplyReport,
) -> Result<(), NetworkError> {
    write_persisted_apply_report(report)?;
    write_recovery_journal(&RecoveryJournal {
        tun_name: tun_name.to_string(),
        phase: RecoveryPhase::Applying,
        routes: route_snapshots(route_plan),
        policy: policy.map(policy_snapshot),
        dns: None,
        report: Some(report.clone()),
    })
}

pub(super) fn write_running_journal(
    tun_name: &str,
    routes: &[RoutePlanRouteOp],
    policy: Option<&PolicyRoutingState>,
    dns: Option<&DnsState>,
    report: &RouteApplyReport,
) -> Result<(), NetworkError> {
    write_persisted_apply_report(report)?;
    write_recovery_journal(&RecoveryJournal {
        tun_name: tun_name.to_string(),
        phase: RecoveryPhase::Running,
        routes: route_snapshots_from_ops(routes),
        policy: policy.map(policy_snapshot),
        dns: dns.cloned(),
        report: Some(report.clone()),
    })
}

pub(super) fn clear_recovery_journal() -> Result<(), NetworkError> {
    let path = recovery_journal_path();
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(NetworkError::Io(err)),
    }
}

pub(super) fn write_persisted_apply_report(report: &RouteApplyReport) -> Result<(), NetworkError> {
    let path = last_apply_report_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string(report).map_err(|err| {
        NetworkError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, err))
    })?;
    fs::write(path, json)?;
    Ok(())
}

pub(super) fn load_persisted_apply_report() -> Result<Option<RouteApplyReport>, NetworkError> {
    let path = last_apply_report_path();
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(NetworkError::Io(err)),
    };
    let report = serde_json::from_str(&text).map_err(|err| {
        NetworkError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, err))
    })?;
    Ok(Some(report))
}

pub(super) fn clear_persisted_apply_report() -> Result<(), NetworkError> {
    let path = last_apply_report_path();
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(NetworkError::Io(err)),
    }
}

pub(super) fn attempt_startup_repair_sync() -> Result<(), NetworkError> {
    let runtime = tokio::runtime::Runtime::new().map_err(NetworkError::Io)?;
    runtime.block_on(async { attempt_startup_repair().await })
}

async fn attempt_startup_repair() -> Result<(), NetworkError> {
    let Some(journal) = load_recovery_journal()? else {
        return Ok(());
    };

    let netlink = netlink_handle()?;
    let handle = netlink.handle();

    if !journal.routes.is_empty() || journal.policy.is_some() {
        let _ = cleanup_exact_snapshot(handle, &journal.routes, journal.policy.as_ref()).await;
    } else {
        let stateless_result = async {
            if let Ok(link_index) = link_index(handle, &journal.tun_name).await {
                let _ =
                    cleanup_stale_default_routes_once(handle, &journal.tun_name, link_index).await;
            }
            let _ = cleanup_policy_rules_once(handle).await;
            Ok::<_, NetworkError>(())
        }
        .await;
        stateless_result?;
    }

    netlink.shutdown().await;

    if let Some(dns) = journal.dns {
        cleanup_dns(&journal.tun_name, dns).await?;
    }

    clear_recovery_journal()
}

fn load_recovery_journal() -> Result<Option<RecoveryJournal>, NetworkError> {
    let path = recovery_journal_path();
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(NetworkError::Io(err)),
    };
    let journal = serde_json::from_str(&text).map_err(|err| {
        NetworkError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, err))
    })?;
    Ok(Some(journal))
}

fn write_recovery_journal(journal: &RecoveryJournal) -> Result<(), NetworkError> {
    let path = recovery_journal_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string(journal).map_err(|err| {
        NetworkError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, err))
    })?;
    fs::write(path, json)?;
    Ok(())
}

fn recovery_journal_path() -> PathBuf {
    if let Some(dir) = env::var_os("STATE_DIRECTORY") {
        return PathBuf::from(dir).join(RECOVERY_JOURNAL_FILE);
    }
    PathBuf::from("/var/lib/r-wg").join(RECOVERY_JOURNAL_FILE)
}

fn last_apply_report_path() -> PathBuf {
    if let Some(dir) = env::var_os("STATE_DIRECTORY") {
        return PathBuf::from(dir).join(LAST_APPLY_REPORT_FILE);
    }
    PathBuf::from("/var/lib/r-wg").join(LAST_APPLY_REPORT_FILE)
}

fn route_snapshots(route_plan: &RoutePlan) -> Vec<RecoveryRouteSnapshot> {
    route_snapshots_from_ops(&route_plan.route_ops)
}

fn route_snapshots_from_ops(route_ops: &[RoutePlanRouteOp]) -> Vec<RecoveryRouteSnapshot> {
    route_ops
        .iter()
        .filter(|route_op| matches!(route_op.kind, RoutePlanRouteKind::Allowed))
        .map(|route_op| RecoveryRouteSnapshot {
            addr: route_op.route.addr,
            cidr: route_op.route.cidr,
            table_id: route_op.table_id,
        })
        .collect()
}

fn policy_snapshot(policy: &PolicyRoutingState) -> RecoveryPolicySnapshot {
    RecoveryPolicySnapshot {
        table_id: policy.table_id,
        fwmark: policy.fwmark,
        v4: policy.v4,
        v6: policy.v6,
    }
}

pub(super) async fn cleanup_exact_snapshot(
    handle: &Handle,
    routes: &[RecoveryRouteSnapshot],
    policy: Option<&RecoveryPolicySnapshot>,
) -> Result<(), NetworkError> {
    for route in routes {
        let _ = cleanup_route_snapshot(handle, route).await;
    }

    if let Some(policy) = policy {
        let _ = cleanup_policy_snapshot(handle, policy).await;
    }

    Ok(())
}

async fn cleanup_route_snapshot(
    handle: &Handle,
    snapshot: &RecoveryRouteSnapshot,
) -> Result<(), NetworkError> {
    let route = AllowedIp {
        addr: snapshot.addr,
        cidr: snapshot.cidr,
    };
    let message = build_route_message_without_oif(&route, snapshot.table_id);
    let _ = handle.route().del(message.clone()).execute().await;

    let filter = match snapshot.addr {
        IpAddr::V4(_) => RouteMessageBuilder::<Ipv4Addr>::default().build(),
        IpAddr::V6(_) => RouteMessageBuilder::<Ipv6Addr>::default().build(),
    };

    let mut routes = handle.route().get(filter).execute();
    while let Some(message) = routes.try_next().await? {
        if route_message_matches_snapshot(&message, snapshot) {
            let _ = handle.route().del(message).execute().await;
            break;
        }
    }

    Ok(())
}

async fn cleanup_policy_snapshot(
    handle: &Handle,
    snapshot: &RecoveryPolicySnapshot,
) -> Result<(), NetworkError> {
    let messages = policy_rule_messages(snapshot);
    for message in messages {
        let _ = handle.rule().del(message).execute().await;
    }
    Ok(())
}

pub(super) async fn cleanup_policy_state(
    handle: &Handle,
    policy: &PolicyRoutingState,
) -> Result<(), NetworkError> {
    let snapshot = policy_snapshot(policy);
    cleanup_policy_snapshot(handle, &snapshot).await
}

fn build_route_message_without_oif(route: &AllowedIp, table_id: Option<u32>) -> RouteMessage {
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

fn route_message_matches_snapshot(
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

fn policy_rule_messages(snapshot: &RecoveryPolicySnapshot) -> Vec<RuleMessage> {
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
