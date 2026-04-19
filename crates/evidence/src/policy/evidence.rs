//! `EvidencePolicy` + `TracePolicy` — the DAL-driven policy derivation.
//!
//! `EvidencePolicy::for_dal` is the single source of truth for the
//! DO-178C Annex A row that corresponds to each DAL: coverage level,
//! independence requirement, and which trace fields are mandatory.
//! `cmd_generate` / `cmd_trace` feed the output into
//! `validate_trace_links_with_policy` and compliance reporting.

use serde::{Deserialize, Serialize};

use super::dal::Dal;

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
                    require_hlr_sys_trace: true,
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
                    require_hlr_sys_trace: true,
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
                    require_hlr_sys_trace: true,
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
                    require_hlr_sys_trace: false,
                },
                require_structural_coverage: false,
                coverage_level: None,
                require_independent_verification: false,
            },
        }
    }
}

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
    /// Require every HLR to trace up to a System Requirement.
    ///
    /// When `true`, an HLR with an empty `traces_to` vector is a
    /// Link-phase error. Defaults to `false` so external projects
    /// without a SYS layer keep validating cleanly; turn on via the
    /// `--require-hlr-sys-trace` CLI flag on
    /// `cargo evidence trace --validate` or via `[trace]` in
    /// `boundary.toml` once that surface lands.
    #[serde(default)]
    pub require_hlr_sys_trace: bool,
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
