use std::cmp::Ordering;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::core::config::AllowedIp;

use super::ToolError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CidrExclusionStats {
    pub input_include_count: usize,
    pub input_exclude_count: usize,
    pub normalized_include_count: usize,
    pub normalized_exclude_count: usize,
    pub output_count: usize,
    pub host_bits_normalized: bool,
    pub merged_prefixes: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CidrExclusionResult {
    pub normalized_includes: Vec<AllowedIp>,
    pub normalized_excludes: Vec<AllowedIp>,
    pub remaining: Vec<AllowedIp>,
    pub stats: CidrExclusionStats,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CidrNormalizationStats {
    pub input_count: usize,
    pub normalized_count: usize,
    pub host_bits_normalized: bool,
    pub merged_prefixes: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CidrNormalizationResult {
    pub normalized: Vec<AllowedIp>,
    pub stats: CidrNormalizationStats,
}

pub fn parse_tool_prefixes(input: &str) -> Result<Vec<AllowedIp>, ToolError> {
    let mut parsed = Vec::new();

    for raw_line in input.lines() {
        let line = strip_allowed_ips_assignment(raw_line).trim();
        if line.is_empty() {
            continue;
        }

        for raw_token in line.split(',') {
            let token = raw_token.trim();
            if token.is_empty() {
                continue;
            }
            parsed.push(parse_tool_prefix(token)?);
        }
    }

    Ok(parsed)
}

pub fn normalize_cidr_set(
    prefixes: &[AllowedIp],
    limit: usize,
) -> Result<CidrNormalizationResult, ToolError> {
    let (normalized, stats) = normalize_prefixes(prefixes);
    if normalized.len() > limit {
        return Err(ToolError::TooManyResults {
            limit,
            produced: normalized.len(),
        });
    }

    Ok(CidrNormalizationResult {
        normalized,
        stats: CidrNormalizationStats {
            input_count: prefixes.len(),
            normalized_count: stats.count,
            host_bits_normalized: stats.host_bits_normalized,
            merged_prefixes: stats.merged_prefixes,
        },
    })
}

pub fn compute_cidr_exclusion(
    includes: &[AllowedIp],
    excludes: &[AllowedIp],
    limit: usize,
) -> Result<CidrExclusionResult, ToolError> {
    let (normalized_includes, include_stats) = normalize_prefixes(includes);
    let (normalized_excludes, exclude_stats) = normalize_prefixes(excludes);

    let mut remaining = Vec::new();
    for include in &normalized_includes {
        let mut current = vec![include.clone()];
        for exclude in &normalized_excludes {
            let mut next = Vec::new();
            for prefix in current {
                subtract_prefix(&prefix, exclude, &mut next, limit)?;
            }
            current = next;
            if current.is_empty() {
                break;
            }
        }
        remaining.extend(current);
        if remaining.len() > limit {
            return Err(ToolError::TooManyResults {
                limit,
                produced: remaining.len(),
            });
        }
    }

    let (remaining, output_stats) = normalize_prefixes(&remaining);
    if remaining.len() > limit {
        return Err(ToolError::TooManyResults {
            limit,
            produced: remaining.len(),
        });
    }

    Ok(CidrExclusionResult {
        normalized_includes,
        normalized_excludes,
        stats: CidrExclusionStats {
            input_include_count: includes.len(),
            input_exclude_count: excludes.len(),
            normalized_include_count: include_stats.count,
            normalized_exclude_count: exclude_stats.count,
            output_count: remaining.len(),
            host_bits_normalized: include_stats.host_bits_normalized
                || exclude_stats.host_bits_normalized
                || output_stats.host_bits_normalized,
            merged_prefixes: include_stats.merged_prefixes
                || exclude_stats.merged_prefixes
                || output_stats.merged_prefixes,
        },
        remaining,
    })
}

#[derive(Clone, Copy, Default)]
struct NormalizeStats {
    count: usize,
    host_bits_normalized: bool,
    merged_prefixes: bool,
}

fn strip_allowed_ips_assignment(line: &str) -> &str {
    if let Some((lhs, rhs)) = line.split_once('=') {
        if lhs.trim().eq_ignore_ascii_case("AllowedIPs") {
            return rhs;
        }
    }
    line
}

fn parse_tool_prefix(token: &str) -> Result<AllowedIp, ToolError> {
    if token.contains('/') {
        return token
            .parse::<AllowedIp>()
            .map_err(|message| ToolError::ParseToken {
                token: token.to_string(),
                message: message.to_string(),
            });
    }

    let addr = token.parse::<IpAddr>().map_err(|_| ToolError::ParseToken {
        token: token.to_string(),
        message: "invalid ip".to_string(),
    })?;
    let cidr = match addr {
        IpAddr::V4(_) => 32,
        IpAddr::V6(_) => 128,
    };

    Ok(AllowedIp { addr, cidr })
}

fn normalize_prefixes(prefixes: &[AllowedIp]) -> (Vec<AllowedIp>, NormalizeStats) {
    let mut stats = NormalizeStats::default();
    if prefixes.is_empty() {
        return (Vec::new(), stats);
    }

    let mut normalized = Vec::with_capacity(prefixes.len());
    for prefix in prefixes {
        let canonical = canonicalize(prefix);
        if canonical != *prefix {
            stats.host_bits_normalized = true;
        }
        normalized.push(canonical);
    }

    normalized.sort_by(prefix_sort_key);
    normalized.dedup();

    let mut filtered = Vec::with_capacity(normalized.len());
    for prefix in normalized {
        if filtered
            .iter()
            .any(|existing| contains_prefix(existing, &prefix))
        {
            continue;
        }
        filtered.push(prefix);
    }

    let mut changed = true;
    while changed {
        changed = false;
        filtered.sort_by(prefix_sort_key);
        let mut merged = Vec::with_capacity(filtered.len());
        let mut idx = 0;
        while idx < filtered.len() {
            if idx + 1 < filtered.len() {
                if let Some(parent) = sibling_parent(&filtered[idx], &filtered[idx + 1]) {
                    merged.push(parent);
                    changed = true;
                    stats.merged_prefixes = true;
                    idx += 2;
                    continue;
                }
            }
            merged.push(filtered[idx].clone());
            idx += 1;
        }

        let mut collapsed = Vec::with_capacity(merged.len());
        for prefix in merged {
            if collapsed
                .iter()
                .any(|existing| contains_prefix(existing, &prefix))
            {
                changed = true;
                continue;
            }
            collapsed.push(prefix);
        }
        filtered = collapsed;
    }

    stats.count = filtered.len();
    (filtered, stats)
}

fn subtract_prefix(
    include: &AllowedIp,
    exclude: &AllowedIp,
    out: &mut Vec<AllowedIp>,
    limit: usize,
) -> Result<(), ToolError> {
    if !overlaps(include, exclude) {
        out.push(include.clone());
    } else if contains_prefix(exclude, include) {
        return Ok(());
    } else if include.cidr >= max_bits(include.addr) {
        return Ok(());
    } else {
        let (left, right) = split_children(include);
        subtract_prefix(&left, exclude, out, limit)?;
        subtract_prefix(&right, exclude, out, limit)?;
    }

    if out.len() > limit {
        return Err(ToolError::TooManyResults {
            limit,
            produced: out.len(),
        });
    }
    Ok(())
}

fn sibling_parent(left: &AllowedIp, right: &AllowedIp) -> Option<AllowedIp> {
    if left.addr.is_ipv4() != right.addr.is_ipv4() || left.cidr != right.cidr || left.cidr == 0 {
        return None;
    }

    let left = canonicalize(left);
    let right = canonicalize(right);
    if left == right {
        return None;
    }

    let parent = AllowedIp {
        addr: apply_mask(left.addr, left.cidr - 1),
        cidr: left.cidr - 1,
    };

    if contains_prefix(&parent, &left) && contains_prefix(&parent, &right) {
        let (child_a, child_b) = split_children(&parent);
        let ordered_children = if prefix_sort_key(&child_a, &child_b) == Ordering::Greater {
            (child_b, child_a)
        } else {
            (child_a, child_b)
        };
        let ordered_inputs = if prefix_sort_key(&left, &right) == Ordering::Greater {
            (right, left)
        } else {
            (left, right)
        };
        if ordered_children.0 == ordered_inputs.0 && ordered_children.1 == ordered_inputs.1 {
            return Some(parent);
        }
    }

    None
}

fn split_children(prefix: &AllowedIp) -> (AllowedIp, AllowedIp) {
    let next_cidr = prefix.cidr + 1;
    let left = AllowedIp {
        addr: apply_mask(prefix.addr, next_cidr),
        cidr: next_cidr,
    };
    let step = host_increment(prefix.addr, next_cidr);
    let right = AllowedIp {
        addr: add_offset(left.addr, step),
        cidr: next_cidr,
    };
    (left, right)
}

fn contains_prefix(container: &AllowedIp, candidate: &AllowedIp) -> bool {
    if container.addr.is_ipv4() != candidate.addr.is_ipv4() || container.cidr > candidate.cidr {
        return false;
    }

    canonicalize(container).addr == apply_mask(candidate.addr, container.cidr)
}

fn overlaps(left: &AllowedIp, right: &AllowedIp) -> bool {
    if left.addr.is_ipv4() != right.addr.is_ipv4() {
        return false;
    }

    let common = left.cidr.min(right.cidr);
    apply_mask(left.addr, common) == apply_mask(right.addr, common)
}

fn canonicalize(prefix: &AllowedIp) -> AllowedIp {
    AllowedIp {
        addr: apply_mask(prefix.addr, prefix.cidr),
        cidr: prefix.cidr,
    }
}

fn prefix_sort_key(left: &AllowedIp, right: &AllowedIp) -> Ordering {
    match (left.addr, right.addr) {
        (IpAddr::V4(a), IpAddr::V4(b)) => {
            (u32::from(a), left.cidr).cmp(&(u32::from(b), right.cidr))
        }
        (IpAddr::V6(a), IpAddr::V6(b)) => {
            (u128::from(a), left.cidr).cmp(&(u128::from(b), right.cidr))
        }
        (IpAddr::V4(_), IpAddr::V6(_)) => Ordering::Less,
        (IpAddr::V6(_), IpAddr::V4(_)) => Ordering::Greater,
    }
}

fn apply_mask(addr: IpAddr, cidr: u8) -> IpAddr {
    match addr {
        IpAddr::V4(addr) => IpAddr::V4(apply_v4_mask(addr, cidr)),
        IpAddr::V6(addr) => IpAddr::V6(apply_v6_mask(addr, cidr)),
    }
}

fn apply_v4_mask(addr: Ipv4Addr, cidr: u8) -> Ipv4Addr {
    let raw = u32::from(addr);
    let masked = if cidr == 0 {
        0
    } else {
        let shift = 32u32.saturating_sub(u32::from(cidr));
        raw & (u32::MAX << shift)
    };
    Ipv4Addr::from(masked)
}

fn apply_v6_mask(addr: Ipv6Addr, cidr: u8) -> Ipv6Addr {
    let raw = u128::from(addr);
    let masked = if cidr == 0 {
        0
    } else {
        let shift = 128u32.saturating_sub(u32::from(cidr));
        raw & (u128::MAX << shift)
    };
    Ipv6Addr::from(masked)
}

fn max_bits(addr: IpAddr) -> u8 {
    match addr {
        IpAddr::V4(_) => 32,
        IpAddr::V6(_) => 128,
    }
}

fn host_increment(addr: IpAddr, cidr: u8) -> u128 {
    let host_bits = u32::from(max_bits(addr) - cidr);
    if host_bits == 0 {
        0
    } else {
        1u128 << host_bits
    }
}

fn add_offset(addr: IpAddr, offset: u128) -> IpAddr {
    match addr {
        IpAddr::V4(addr) => {
            let next = u32::from(addr).saturating_add(offset as u32);
            IpAddr::V4(Ipv4Addr::from(next))
        }
        IpAddr::V6(addr) => {
            let next = u128::from(addr).saturating_add(offset);
            IpAddr::V6(Ipv6Addr::from(next))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn parse_many(input: &str) -> Vec<AllowedIp> {
        parse_tool_prefixes(input).unwrap()
    }

    fn prefixes(values: &[&str]) -> Vec<AllowedIp> {
        values.iter().map(|value| value.parse().unwrap()).collect()
    }

    #[test]
    fn parse_tool_prefixes_accepts_single_ips_and_assignments() {
        let parsed = parse_many("AllowedIPs = 10.0.0.1, 10.0.1.0/24\n2001:db8::1\n\n10.0.2.0/24");

        assert_eq!(
            parsed,
            vec![
                AllowedIp {
                    addr: "10.0.0.1".parse().unwrap(),
                    cidr: 32,
                },
                "10.0.1.0/24".parse().unwrap(),
                AllowedIp {
                    addr: "2001:db8::1".parse().unwrap(),
                    cidr: 128,
                },
                "10.0.2.0/24".parse().unwrap(),
            ]
        );
    }

    #[test]
    fn normalize_prefixes_canonicalizes_and_merges_siblings() {
        let (normalized, stats) = normalize_prefixes(&parse_many("10.0.0.1/25,10.0.0.129/25"));

        assert_eq!(normalized, prefixes(&["10.0.0.0/24"]));
        assert!(stats.host_bits_normalized);
        assert!(stats.merged_prefixes);
    }

    #[test]
    fn compute_cidr_exclusion_keeps_disjoint_prefixes() {
        let result =
            compute_cidr_exclusion(&prefixes(&["10.0.0.0/24"]), &prefixes(&["10.1.0.0/16"]), 32)
                .unwrap();

        assert_eq!(result.remaining, prefixes(&["10.0.0.0/24"]));
    }

    #[test]
    fn compute_cidr_exclusion_drops_fully_covered_prefix() {
        let result =
            compute_cidr_exclusion(&prefixes(&["10.0.0.0/24"]), &prefixes(&["10.0.0.0/16"]), 32)
                .unwrap();

        assert!(result.remaining.is_empty());
    }

    #[test]
    fn compute_cidr_exclusion_splits_partial_overlap() {
        let result = compute_cidr_exclusion(
            &prefixes(&["10.0.0.0/24"]),
            &prefixes(&["10.0.0.64/26"]),
            32,
        )
        .unwrap();

        assert_eq!(
            result.remaining,
            prefixes(&["10.0.0.0/26", "10.0.0.128/25"])
        );
    }

    #[test]
    fn compute_cidr_exclusion_handles_multiple_includes_and_excludes() {
        let result = compute_cidr_exclusion(
            &prefixes(&["10.0.0.0/24", "10.0.1.0/24"]),
            &prefixes(&["10.0.0.128/25", "10.0.1.0/25"]),
            32,
        )
        .unwrap();

        assert_eq!(
            result.remaining,
            prefixes(&["10.0.0.0/25", "10.0.1.128/25"])
        );
    }

    #[test]
    fn compute_cidr_exclusion_separates_ipv4_and_ipv6() {
        let result = compute_cidr_exclusion(
            &prefixes(&["10.0.0.0/24", "2001:db8::/126"]),
            &prefixes(&["2001:db8::2/127"]),
            32,
        )
        .unwrap();

        assert_eq!(
            result.remaining,
            prefixes(&["10.0.0.0/24", "2001:db8::/127"])
        );
    }

    #[test]
    fn compute_cidr_exclusion_reports_limit_overflow() {
        let err = compute_cidr_exclusion(&prefixes(&["0.0.0.0/0"]), &prefixes(&["10.0.0.0/8"]), 7)
            .unwrap_err();

        assert_eq!(
            err,
            ToolError::TooManyResults {
                limit: 7,
                produced: 8,
            }
        );
    }

    #[test]
    fn compute_cidr_exclusion_reports_stats() {
        let result = compute_cidr_exclusion(
            &parse_many("10.0.0.1/25,10.0.0.129/25"),
            &parse_many("10.0.0.64/26"),
            32,
        )
        .unwrap();

        assert_eq!(result.stats.input_include_count, 2);
        assert_eq!(result.stats.input_exclude_count, 1);
        assert_eq!(result.stats.normalized_include_count, 1);
        assert_eq!(result.stats.normalized_exclude_count, 1);
        assert_eq!(result.stats.output_count, 2);
        assert!(result.stats.host_bits_normalized);
        assert!(result.stats.merged_prefixes);
    }

    #[test]
    fn compute_cidr_exclusion_allows_empty_inputs() {
        let result = compute_cidr_exclusion(&[], &[], 16).unwrap();

        assert!(result.normalized_includes.is_empty());
        assert!(result.normalized_excludes.is_empty());
        assert!(result.remaining.is_empty());
        assert_eq!(result.stats.output_count, 0);
    }

    #[test]
    fn normalize_prefixes_removes_duplicates_after_canonicalization() {
        let (normalized, _) = normalize_prefixes(&parse_many("10.0.0.1/24,10.0.0.2/24"));

        assert_eq!(normalized, prefixes(&["10.0.0.0/24"]));
    }

    #[test]
    fn normalize_prefixes_removes_covered_children() {
        let (normalized, _) =
            normalize_prefixes(&prefixes(&["10.0.0.0/16", "10.0.1.0/24", "10.0.2.0/24"]));

        assert_eq!(normalized, prefixes(&["10.0.0.0/16"]));
    }

    #[test]
    fn normalize_prefixes_handles_ipv6_merges() {
        let (normalized, stats) =
            normalize_prefixes(&prefixes(&["2001:db8::/127", "2001:db8::2/127"]));

        assert_eq!(normalized, prefixes(&["2001:db8::/126"]));
        assert!(stats.merged_prefixes);
    }

    #[test]
    fn normalize_cidr_set_returns_explicit_normalization_result() {
        let result = normalize_cidr_set(&parse_many("10.0.0.1/25,10.0.0.129/25"), 32).unwrap();

        assert_eq!(result.normalized, prefixes(&["10.0.0.0/24"]));
        assert_eq!(result.stats.input_count, 2);
        assert_eq!(result.stats.normalized_count, 1);
        assert!(result.stats.host_bits_normalized);
        assert!(result.stats.merged_prefixes);
    }

    #[test]
    fn normalize_cidr_set_reports_limit_overflow() {
        let err = normalize_cidr_set(&prefixes(&["10.0.0.0/25", "10.0.0.128/25"]), 0).unwrap_err();

        assert_eq!(
            err,
            ToolError::TooManyResults {
                limit: 0,
                produced: 1,
            }
        );
    }

    #[test]
    fn parse_tool_prefixes_rejects_invalid_tokens() {
        let err = parse_tool_prefixes("not-an-ip").unwrap_err();

        assert_eq!(
            err,
            ToolError::ParseToken {
                token: "not-an-ip".to_string(),
                message: "invalid ip".to_string(),
            }
        );
    }

    #[test]
    fn normalize_prefixes_preserves_unique_family_order() {
        let (normalized, _) = normalize_prefixes(&prefixes(&[
            "2001:db8::/126",
            "10.0.0.0/24",
            "2001:db8::8/125",
        ]));

        assert_eq!(
            normalized,
            prefixes(&["10.0.0.0/24", "2001:db8::/126", "2001:db8::8/125"])
        );
    }

    #[test]
    fn compute_cidr_exclusion_subtracts_single_host() {
        let result =
            compute_cidr_exclusion(&prefixes(&["10.0.0.0/30"]), &prefixes(&["10.0.0.1/32"]), 32)
                .unwrap();

        assert_eq!(result.remaining, prefixes(&["10.0.0.0/32", "10.0.0.2/31"]));
    }

    #[test]
    fn normalize_prefixes_skips_duplicate_exact_entries() {
        let (normalized, _) = normalize_prefixes(&prefixes(&["10.0.0.0/24", "10.0.0.0/24"]));

        assert_eq!(normalized, prefixes(&["10.0.0.0/24"]));
    }

    #[test]
    fn normalize_prefixes_detects_no_merge_for_non_siblings() {
        let (normalized, stats) = normalize_prefixes(&prefixes(&["10.0.0.0/25", "10.0.1.0/25"]));

        assert_eq!(normalized, prefixes(&["10.0.0.0/25", "10.0.1.0/25"]));
        assert!(!stats.merged_prefixes);
    }

    #[test]
    fn compute_cidr_exclusion_tracks_output_normalization() {
        let result = compute_cidr_exclusion(
            &prefixes(&["10.0.0.0/24"]),
            &prefixes(&["10.0.0.64/27", "10.0.0.96/27"]),
            32,
        )
        .unwrap();

        assert_eq!(
            result.remaining,
            prefixes(&["10.0.0.0/26", "10.0.0.128/25"])
        );
        assert!(result.stats.merged_prefixes);
    }

    #[test]
    fn normalize_prefixes_handles_ipv6_host_bits() {
        let (normalized, stats) = normalize_prefixes(&prefixes(&["2001:db8::1/126"]));

        assert_eq!(normalized, prefixes(&["2001:db8::/126"]));
        assert!(stats.host_bits_normalized);
    }

    #[test]
    fn parse_tool_prefixes_ignores_blank_lines_and_commas() {
        let parsed = parse_many("\n10.0.0.0/24,,\n,\n10.0.1.0/24\n");

        assert_eq!(parsed, prefixes(&["10.0.0.0/24", "10.0.1.0/24"]));
    }

    #[test]
    fn normalize_prefixes_uses_stable_dedup_for_large_sets() {
        let mut items = prefixes(&["10.0.0.0/24"]);
        items.extend(prefixes(&["10.0.0.0/24", "10.0.0.1/24"]));
        let (normalized, _) = normalize_prefixes(&items);

        assert_eq!(normalized, prefixes(&["10.0.0.0/24"]));
    }

    #[test]
    fn compute_cidr_exclusion_can_remove_ipv6_host() {
        let result = compute_cidr_exclusion(
            &prefixes(&["2001:db8::/126"]),
            &prefixes(&["2001:db8::1/128"]),
            32,
        )
        .unwrap();

        assert_eq!(
            result.remaining,
            prefixes(&["2001:db8::/128", "2001:db8::2/127"])
        );
    }

    #[test]
    fn normalize_prefixes_handles_empty_slice() {
        let (normalized, stats) = normalize_prefixes(&[]);

        assert!(normalized.is_empty());
        assert_eq!(stats.count, 0);
    }

    #[test]
    fn normalize_prefixes_covers_multiple_levels_of_merges() {
        let (normalized, stats) = normalize_prefixes(&prefixes(&[
            "10.0.0.0/26",
            "10.0.0.64/26",
            "10.0.0.128/26",
            "10.0.0.192/26",
        ]));

        assert_eq!(normalized, prefixes(&["10.0.0.0/24"]));
        assert!(stats.merged_prefixes);
    }

    #[test]
    fn normalize_prefixes_does_not_cross_ip_families() {
        let (normalized, _) = normalize_prefixes(&prefixes(&[
            "0.0.0.0/0",
            "::/0",
            "10.0.0.0/24",
            "2001:db8::/32",
        ]));

        assert_eq!(normalized, prefixes(&["0.0.0.0/0", "::/0"]));
    }

    #[test]
    fn normalize_prefixes_detects_covered_ipv6_child() {
        let (normalized, _) = normalize_prefixes(&prefixes(&["2001:db8::/32", "2001:db8:1::/48"]));

        assert_eq!(normalized, prefixes(&["2001:db8::/32"]));
    }

    #[test]
    fn compute_cidr_exclusion_handles_ipv6_partial_overlap() {
        let result = compute_cidr_exclusion(
            &prefixes(&["2001:db8::/126"]),
            &prefixes(&["2001:db8::2/127"]),
            32,
        )
        .unwrap();

        assert_eq!(result.remaining, prefixes(&["2001:db8::/127"]));
    }

    #[test]
    fn normalize_prefixes_uses_family_specific_masks() {
        let left = AllowedIp {
            addr: "255.255.255.255".parse().unwrap(),
            cidr: 1,
        };
        let right = AllowedIp {
            addr: "ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff".parse().unwrap(),
            cidr: 1,
        };

        assert_eq!(canonicalize(&left), prefixes(&["128.0.0.0/1"])[0]);
        assert_eq!(canonicalize(&right), prefixes(&["8000::/1"])[0]);
    }

    #[test]
    fn normalize_prefixes_deduplicates_same_ipv6_parent_after_merge() {
        let (normalized, _) = normalize_prefixes(&prefixes(&[
            "2001:db8::/127",
            "2001:db8::2/127",
            "2001:db8::/126",
        ]));

        assert_eq!(normalized, prefixes(&["2001:db8::/126"]));
    }

    #[test]
    fn compute_cidr_exclusion_keeps_stats_for_empty_excludes() {
        let result = compute_cidr_exclusion(&prefixes(&["10.0.0.0/24"]), &[], 32).unwrap();

        assert_eq!(result.stats.normalized_include_count, 1);
        assert_eq!(result.stats.normalized_exclude_count, 0);
        assert_eq!(result.stats.output_count, 1);
    }

    #[test]
    fn normalize_prefixes_handles_hashable_duplicates() {
        let parsed = parse_many("10.0.0.0/24\n10.0.0.0/24\n10.0.0.0/25\n10.0.0.128/25");
        let mut uniq = HashSet::new();
        for prefix in &parsed {
            uniq.insert((prefix.addr, prefix.cidr));
        }
        assert_eq!(uniq.len(), 3);
        let (normalized, _) = normalize_prefixes(&parsed);
        assert_eq!(normalized, prefixes(&["10.0.0.0/24"]));
    }
}
