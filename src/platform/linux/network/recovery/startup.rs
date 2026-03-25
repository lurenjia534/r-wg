use std::future::Future;
use std::pin::Pin;

use super::super::dns::{cleanup_dns, DnsState};
use super::super::netlink::{link_index, netlink_handle, NetlinkConnection};
use super::super::policy::{cleanup_policy_rules_once, cleanup_stale_default_routes_once};
use super::super::NetworkError;
use super::{
    cleanup_exact_snapshot, clear_recovery_journal, journal_requires_exact_cleanup,
    load_recovery_journal, RecoveryJournal, RecoveryPolicySnapshot, RecoveryRouteSnapshot,
};

type LocalBoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

pub(crate) fn attempt_startup_repair_sync() -> Result<(), NetworkError> {
    let runtime = tokio::runtime::Runtime::new().map_err(NetworkError::Io)?;
    runtime.block_on(async { attempt_startup_repair().await })
}

async fn attempt_startup_repair() -> Result<(), NetworkError> {
    let backend = SystemStartupRepairBackend;
    attempt_startup_repair_with_backend(&backend).await
}

pub(crate) async fn attempt_startup_repair_with_backend<B: StartupRepairBackend>(
    backend: &B,
) -> Result<(), NetworkError> {
    let Some(journal) = backend.load_journal()? else {
        return Ok(());
    };

    let session = backend.open_session().await?;

    if journal_requires_exact_cleanup(&journal) {
        let _ = backend
            .cleanup_exact_snapshot(&session, &journal.routes, journal.policy.as_ref())
            .await;
    } else {
        let stateless_result = async {
            if let Ok(link_index) = backend.link_index(&session, &journal.tun_name).await {
                let _ = backend
                    .cleanup_stale_default_routes_once(&session, &journal.tun_name, link_index)
                    .await;
            }
            let _ = backend.cleanup_policy_rules_once(&session).await;
            Ok::<_, NetworkError>(())
        }
        .await;
        stateless_result?;
    }

    backend.close_session(session).await;

    if let Some(dns) = journal.dns {
        backend.cleanup_dns(&journal.tun_name, dns).await?;
    }

    backend.clear_recovery_journal()
}

pub(crate) trait StartupRepairBackend {
    type Session;

    fn load_journal(&self) -> Result<Option<RecoveryJournal>, NetworkError>;
    fn clear_recovery_journal(&self) -> Result<(), NetworkError>;
    fn open_session<'a>(&'a self) -> LocalBoxFuture<'a, Result<Self::Session, NetworkError>>;
    fn close_session<'a>(&'a self, session: Self::Session) -> LocalBoxFuture<'a, ()>;
    fn cleanup_exact_snapshot<'a>(
        &'a self,
        session: &'a Self::Session,
        routes: &'a [RecoveryRouteSnapshot],
        policy: Option<&'a RecoveryPolicySnapshot>,
    ) -> LocalBoxFuture<'a, Result<(), NetworkError>>;
    fn link_index<'a>(
        &'a self,
        session: &'a Self::Session,
        tun_name: &'a str,
    ) -> LocalBoxFuture<'a, Result<u32, NetworkError>>;
    fn cleanup_stale_default_routes_once<'a>(
        &'a self,
        session: &'a Self::Session,
        tun_name: &'a str,
        link_index: u32,
    ) -> LocalBoxFuture<'a, Result<(), NetworkError>>;
    fn cleanup_policy_rules_once<'a>(
        &'a self,
        session: &'a Self::Session,
    ) -> LocalBoxFuture<'a, Result<(), NetworkError>>;
    fn cleanup_dns<'a>(
        &'a self,
        tun_name: &'a str,
        dns: DnsState,
    ) -> LocalBoxFuture<'a, Result<(), NetworkError>>;
}

struct SystemStartupRepairBackend;

impl StartupRepairBackend for SystemStartupRepairBackend {
    type Session = NetlinkConnection;

    fn load_journal(&self) -> Result<Option<RecoveryJournal>, NetworkError> {
        load_recovery_journal()
    }

    fn clear_recovery_journal(&self) -> Result<(), NetworkError> {
        clear_recovery_journal()
    }

    fn open_session<'a>(&'a self) -> LocalBoxFuture<'a, Result<Self::Session, NetworkError>> {
        Box::pin(async { netlink_handle() })
    }

    fn close_session<'a>(&'a self, session: Self::Session) -> LocalBoxFuture<'a, ()> {
        Box::pin(async move {
            session.shutdown().await;
        })
    }

    fn cleanup_exact_snapshot<'a>(
        &'a self,
        session: &'a Self::Session,
        routes: &'a [RecoveryRouteSnapshot],
        policy: Option<&'a RecoveryPolicySnapshot>,
    ) -> LocalBoxFuture<'a, Result<(), NetworkError>> {
        Box::pin(async move { cleanup_exact_snapshot(session.handle(), routes, policy).await })
    }

    fn link_index<'a>(
        &'a self,
        session: &'a Self::Session,
        tun_name: &'a str,
    ) -> LocalBoxFuture<'a, Result<u32, NetworkError>> {
        Box::pin(async move { link_index(session.handle(), tun_name).await })
    }

    fn cleanup_stale_default_routes_once<'a>(
        &'a self,
        session: &'a Self::Session,
        tun_name: &'a str,
        link_index: u32,
    ) -> LocalBoxFuture<'a, Result<(), NetworkError>> {
        Box::pin(async move {
            cleanup_stale_default_routes_once(session.handle(), tun_name, link_index).await
        })
    }

    fn cleanup_policy_rules_once<'a>(
        &'a self,
        session: &'a Self::Session,
    ) -> LocalBoxFuture<'a, Result<(), NetworkError>> {
        Box::pin(async move { cleanup_policy_rules_once(session.handle(), None).await })
    }

    fn cleanup_dns<'a>(
        &'a self,
        tun_name: &'a str,
        dns: DnsState,
    ) -> LocalBoxFuture<'a, Result<(), NetworkError>> {
        Box::pin(async move { cleanup_dns(tun_name, dns).await })
    }
}
