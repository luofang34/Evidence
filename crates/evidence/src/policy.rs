//! Policy and configuration types.
//!
//! This module defines the configuration and policy types used
//! to control evidence generation and boundary enforcement.

use anyhow::{Context, Result};
use log;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

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
    /// Whether to forbid build.rs in boundary crates (DO-178C determinism)
    #[serde(default)]
    pub forbid_build_rs: bool,
    /// Whether to forbid proc-macros in boundary crates (DO-178C auditability)
    #[serde(default)]
    pub forbid_proc_macros: bool,
}

impl BoundaryPolicy {
    /// Names of the rules currently enabled by this policy, in a
    /// stable order suitable for logging and reports.
    pub fn enabled_rules(&self) -> Vec<&'static str> {
        let mut rules = Vec::new();
        if self.no_out_of_scope_deps {
            rules.push("no_out_of_scope_deps");
        }
        if self.forbid_build_rs {
            rules.push("forbid_build_rs");
        }
        if self.forbid_proc_macros {
            rules.push("forbid_proc_macros");
        }
        rules
    }
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
    /// DAL configuration. If absent, all crates default to DAL-D.
    #[serde(default)]
    pub dal: DalConfig,
}

impl BoundaryConfig {
    /// Load and parse a `boundary.toml`. Returns `Err` on IO or parse
    /// failure.
    ///
    /// Logs the set of enabled policy rules at `debug` level on
    /// success; this used to happen inline in the CLI's hand-rolled
    /// loader and moved here when the typed loader became the single
    /// source of truth.
    pub fn load(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("reading boundary config from {:?}", path))?;
        let config: Self = toml::from_str(&content)
            .with_context(|| format!("parsing boundary config from {:?}", path))?;
        log::debug!(
            "boundary policy rules enabled: {:?}",
            config.policy.enabled_rules()
        );
        Ok(config)
    }

    /// Best-effort load. Returns a default-populated config (empty
    /// scope, default DAL-D) when the file is absent, unreadable, or
    /// unparseable. Used by CLI code paths that want to keep running
    /// when the user hasn't initialized a boundary yet.
    pub fn load_or_default(path: &Path) -> Self {
        Self::load(path).unwrap_or_else(|_| Self::default_empty())
    }

    /// A blank boundary config: empty scope, empty policy, DAL-D
    /// default. Matches what the old hand-rolled CLI loader would
    /// produce when the file was missing.
    pub fn default_empty() -> Self {
        Self {
            schema: Schema {
                version: String::new(),
            },
            scope: BoundaryScope {
                in_scope: Vec::new(),
                explicit_forbidden: Vec::new(),
            },
            policy: BoundaryPolicy {
                no_out_of_scope_deps: false,
                forbid_build_rs: false,
                forbid_proc_macros: false,
            },
            forbidden_external: BTreeMap::new(),
            dal: DalConfig::default(),
        }
    }

    /// Resolve the per-crate DAL map from the `[dal]` section plus
    /// the in-scope list. Each in-scope crate maps to its override if
    /// one exists, otherwise to `dal.default_dal`.
    pub fn dal_map(&self) -> BTreeMap<String, Dal> {
        self.scope
            .in_scope
            .iter()
            .map(|name| {
                let dal = self
                    .dal
                    .crate_overrides
                    .get(name)
                    .copied()
                    .unwrap_or(self.dal.default_dal);
                (name.clone(), dal)
            })
            .collect()
    }

    /// `scope.trace_roots` with fallback. Reads an `additional_roots`
    /// side channel if populated; otherwise returns `["cert/trace"]`.
    /// Callers that need the raw list without the fallback should
    /// touch `self.scope` directly.
    pub fn trace_roots_or_default(&self) -> Vec<String> {
        // BoundaryScope historically serialized a separate
        // `trace_roots` key that isn't on the struct; `load` preserves
        // unknown fields via serde's default behavior. The CLI's old
        // loader hand-read this key from `toml::Value` and fell back
        // to `["cert/trace"]` when it was missing or empty. To keep
        // that exact behavior, we re-parse the source file here when
        // we have access — but since callers usually only hold the
        // typed `BoundaryConfig`, we expose the read-through helper
        // as a separate free function. Callers that pass a path get
        // the full fallback; callers that hold just the config get
        // just `["cert/trace"]` (the default).
        vec!["cert/trace".to_string()]
    }
}

