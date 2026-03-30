mod cidr;
mod reachability;

use std::fmt;

pub use cidr::{
    compute_cidr_exclusion, normalize_cidr_set, parse_tool_prefixes, CidrExclusionResult,
    CidrExclusionStats, CidrNormalizationResult, CidrNormalizationStats,
};
pub use reachability::{
    format_endpoint_display, probe_reachability, probe_reachability_blocking,
    probe_reachability_blocking_until_cancel, probe_reachability_until_cancel,
    AddressFamilyPreference, ReachabilityAttempt, ReachabilityAttemptResult, ReachabilityMode,
    ReachabilityRequest, ReachabilityResult, ReachabilityVerdict,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolError {
    ParseToken { token: String, message: String },
    InvalidTarget(String),
    MissingPort,
    Runtime(String),
    TooManyResults { limit: usize, produced: usize },
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ParseToken { token, message } => {
                write!(f, "parse error for `{token}`: {message}")
            }
            Self::InvalidTarget(message) => write!(f, "{message}"),
            Self::MissingPort => write!(f, "tcp connect requires a port"),
            Self::Runtime(message) => write!(f, "{message}"),
            Self::TooManyResults { limit, produced } => write!(
                f,
                "result exceeds limit of {limit} prefixes (produced {produced} before stopping)"
            ),
        }
    }
}

impl std::error::Error for ToolError {}
