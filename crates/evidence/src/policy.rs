//! Policy and configuration types.
//!
//! This module defines the configuration and policy types used
//! to control evidence generation and boundary enforcement.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ============================================================================
// Profile Configuration
// ============================================================================

/// Build/certification profile (e.g., dev, cert, record).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    /// Development profile - relaxed checks
    #[default]
    Dev,
    /// Certification profile - strict checks
    Cert,
    /// Recording profile - captures evidence without enforcement
    Record,
}

impl std::fmt::Display for Profile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Profile::Dev => write!(f, "dev"),
            Profile::Cert => write!(f, "cert"),
            Profile::Record => write!(f, "record"),
        }
    }
}

impl std::str::FromStr for Profile {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "dev" => Ok(Profile::Dev),
            "cert" => Ok(Profile::Cert),
            "record" => Ok(Profile::Record),
            _ => anyhow::bail!("Unknown profile: {}", s),
        }
    }
}

// ============================================================================
// Boundary Configuration
// ============================================================================

/// Schema version information.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Schema {
    pub version: String,
}

/// Boundary scope configuration.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BoundaryScope {
    /// Crates that are in scope for certification
    pub in_scope: Vec<String>,
    /// Workspace crates that are explicitly forbidden as dependencies
    #[serde(default)]
    pub explicit_forbidden: Vec<String>,
}

/// Boundary policy rules.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BoundaryPolicy {
    /// Whether to forbid dependencies on out-of-scope workspace crates
    pub no_out_of_scope_deps: bool,
    /// Whether to forbid build.rs in boundary crates (future)
    #[serde(default)]
    #[allow(dead_code)]
    pub forbid_build_rs: bool,
    /// Whether to forbid proc-macros in boundary crates (future)
    #[serde(default)]
    #[allow(dead_code)]
    pub forbid_proc_macros: bool,
}

/// Complete boundary configuration (loaded from boundary.toml).
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BoundaryConfig {
    pub schema: Schema,
    pub scope: BoundaryScope,
    pub policy: BoundaryPolicy,
    /// Forbidden external crates with reasons
    #[serde(default)]
    pub forbidden_external: BTreeMap<String, String>,
}

// ============================================================================
// Trace Policy
// ============================================================================

/// Policy for trace validation and generation.
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct TracePolicy {
    /// Require all items to have UIDs
    #[serde(default = "default_true")]
    pub require_uids: bool,
    /// Require all items to have owners
    #[serde(default = "default_true")]
    pub require_owners: bool,
    /// Require all HLRs to have verification methods
    #[serde(default = "default_true")]
    pub require_hlr_verification_methods: bool,
    /// Require all LLRs to have verification methods
    #[serde(default = "default_true")]
    pub require_llr_verification_methods: bool,
    /// Require derived LLRs to have rationale
    #[serde(default = "default_true")]
    pub require_derived_rationale: bool,
}

fn default_true() -> bool {
    true
}

// ============================================================================
// Profile-Specific Configuration
// ============================================================================

/// Profile metadata.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ProfileMeta {
    pub name: String,
    pub description: String,
}

/// Profile check settings.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ProfileChecks {
    /// Require clean git tree
    pub require_clean_git: bool,
    /// Require coverage collection
    #[serde(default)]
    #[allow(dead_code)]
    pub require_coverage: bool,
    /// Allow --all-features in lint/test
    #[serde(default)]
    pub allow_all_features: bool,
    /// Require offline build
    #[serde(default)]
    #[allow(dead_code)]
    pub offline_required: bool,
}

/// Profile evidence settings.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ProfileEvidence {
    /// Include timestamps in evidence (breaks determinism)
    #[serde(default)]
    #[allow(dead_code)]
    pub include_timestamps: bool,
    /// Strict hash validation mode
    #[serde(default)]
    #[allow(dead_code)]
    pub strict_hash_validation: bool,
    /// Fail if git tree is dirty
    #[serde(default)]
    pub fail_on_dirty: bool,
}

/// Complete profile configuration (loaded from profiles/*.toml).
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ProfileConfig {
    pub profile: ProfileMeta,
    pub checks: ProfileChecks,
    pub evidence: ProfileEvidence,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profile_display() {
        assert_eq!(Profile::Dev.to_string(), "dev");
        assert_eq!(Profile::Cert.to_string(), "cert");
        assert_eq!(Profile::Record.to_string(), "record");
    }

    #[test]
    fn test_profile_parse() {
        assert_eq!("dev".parse::<Profile>().unwrap(), Profile::Dev);
        assert_eq!("cert".parse::<Profile>().unwrap(), Profile::Cert);
        assert_eq!("CERT".parse::<Profile>().unwrap(), Profile::Cert);
        assert!("unknown".parse::<Profile>().is_err());
    }
}