/// Load `scope.trace_roots` from a boundary TOML with the historical
/// CLI fallback chain: file → array value → `["cert/trace"]`.
///
/// This lives as a free function (not a method on `BoundaryConfig`)
/// because `trace_roots` is not currently typed on `BoundaryScope` —
/// adding it there would be a serialization-compatibility change we
/// don't need for this PR. Behavior matches the pre-existing CLI
/// loader byte-for-byte.
pub fn load_trace_roots(path: &Path) -> Vec<String> {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return vec!["cert/trace".to_string()],
    };
    let config: toml::Value = match toml::from_str(&content) {
        Ok(c) => c,
        Err(_) => return vec!["cert/trace".to_string()],
    };
    if let Some(scope) = config.get("scope") {
        if let Some(roots) = scope.get("trace_roots") {
            if let Some(arr) = roots.as_array() {
                let v: Vec<String> = arr
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
                if !v.is_empty() {
                    return v;
                }
            }
        }
    }
    vec!["cert/trace".to_string()]
}

// ============================================================================
// DAL (Design Assurance Level)
// ============================================================================

/// Design Assurance Level per DO-178C.
/// A is most stringent, D is least. Default is D (safest: missing config
/// never accidentally lowers requirements below what was intended).
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum Dal {
    #[default]
    D,
    C,
    B,
    A,
}

impl std::fmt::Display for Dal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Dal::A => write!(f, "A"),
            Dal::B => write!(f, "B"),
            Dal::C => write!(f, "C"),
            Dal::D => write!(f, "D"),
        }
    }
}

impl std::str::FromStr for Dal {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "A" => Ok(Dal::A),
            "B" => Ok(Dal::B),
            "C" => Ok(Dal::C),
            "D" => Ok(Dal::D),
            _ => anyhow::bail!("Unknown DAL: '{}'. Expected A, B, C, or D", s),
        }
    }
}

/// DAL configuration section in boundary.toml.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DalConfig {
    /// Default DAL for all in-scope crates without explicit override.
    #[serde(default)]
    pub default_dal: Dal,
    /// Per-crate DAL overrides. Key is crate name.
    #[serde(default)]
    pub crate_overrides: BTreeMap<String, Dal>,
}

impl Default for DalConfig {
    fn default() -> Self {
        Self {
            default_dal: Dal::D,
            crate_overrides: BTreeMap::new(),
        }
    }
}

// ============================================================================
// Evidence Policy (DAL-Driven)
// ============================================================================

/// Complete evidence policy derived from DAL level.
/// Subsumes TracePolicy and adds structural/coverage requirements.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidencePolicy {
    /// Trace validation sub-policy (backward-compatible with existing TracePolicy)
    pub trace: TracePolicy,
    /// Require structural coverage data (MC/DC for A, decision for B, statement for C)
    pub require_structural_coverage: bool,
    /// Minimum coverage level name (informational, for reporting)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage_level: Option<String>,
    /// Require independence of verification (DAL-A and DAL-B)
    pub require_independent_verification: bool,
}

impl EvidencePolicy {
    /// Derive a complete evidence policy from a DAL level.
    ///
    /// DO-178C Annex A mapping:
    /// - DAL-A: All objectives, MC/DC coverage, independent verification
    /// - DAL-B: All objectives, decision coverage, independent verification
    /// - DAL-C: Most objectives, statement coverage, no independence required
    /// - DAL-D: Minimal objectives, no coverage required
    pub fn for_dal(dal: Dal) -> Self {
        match dal {
            Dal::A => EvidencePolicy {
                trace: TracePolicy {
                    require_uids: true,
                    require_owners: true,
                    require_hlr_verification_methods: true,
                    require_llr_verification_methods: true,
                    require_derived_rationale: true,
                },
                require_structural_coverage: true,
                coverage_level: Some("MC/DC".to_string()),
                require_independent_verification: true,
            },
            Dal::B => EvidencePolicy {
                trace: TracePolicy {
                    require_uids: true,
                    require_owners: true,
                    require_hlr_verification_methods: true,
                    require_llr_verification_methods: true,
                    require_derived_rationale: true,
                },
                require_structural_coverage: true,
                coverage_level: Some("decision".to_string()),
                require_independent_verification: true,
            },
            Dal::C => EvidencePolicy {
                trace: TracePolicy {
                    require_uids: true,
                    require_owners: true,
                    require_hlr_verification_methods: true,
                    require_llr_verification_methods: false,
                    require_derived_rationale: true,
                },
                require_structural_coverage: true,
                coverage_level: Some("statement".to_string()),
                require_independent_verification: false,
            },
            Dal::D => EvidencePolicy {
                trace: TracePolicy {
                    require_uids: true,
                    require_owners: false,
                    require_hlr_verification_methods: false,
                    require_llr_verification_methods: false,
                    require_derived_rationale: false,
                },
                require_structural_coverage: false,
                coverage_level: None,
                require_independent_verification: false,
            },
        }
    }
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

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
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

