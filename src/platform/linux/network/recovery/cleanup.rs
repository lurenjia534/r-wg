use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use futures_util::stream::TryStreamExt;
use netlink_packet_route::route::RouteMessage;
use rtnetlink::{Handle, RouteMessageBuilder};

use super::super::policy::PolicyRoutingState;
use super::super::NetworkError;
use super::netlink_match::{
    build_route_message_without_oif, policy_rule_messages, route_message_matches_snapshot,
};
use super::snapshot::policy_snapshot;
use super::{RecoveryPolicySnapshot, RecoveryRouteSnapshot};
use crate::core::config::AllowedIp;

pub(crate) async fn cleanup_exact_snapshot(
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

pub(crate) async fn cleanup_policy_state(
    handle: &Handle,
    policy: &PolicyRoutingState,
) -> Result<(), NetworkError> {
    let snapshot = policy_snapshot(policy);
    cleanup_policy_snapshot(handle, &snapshot).await
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

    let filter = route_family_filter(snapshot.addr);
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

fn route_family_filter(addr: IpAddr) -> RouteMessage {
    match addr {
        IpAddr::V4(_) => RouteMessageBuilder::<Ipv4Addr>::default().build(),
        IpAddr::V6(_) => RouteMessageBuilder::<Ipv6Addr>::default().build(),
    }
}
