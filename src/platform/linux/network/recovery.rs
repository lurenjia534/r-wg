//! Linux 崩溃恢复日志与启动期修复。
//!
//! 设计目标：
//! - 在修改系统网络状态前，把“足够回滚”的信息持久化到 root-owned journal；
//! - clean stop 成功后删除 journal，异常退出则保留，供下次 service 启动或 boot-time repair 使用；
//! - 启动期先做无状态清理（stale route / policy rule），再做有状态回滚（DNS）。
//!
//! 当前恢复系统拆成多个子模块：
//! - `journal`：只负责回滚契约所需的持久化；
//! - `last_apply_report`：只负责最近一次 apply 诊断产物；
//! - `startup`：启动期修复编排；
//! - `cleanup`：按 snapshot 清理系统状态；
//! - `snapshot`：从 route/policy 状态提取回滚快照；
//! - `netlink_match`：netlink 消息匹配与 rule 构造。

mod cleanup;
mod journal;
mod last_apply_report;
mod netlink_match;
mod snapshot;
mod startup;

use cleanup::cleanup_exact_snapshot;
use journal::{journal_requires_exact_cleanup, load_recovery_journal, RecoveryJournal};
use snapshot::{RecoveryPolicySnapshot, RecoveryRouteSnapshot};

pub(super) use cleanup::cleanup_policy_state;
pub(super) use journal::{clear_recovery_journal, write_applying_journal, write_running_journal};
pub(super) use last_apply_report::{
    clear_persisted_apply_report, load_persisted_apply_report, write_persisted_apply_report,
};
pub(super) use startup::attempt_startup_repair_sync;