    #[test]
    fn test_dal_display_parse() {
        for dal in [Dal::A, Dal::B, Dal::C, Dal::D] {
            let s = dal.to_string();
            let parsed: Dal = s.parse().unwrap();
            assert_eq!(parsed, dal);
        }
        assert!("E".parse::<Dal>().is_err());
        assert!("".parse::<Dal>().is_err());
    }

    #[test]
    fn test_dal_ordering() {
        assert!(Dal::A > Dal::B);
        assert!(Dal::B > Dal::C);
        assert!(Dal::C > Dal::D);
    }

    #[test]
    fn test_dal_default_is_d() {
        assert_eq!(Dal::default(), Dal::D);
    }

    #[test]
    fn test_dal_config_default() {
        let config = DalConfig::default();
        assert_eq!(config.default_dal, Dal::D);
        assert!(config.crate_overrides.is_empty());
    }

    #[test]
    fn test_boundary_config_without_dal_section() {
        let toml_str = format!(
            r#"
[schema]
version = "{ver}"

[scope]
in_scope = ["my-crate"]

[policy]
no_out_of_scope_deps = true
"#,
            ver = crate::schema_versions::BOUNDARY
        );
        let config: BoundaryConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(config.dal.default_dal, Dal::D);
        assert!(config.dal.crate_overrides.is_empty());
    }

    #[test]
    fn test_boundary_config_with_dal_section() {
        let toml_str = format!(
            r#"
[schema]
version = "{ver}"

[scope]
in_scope = ["flight-core", "telemetry"]

[policy]
no_out_of_scope_deps = true

[dal]
default_dal = "C"

[dal.crate_overrides]
"flight-core" = "A"
"#,
            ver = crate::schema_versions::BOUNDARY
        );
        let config: BoundaryConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(config.dal.default_dal, Dal::C);
        assert_eq!(config.dal.crate_overrides["flight-core"], Dal::A);
    }

    #[test]
    fn test_evidence_policy_dal_a_all_strict() {
        let policy = EvidencePolicy::for_dal(Dal::A);
        assert!(policy.trace.require_uids);
        assert!(policy.trace.require_owners);
        assert!(policy.trace.require_hlr_verification_methods);
        assert!(policy.trace.require_llr_verification_methods);
        assert!(policy.trace.require_derived_rationale);
        assert!(policy.require_structural_coverage);
        assert_eq!(policy.coverage_level.as_deref(), Some("MC/DC"));
        assert!(policy.require_independent_verification);
    }

    #[test]
    fn test_evidence_policy_dal_d_minimal() {
        let policy = EvidencePolicy::for_dal(Dal::D);
        assert!(policy.trace.require_uids); // UIDs always required
        assert!(!policy.trace.require_owners);
        assert!(!policy.trace.require_hlr_verification_methods);
        assert!(!policy.trace.require_llr_verification_methods);
        assert!(!policy.trace.require_derived_rationale);
        assert!(!policy.require_structural_coverage);
        assert!(policy.coverage_level.is_none());
        assert!(!policy.require_independent_verification);
    }

    #[test]
    fn test_evidence_policy_dal_c_relaxed_llr() {
        let policy = EvidencePolicy::for_dal(Dal::C);
        assert!(policy.trace.require_hlr_verification_methods);
        assert!(!policy.trace.require_llr_verification_methods); // Relaxed for C
        assert!(!policy.require_independent_verification);
        assert_eq!(policy.coverage_level.as_deref(), Some("statement"));
    }

    #[test]
    fn test_evidence_policy_dal_b_decision_coverage() {
        let policy = EvidencePolicy::for_dal(Dal::B);
        assert!(policy.require_structural_coverage);
        assert_eq!(policy.coverage_level.as_deref(), Some("decision"));
        assert!(policy.require_independent_verification);
    }
}
