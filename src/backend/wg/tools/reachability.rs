use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};
use std::time::Instant;

use tokio::net::{lookup_host, TcpStream};
use tokio::runtime::Builder;
use tokio::time::{sleep, timeout, Duration};

use crate::backend::wg::config::Endpoint;

use super::ToolError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReachabilityMode {
    ResolveOnly,
    TcpConnect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressFamilyPreference {
    System,
    PreferIpv4,
    PreferIpv6,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReachabilityRequest {
    pub target: String,
    pub mode: ReachabilityMode,
    pub port_override: Option<u16>,
    pub family_preference: AddressFamilyPreference,
    pub timeout_ms: u64,
    pub max_addresses: usize,
    pub stop_on_first_success: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReachabilityAttemptResult {
    Resolved,
    Connected,
    Refused,
    TimedOut,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReachabilityAttempt {
    pub address: SocketAddr,
    pub result: ReachabilityAttemptResult,
    pub elapsed_ms: u64,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReachabilityVerdict {
    Resolved,
    Reachable,
    PartiallyReachable,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReachabilityResult {
    pub normalized_target: String,
    pub mode: ReachabilityMode,
    pub resolved: Vec<SocketAddr>,
    pub attempts: Vec<ReachabilityAttempt>,
    pub verdict: ReachabilityVerdict,
    pub summary: String,
}

pub async fn probe_reachability(
    request: ReachabilityRequest,
) -> Result<ReachabilityResult, ToolError> {
    let normalized = normalize_request(&request)?;
    let timeout_duration = Duration::from_millis(request.timeout_ms.max(1));

    let resolved = match timeout(timeout_duration, resolve_target(&normalized, &request)).await {
        Ok(addrs) => addrs,
        Err(_) => {
            return Ok(ReachabilityResult {
                normalized_target: normalized.display_target.clone(),
                mode: request.mode,
                resolved: Vec::new(),
                attempts: Vec::new(),
                verdict: ReachabilityVerdict::Failed,
                summary: format!(
                    "Timed out resolving {} after {} ms.",
                    normalized.display_target, request.timeout_ms
                ),
            });
        }
    };

    let resolved = match resolved {
        Ok(addrs) => addrs,
        Err(message) => {
            return Ok(ReachabilityResult {
                normalized_target: normalized.display_target.clone(),
                mode: request.mode,
                resolved: Vec::new(),
                attempts: Vec::new(),
                verdict: ReachabilityVerdict::Failed,
                summary: message,
            });
        }
    };

    if request.mode == ReachabilityMode::ResolveOnly {
        return Ok(ReachabilityResult {
            normalized_target: normalized.display_target.clone(),
            mode: request.mode,
            attempts: resolved
                .iter()
                .map(|address| ReachabilityAttempt {
                    address: *address,
                    result: ReachabilityAttemptResult::Resolved,
                    elapsed_ms: 0,
                    message: "resolved".to_string(),
                })
                .collect(),
            verdict: if resolved.is_empty() {
                ReachabilityVerdict::Failed
            } else {
                ReachabilityVerdict::Resolved
            },
            summary: if resolved.is_empty() {
                format!("No addresses resolved for {}.", normalized.display_target)
            } else {
                format!(
                    "Resolved {} address(es) for {}.",
                    resolved.len(),
                    normalized.display_target
                )
            },
            resolved,
        });
    }

    let mut attempts = Vec::new();
    let mut success_count = 0usize;
    for address in &resolved {
        let started = Instant::now();
        let attempt = match timeout(timeout_duration, TcpStream::connect(*address)).await {
            Ok(Ok(_stream)) => {
                success_count += 1;
                ReachabilityAttempt {
                    address: *address,
                    result: ReachabilityAttemptResult::Connected,
                    elapsed_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                    message: "connected".to_string(),
                }
            }
            Err(_) => ReachabilityAttempt {
                address: *address,
                result: ReachabilityAttemptResult::TimedOut,
                elapsed_ms: request.timeout_ms,
                message: format!("timed out after {} ms", request.timeout_ms),
            },
            Ok(Err(err)) => ReachabilityAttempt {
                address: *address,
                result: if matches!(err.kind(), std::io::ErrorKind::ConnectionRefused) {
                    ReachabilityAttemptResult::Refused
                } else {
                    ReachabilityAttemptResult::Failed
                },
                elapsed_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                message: err.to_string(),
            },
        };
        attempts.push(attempt);
        if request.stop_on_first_success && success_count > 0 {
            break;
        }
    }

    let verdict = match success_count {
        0 => ReachabilityVerdict::Failed,
        count if count == attempts.len() => ReachabilityVerdict::Reachable,
        _ => ReachabilityVerdict::PartiallyReachable,
    };
    let summary = match verdict {
        ReachabilityVerdict::Reachable => format!(
            "Connected to {} on {} address(es).",
            normalized.display_target, success_count
        ),
        ReachabilityVerdict::PartiallyReachable => format!(
            "Connected to {} on {} of {} attempt(s).",
            normalized.display_target,
            success_count,
            attempts.len()
        ),
        ReachabilityVerdict::Failed => format!(
            "No TCP connection to {} succeeded across {} attempt(s).",
            normalized.display_target,
            attempts.len()
        ),
        ReachabilityVerdict::Resolved => unreachable!("tcp connect never returns resolved verdict"),
    };

    Ok(ReachabilityResult {
        normalized_target: normalized.display_target,
        mode: request.mode,
        resolved,
        attempts,
        verdict,
        summary,
    })
}

pub fn probe_reachability_blocking(
    request: ReachabilityRequest,
) -> Result<ReachabilityResult, ToolError> {
    let runtime = Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|err| {
            ToolError::Runtime(format!("failed to initialize reachability runtime: {err}"))
        })?;
    runtime.block_on(probe_reachability(request))
}

pub async fn probe_reachability_until_cancel(
    request: ReachabilityRequest,
    is_cancelled: impl Fn() -> bool + Send + Sync + 'static,
) -> Result<ReachabilityResult, String> {
    tokio::select! {
        _ = wait_for_cancel(is_cancelled) => Err("cancelled".to_string()),
        result = probe_reachability(request) => result.map_err(|err| err.to_string()),
    }
}

pub fn probe_reachability_blocking_until_cancel(
    request: ReachabilityRequest,
    is_cancelled: impl Fn() -> bool + Send + Sync + 'static,
) -> Result<ReachabilityResult, String> {
    let runtime = Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|err| format!("failed to initialize reachability runtime: {err}"))?;
    runtime.block_on(probe_reachability_until_cancel(request, is_cancelled))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalizedRequest {
    host: String,
    display_target: String,
    port: Option<u16>,
}

fn normalize_request(request: &ReachabilityRequest) -> Result<NormalizedRequest, ToolError> {
    let target = request.target.trim();
    if target.is_empty() {
        return Err(ToolError::InvalidTarget("target is empty".to_string()));
    }

    let inline_endpoint = parse_endpoint_like(target);
    let host = inline_endpoint
        .as_ref()
        .map(|endpoint| endpoint.host.clone())
        .unwrap_or_else(|| target.to_string());
    let port = inline_endpoint
        .as_ref()
        .map(|endpoint| endpoint.port)
        .or(request.port_override);

    if request.mode == ReachabilityMode::TcpConnect && port.is_none() {
        return Err(ToolError::MissingPort);
    }

    let display_target = match port {
        Some(port) => format_endpoint_display_parts(&host, port),
        None => host.clone(),
    };

    Ok(NormalizedRequest {
        host,
        display_target,
        port,
    })
}

fn parse_endpoint_like(target: &str) -> Option<Endpoint> {
    if let Ok(ip) = target.parse::<IpAddr>() {
        let _ = ip;
        return None;
    }

    if target.starts_with('[') {
        return target.parse::<Endpoint>().ok();
    }

    let (host, port_text) = target.rsplit_once(':')?;
    if host.contains(':') {
        return None;
    }
    let port = port_text.parse::<u16>().ok()?;
    if port == 0 {
        return None;
    }

    Some(Endpoint {
        host: host.to_string(),
        port,
    })
}

async fn resolve_target(
    normalized: &NormalizedRequest,
    request: &ReachabilityRequest,
) -> Result<Vec<SocketAddr>, String> {
    let port = normalized.port.unwrap_or(0);
    let addrs = if let Ok(ip) = normalized.host.parse::<IpAddr>() {
        vec![SocketAddr::new(ip, port)]
    } else {
        lookup_host((normalized.host.as_str(), port))
            .await
            .map_err(|err| format!("Failed to resolve {}: {err}", normalized.host))?
            .collect::<Vec<_>>()
    };

    if addrs.is_empty() {
        return Ok(Vec::new());
    }

    Ok(order_addresses(
        addrs,
        request.family_preference,
        request.max_addresses.max(1),
    ))
}

fn order_addresses(
    addrs: Vec<SocketAddr>,
    preference: AddressFamilyPreference,
    max_addresses: usize,
) -> Vec<SocketAddr> {
    let mut seen = HashSet::new();
    let mut ordered = Vec::new();
    for addr in addrs {
        if seen.insert(addr) {
            ordered.push(addr);
        }
    }

    match preference {
        AddressFamilyPreference::System => {}
        AddressFamilyPreference::PreferIpv4 => {
            ordered.sort_by_key(|addr| if addr.is_ipv4() { 0 } else { 1 });
        }
        AddressFamilyPreference::PreferIpv6 => {
            ordered.sort_by_key(|addr| if addr.is_ipv6() { 0 } else { 1 });
        }
    }

    ordered.truncate(max_addresses);
    ordered
}

pub fn format_endpoint_display(endpoint: &Endpoint) -> String {
    format_endpoint_display_parts(&endpoint.host, endpoint.port)
}

fn format_endpoint_display_parts(host: &str, port: u16) -> String {
    if host.contains(':')
        && host
            .parse::<IpAddr>()
            .map(|ip| ip.is_ipv6())
            .unwrap_or(false)
    {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

async fn wait_for_cancel(is_cancelled: impl Fn() -> bool) {
    while !is_cancelled() {
        sleep(Duration::from_millis(25)).await;
    }
}

#[cfg(test)]
mod tests {
    use std::io::ErrorKind;
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddrV4, SocketAddrV6};

    use tokio::net::TcpListener;

    use super::*;

    fn request(target: &str, mode: ReachabilityMode) -> ReachabilityRequest {
        ReachabilityRequest {
            target: target.to_string(),
            mode,
            port_override: None,
            family_preference: AddressFamilyPreference::PreferIpv4,
            timeout_ms: 400,
            max_addresses: 8,
            stop_on_first_success: true,
        }
    }

    #[tokio::test]
    async fn resolve_only_accepts_literal_ip_without_port() {
        let result = probe_reachability(request("203.0.113.10", ReachabilityMode::ResolveOnly))
            .await
            .unwrap();

        assert_eq!(result.verdict, ReachabilityVerdict::Resolved);
        assert_eq!(result.resolved, vec!["203.0.113.10:0".parse().unwrap()]);
    }

    #[tokio::test]
    async fn resolve_only_supports_localhost() {
        let result = probe_reachability(request("localhost", ReachabilityMode::ResolveOnly))
            .await
            .unwrap();

        assert_eq!(result.verdict, ReachabilityVerdict::Resolved);
        assert!(!result.resolved.is_empty());
    }

    #[tokio::test]
    async fn tcp_connect_succeeds_against_loopback_listener() {
        let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await {
            Ok(listener) => listener,
            Err(err) if err.kind() == ErrorKind::PermissionDenied => return,
            Err(err) => panic!("failed to bind loopback listener: {err}"),
        };
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = listener.accept().await;
        });

        let result = probe_reachability(request(
            &format!("127.0.0.1:{}", addr.port()),
            ReachabilityMode::TcpConnect,
        ))
        .await
        .unwrap();

        assert_eq!(result.verdict, ReachabilityVerdict::Reachable);
        assert_eq!(result.attempts.len(), 1);
        assert_eq!(
            result.attempts[0].result,
            ReachabilityAttemptResult::Connected
        );
    }

    #[tokio::test]
    async fn tcp_connect_reports_refused_for_unused_port() {
        let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await {
            Ok(listener) => listener,
            Err(err) if err.kind() == ErrorKind::PermissionDenied => return,
            Err(err) => panic!("failed to bind loopback listener: {err}"),
        };
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let result = probe_reachability(request(
            &format!("127.0.0.1:{}", addr.port()),
            ReachabilityMode::TcpConnect,
        ))
        .await
        .unwrap();

        assert_eq!(result.verdict, ReachabilityVerdict::Failed);
        assert_eq!(result.attempts.len(), 1);
    }

    #[tokio::test]
    async fn tcp_connect_requires_port() {
        let err = probe_reachability(request("example.com", ReachabilityMode::TcpConnect))
            .await
            .unwrap_err();

        assert_eq!(err, ToolError::MissingPort);
    }

    #[tokio::test]
    async fn resolve_failure_returns_failed_verdict() {
        let result = probe_reachability(request(
            "no-such-host.invalid",
            ReachabilityMode::ResolveOnly,
        ))
        .await
        .unwrap();

        assert_eq!(result.verdict, ReachabilityVerdict::Failed);
        assert!(result.resolved.is_empty());
    }

    #[test]
    fn order_addresses_prefers_ipv4() {
        let addrs = vec![
            SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::LOCALHOST, 443, 0, 0)),
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 443)),
        ];

        let ordered = order_addresses(addrs, AddressFamilyPreference::PreferIpv4, 8);

        assert!(ordered[0].is_ipv4());
        assert!(ordered[1].is_ipv6());
    }

    #[test]
    fn order_addresses_prefers_ipv6() {
        let addrs = vec![
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 443)),
            SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::LOCALHOST, 443, 0, 0)),
        ];

        let ordered = order_addresses(addrs, AddressFamilyPreference::PreferIpv6, 8);

        assert!(ordered[0].is_ipv6());
        assert!(ordered[1].is_ipv4());
    }

    #[test]
    fn order_addresses_honors_max_addresses_and_deduplicates() {
        let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 443));
        let ordered = order_addresses(
            vec![
                addr,
                addr,
                SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::LOCALHOST, 443, 0, 0)),
            ],
            AddressFamilyPreference::System,
            1,
        );

        assert_eq!(ordered, vec![addr]);
    }

    #[test]
    fn parse_endpoint_like_keeps_bare_ipv6_literal_without_port() {
        assert!(parse_endpoint_like("2001:db8::1").is_none());
        assert_eq!(
            parse_endpoint_like("[2001:db8::1]:443").unwrap(),
            Endpoint {
                host: "2001:db8::1".to_string(),
                port: 443,
            }
        );
    }

    #[test]
    fn normalize_request_formats_ipv6_targets() {
        let normalized = normalize_request(&ReachabilityRequest {
            target: "[2001:db8::1]:443".to_string(),
            mode: ReachabilityMode::TcpConnect,
            port_override: None,
            family_preference: AddressFamilyPreference::System,
            timeout_ms: 500,
            max_addresses: 8,
            stop_on_first_success: true,
        })
        .unwrap();

        assert_eq!(normalized.display_target, "[2001:db8::1]:443");
    }
}