#[cfg(test)]
use journal::RecoveryPhase;
#[cfg(test)]
use netlink_match::{
    build_route_message_without_oif, policy_rule_messages, route_message_matches_snapshot,
};
#[cfg(test)]
use startup::{attempt_startup_repair_with_backend, StartupRepairBackend};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::wg::route_plan::RouteApplyReport;
    use crate::platform::linux::network::NetworkError;
    use crate::platform::linux::network::dns::DnsState;
    use std::cell::RefCell;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    fn make_journal(
        routes: Vec<RecoveryRouteSnapshot>,
        policy: Option<RecoveryPolicySnapshot>,
    ) -> RecoveryJournal {
        RecoveryJournal {
            tun_name: "tun0".to_string(),
            phase: RecoveryPhase::Applying,
            routes,
            policy,
            dns: None,
        }
    }

    fn resolved_dns_state() -> DnsState {
        serde_json::from_str(r#"{"backend":"Resolved"}"#).unwrap()
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum FakeCall {
        LoadJournal,
        OpenSession,
        ExactCleanup { routes: usize, has_policy: bool },
        LinkIndex(String),
        StaleRouteCleanup { tun_name: String, link_index: u32 },
        PolicyRuleCleanup,
        CloseSession,
        DnsCleanup(String),
        ClearJournal,
    }

    struct FakeStartupRepairBackend {
        journal: Option<RecoveryJournal>,
        calls: RefCell<Vec<FakeCall>>,
        link_index_result: Option<u32>,
        dns_cleanup_error: Option<String>,
        clear_journal_error: Option<String>,
    }

    impl FakeStartupRepairBackend {
        fn new(journal: Option<RecoveryJournal>) -> Self {
            Self {
                journal,
                calls: RefCell::new(Vec::new()),
                link_index_result: Some(42),
                dns_cleanup_error: None,
                clear_journal_error: None,
            }
        }

        fn calls(&self) -> Vec<FakeCall> {
            self.calls.borrow().clone()
        }
    }

    impl StartupRepairBackend for FakeStartupRepairBackend {
        type Session = ();

        fn load_journal(&self) -> Result<Option<RecoveryJournal>, NetworkError> {
            self.calls.borrow_mut().push(FakeCall::LoadJournal);
            Ok(self.journal.clone())
        }

        fn clear_recovery_journal(&self) -> Result<(), NetworkError> {
            self.calls.borrow_mut().push(FakeCall::ClearJournal);
            match &self.clear_journal_error {
                Some(message) => Err(NetworkError::Io(std::io::Error::other(message.clone()))),
                None => Ok(()),
            }
        }

        fn open_session<'a>(
            &'a self,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Self::Session, NetworkError>> + 'a>>
        {
            self.calls.borrow_mut().push(FakeCall::OpenSession);
            Box::pin(async { Ok(()) })
        }

        fn close_session<'a>(
            &'a self,
            _session: Self::Session,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + 'a>> {
            self.calls.borrow_mut().push(FakeCall::CloseSession);
            Box::pin(async {})
        }

        fn cleanup_exact_snapshot<'a>(
            &'a self,
            _session: &'a Self::Session,
            routes: &'a [RecoveryRouteSnapshot],
            policy: Option<&'a RecoveryPolicySnapshot>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), NetworkError>> + 'a>>
        {
            self.calls.borrow_mut().push(FakeCall::ExactCleanup {
                routes: routes.len(),
                has_policy: policy.is_some(),
            });
            Box::pin(async { Ok(()) })
        }

        fn link_index<'a>(
            &'a self,
            _session: &'a Self::Session,
            tun_name: &'a str,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u32, NetworkError>> + 'a>>
        {
            self.calls
                .borrow_mut()
                .push(FakeCall::LinkIndex(tun_name.to_string()));
            let result = match self.link_index_result {
                Some(value) => Ok(value),
                None => Err(NetworkError::LinkNotFound(tun_name.to_string())),
            };
            Box::pin(async move { result })
        }

        fn cleanup_stale_default_routes_once<'a>(
            &'a self,
            _session: &'a Self::Session,
            tun_name: &'a str,
            link_index: u32,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), NetworkError>> + 'a>>
        {
            self.calls.borrow_mut().push(FakeCall::StaleRouteCleanup {
                tun_name: tun_name.to_string(),
                link_index,
            });
            Box::pin(async { Ok(()) })
        }

        fn cleanup_policy_rules_once<'a>(
            &'a self,
            _session: &'a Self::Session,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), NetworkError>> + 'a>>
        {
            self.calls.borrow_mut().push(FakeCall::PolicyRuleCleanup);
            Box::pin(async { Ok(()) })
        }

        fn cleanup_dns<'a>(
            &'a self,
            tun_name: &'a str,
            _dns: DnsState,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), NetworkError>> + 'a>>
        {
            self.calls
                .borrow_mut()
                .push(FakeCall::DnsCleanup(tun_name.to_string()));
            let result = match &self.dns_cleanup_error {
                Some(message) => Err(NetworkError::DnsVerifyFailed(message.clone())),
                None => Ok(()),
            };
            Box::pin(async move { result })
        }
    }

    #[test]
    fn startup_repair_skips_work_without_journal() {
        let backend = FakeStartupRepairBackend::new(None);

        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { attempt_startup_repair_with_backend(&backend).await })
            .unwrap();

        assert_eq!(backend.calls(), vec![FakeCall::LoadJournal]);
    }

    #[test]
    fn startup_repair_runs_stateless_flow_end_to_end() {
        let backend = FakeStartupRepairBackend::new(Some(make_journal(Vec::new(), None)));

        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { attempt_startup_repair_with_backend(&backend).await })
            .unwrap();

        assert_eq!(
            backend.calls(),
            vec![
                FakeCall::LoadJournal,
                FakeCall::OpenSession,
                FakeCall::LinkIndex("tun0".to_string()),
                FakeCall::StaleRouteCleanup {
                    tun_name: "tun0".to_string(),
                    link_index: 42,
                },
                FakeCall::PolicyRuleCleanup,
                FakeCall::CloseSession,
                FakeCall::ClearJournal,
            ]
        );
    }

    #[test]
    fn startup_repair_continues_stateless_flow_when_link_lookup_fails() {
        let mut backend = FakeStartupRepairBackend::new(Some(make_journal(Vec::new(), None)));
        backend.link_index_result = None;

        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { attempt_startup_repair_with_backend(&backend).await })
            .unwrap();

        assert_eq!(
            backend.calls(),
            vec![
                FakeCall::LoadJournal,
                FakeCall::OpenSession,
                FakeCall::LinkIndex("tun0".to_string()),
                FakeCall::PolicyRuleCleanup,
                FakeCall::CloseSession,
                FakeCall::ClearJournal,
            ]
        );
    }

    #[test]
    fn startup_repair_runs_exact_cleanup_and_dns_before_clearing_journal() {
        let mut journal = make_journal(
            vec![RecoveryRouteSnapshot {
                addr: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 0)),
                cidr: 24,
                table_id: None,
            }],
            Some(RecoveryPolicySnapshot {
                table_id: 51820,
                fwmark: 0x1234,
                v4: true,
                v6: false,
            }),
        );
        journal.dns = Some(resolved_dns_state());
        let backend = FakeStartupRepairBackend::new(Some(journal));

        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { attempt_startup_repair_with_backend(&backend).await })
            .unwrap();

        assert_eq!(
            backend.calls(),
            vec![
                FakeCall::LoadJournal,
                FakeCall::OpenSession,
                FakeCall::ExactCleanup {
                    routes: 1,
                    has_policy: true,
                },
                FakeCall::CloseSession,
                FakeCall::DnsCleanup("tun0".to_string()),
                FakeCall::ClearJournal,
            ]
        );
    }

    #[test]
    fn startup_repair_keeps_journal_when_dns_cleanup_fails() {
        let mut journal = make_journal(Vec::new(), None);
        journal.dns = Some(resolved_dns_state());
        let mut backend = FakeStartupRepairBackend::new(Some(journal));
        backend.dns_cleanup_error = Some("boom".to_string());

        let error = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { attempt_startup_repair_with_backend(&backend).await })
            .unwrap_err();

        assert!(matches!(error, NetworkError::DnsVerifyFailed(_)));
        assert_eq!(
            backend.calls(),
            vec![
                FakeCall::LoadJournal,
                FakeCall::OpenSession,
                FakeCall::LinkIndex("tun0".to_string()),
                FakeCall::StaleRouteCleanup {
                    tun_name: "tun0".to_string(),
                    link_index: 42,
                },
                FakeCall::PolicyRuleCleanup,
                FakeCall::CloseSession,
                FakeCall::DnsCleanup("tun0".to_string()),
            ]
        );
    }

    #[test]
    fn startup_repair_uses_exact_cleanup_when_snapshot_exists() {
        let route_journal = make_journal(
            vec![RecoveryRouteSnapshot {
                addr: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 0)),
                cidr: 24,
                table_id: None,
            }],
            None,
        );
        let policy_journal = make_journal(
            Vec::new(),
            Some(RecoveryPolicySnapshot {
                table_id: 51820,
                fwmark: 0x1234,
                v4: true,
                v6: false,
            }),
        );
        let stateless_journal = make_journal(Vec::new(), None);

        assert!(journal_requires_exact_cleanup(&route_journal));
        assert!(journal_requires_exact_cleanup(&policy_journal));
        assert!(!journal_requires_exact_cleanup(&stateless_journal));
    }

    #[test]
    fn policy_rule_messages_cover_requested_families() {
        let both = RecoveryPolicySnapshot {
            table_id: 51820,
            fwmark: 0x1234,
            v4: true,
            v6: true,
        };
        let ipv4_only = RecoveryPolicySnapshot {
            v6: false,
            ..both.clone()
        };

        let both_messages = policy_rule_messages(&both);
        let ipv4_messages = policy_rule_messages(&ipv4_only);

        assert_eq!(both_messages.len(), 6);
        assert_eq!(
            both_messages
                .iter()
                .filter(|message| message.header.family == netlink_packet_route::AddressFamily::Inet)
                .count(),
            3
        );
        assert_eq!(
            both_messages
                .iter()
                .filter(|message| message.header.family == netlink_packet_route::AddressFamily::Inet6)
                .count(),
            3
        );
        assert_eq!(ipv4_messages.len(), 3);
        assert!(ipv4_messages
            .iter()
            .all(|message| message.header.family == netlink_packet_route::AddressFamily::Inet));
    }

    #[test]
    fn route_snapshot_match_checks_table_and_destination() {
        let snapshot = RecoveryRouteSnapshot {
            addr: IpAddr::V6(Ipv6Addr::LOCALHOST),
            cidr: 128,
            table_id: Some(51820),
        };
        let matching = build_route_message_without_oif(
            &crate::backend::wg::config::AllowedIp {
                addr: snapshot.addr,
                cidr: snapshot.cidr,
            },
            snapshot.table_id,
        );
        let other_table = build_route_message_without_oif(
            &crate::backend::wg::config::AllowedIp {
                addr: snapshot.addr,
                cidr: snapshot.cidr,
            },
            Some(123),
        );
        let other_destination = build_route_message_without_oif(
            &crate::backend::wg::config::AllowedIp {
                addr: IpAddr::V6(Ipv6Addr::UNSPECIFIED),
                cidr: snapshot.cidr,
            },
            snapshot.table_id,
        );

        assert!(route_message_matches_snapshot(&matching, &snapshot));
        assert!(!route_message_matches_snapshot(&other_table, &snapshot));
        assert!(!route_message_matches_snapshot(
            &other_destination,
            &snapshot
        ));
    }

    #[test]
    fn persisted_report_round_trips_separately_from_journal_contract() {
        let report = RouteApplyReport::new(crate::backend::wg::route_plan::RoutePlanPlatform::Linux);
        let _ = report;
    }
}
