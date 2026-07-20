//! `AssuranceLevel` / `StandardEdition` / `AssuranceSelection` — the
//! explicit assurance claim a named-claim evaluation makes (LLR-109).
//!
//! Cert/record profile evaluation must name the standard edition and
//! the assurance level it evaluates against. A missing selection
//! fails closed via [`AssuranceSelection::require_for_named_claim`];
//! development-mode surfaces construct
//! [`AssuranceSelection::unclassified`] explicitly so a missing
//! `boundary.toml` `[dal]` section can never silently weaken an
//! intended DAL-A/B/C claim into the least stringent level.

use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::boundary::BoundaryConfig;
use super::dal::Dal;
use crate::diagnostic::{DiagnosticCode, Severity};

/// Assurance level an evaluation targets. `DalA`–`DalD` are DO-178C
/// Design Assurance Levels; `Qm` names a quality-management posture
/// with no DAL objectives claimed; `Unclassified` names development
/// mode — no assurance claim at all.
///
/// Ordering is by policy strictness (`Unclassified` least, `DalA`
/// most) so `max()` over a per-crate map yields the highest rigor in
/// scope. Wire strings are snake_case (`"dal_a"`, `"qm"`,
/// `"unclassified"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssuranceLevel {
    /// Development mode: no assurance claim. Maps to the
    /// least-strict policy row (the former DAL-D flag set) but is
    /// named `unclassified` in every diagnostic and report.
    Unclassified,
    /// Quality-management posture: no DAL objectives claimed.
    Qm,
    /// DO-178C Level D.
    DalD,
    /// DO-178C Level C.
    DalC,
    /// DO-178C Level B.
    DalB,
    /// DO-178C Level A — most stringent.
    DalA,
}

impl AssuranceLevel {
    /// Snake_case wire string, matching the serde representation.
    pub fn as_str(self) -> &'static str {
        match self {
            AssuranceLevel::Unclassified => "unclassified",
            AssuranceLevel::Qm => "qm",
            AssuranceLevel::DalD => "dal_d",
            AssuranceLevel::DalC => "dal_c",
            AssuranceLevel::DalB => "dal_b",
            AssuranceLevel::DalA => "dal_a",
        }
    }

    /// Lift a [`Dal`] into the matching assurance level.
    pub fn from_dal(dal: Dal) -> Self {
        match dal {
            Dal::A => AssuranceLevel::DalA,
            Dal::B => AssuranceLevel::DalB,
            Dal::C => AssuranceLevel::DalC,
            Dal::D => AssuranceLevel::DalD,
        }
    }

    /// The [`Dal`] this level claims, if any. `Qm` and
    /// `Unclassified` claim no DAL.
    pub fn as_dal(self) -> Option<Dal> {
        match self {
            AssuranceLevel::DalA => Some(Dal::A),
            AssuranceLevel::DalB => Some(Dal::B),
            AssuranceLevel::DalC => Some(Dal::C),
            AssuranceLevel::DalD => Some(Dal::D),
            AssuranceLevel::Qm | AssuranceLevel::Unclassified => None,
        }
    }

    /// Policy input for levels that claim no DAL: the least-strict
    /// row, identical to today's DAL-D flag set. This is an internal
    /// policy derivation, never a DAL claim — diagnostics and reports
    /// name the level itself (`unclassified` / `qm`).
    pub fn effective_policy_dal(self) -> Dal {
        self.as_dal().unwrap_or(Dal::D)
    }
}

impl std::fmt::Display for AssuranceLevel {
    /// Bundle-facing label: the DAL letter for DAL levels so
    /// `index.json` `dal_map` values stay backward compatible, `QM`,
    /// or `unclassified`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AssuranceLevel::DalA => write!(f, "A"),
            AssuranceLevel::DalB => write!(f, "B"),
            AssuranceLevel::DalC => write!(f, "C"),
            AssuranceLevel::DalD => write!(f, "D"),
            AssuranceLevel::Qm => write!(f, "QM"),
            AssuranceLevel::Unclassified => write!(f, "unclassified"),
        }
    }
}

/// Standard edition an evaluation claims. One variant today — kept
/// as an enum so additional standards or editions extend the type
/// rather than proliferating free-form strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StandardEdition {
    /// DO-178C (ED-12C), accepted via FAA AC 20-115D.
    Do178c,
}

impl StandardEdition {
    /// Standard name as recorded in reports (`"DO-178C"`).
    pub fn standard_name(self) -> &'static str {
        match self {
            StandardEdition::Do178c => "DO-178C",
        }
    }

    /// Edition letter as recorded in reports (`"C"`).
    pub fn edition(self) -> &'static str {
        match self {
            StandardEdition::Do178c => "C",
        }
    }
}

/// An explicit assurance selection: the standard edition plus the
/// assurance level a named-claim evaluation targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssuranceSelection {
    /// Standard edition the evaluation claims.
    pub standard: StandardEdition,
    /// Assurance level the evaluation targets.
    pub level: AssuranceLevel,
}

impl AssuranceSelection {
    /// Development-mode selection: no assurance claim. Constructs
    /// the unclassified level explicitly so no call site reaches for
    /// a silent DAL-D/QM fallback.
    pub fn unclassified() -> Self {
        Self {
            standard: StandardEdition::Do178c,
            level: AssuranceLevel::Unclassified,
        }
    }

    /// Resolve the selection a cert/record (named-claim) evaluation
    /// requires, failing closed when it is absent. The boundary file
    /// must load, declare a `[dal]` section with an explicit
    /// `default_dal`, and name a non-empty `scope.in_scope`; any gap
    /// yields [`AssuranceSelectionError::Missing`]
    /// (`POLICY_ASSURANCE_SELECTION_MISSING`).
    pub fn require_for_named_claim(boundary_path: &Path) -> Result<Self, AssuranceSelectionError> {
        let config =
            BoundaryConfig::load(boundary_path).map_err(|e| AssuranceSelectionError::Missing {
                reason: format!(
                    "boundary config at {} is not loadable: {e}",
                    boundary_path.display()
                ),
            })?;
        config
            .assurance_selection()
            .ok_or_else(|| AssuranceSelectionError::Missing {
                reason: format!(
                    "{} must declare a [dal] section with an explicit default_dal \
                     and a non-empty scope.in_scope for cert/record evaluation; \
                     development mode constructs AssuranceSelection::unclassified() \
                     instead of silently assuming a level",
                    boundary_path.display()
                ),
            })
    }
}

/// Errors returned by [`AssuranceSelection::require_for_named_claim`].
#[derive(Debug, Error)]
pub enum AssuranceSelectionError {
    /// The explicit selection a named-claim evaluation requires is
    /// absent (missing/unloadable boundary, absent `[dal]`, absent
    /// `default_dal`, or empty in-scope set).
    #[error("explicit assurance selection required for cert/record evaluation: {reason}")]
    Missing {
        /// Which precondition failed.
        reason: String,
    },
}

impl DiagnosticCode for AssuranceSelectionError {
    fn code(&self) -> &'static str {
        match self {
            AssuranceSelectionError::Missing { .. } => "POLICY_ASSURANCE_SELECTION_MISSING",
        }
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }
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
    use crate::policy::EvidencePolicy;

    /// Unclassified must map to exactly the DAL-D policy flag set —
    /// the least-strict row — while never being NAMED as a DAL.
    #[test]
    fn unclassified_maps_to_least_strict_policy_row() {
        let unclassified =
            EvidencePolicy::for_dal(AssuranceLevel::Unclassified.effective_policy_dal());
        let dal_d = EvidencePolicy::for_dal(Dal::D);
        assert_eq!(
            unclassified.require_structural_coverage,
            dal_d.require_structural_coverage
        );
        assert_eq!(
            unclassified.require_independent_verification,
            dal_d.require_independent_verification
        );
        assert_eq!(unclassified.coverage_level, dal_d.coverage_level);
        let u_trace = unclassified.trace;
        let d_trace = dal_d.trace;
        assert_eq!(u_trace.require_uids, d_trace.require_uids);
        assert_eq!(u_trace.require_hlr_sys_trace, d_trace.require_hlr_sys_trace);
        assert_eq!(
            u_trace.require_derived_rationale,
            d_trace.require_derived_rationale
        );
        // …but the level itself is never a DAL.
        assert_eq!(AssuranceLevel::Unclassified.as_dal(), None);
        assert_eq!(AssuranceLevel::Unclassified.to_string(), "unclassified");
    }

    #[test]
    fn dal_round_trip_preserves_levels() {
        for dal in [Dal::A, Dal::B, Dal::C, Dal::D] {
            let level = AssuranceLevel::from_dal(dal);
            assert_eq!(level.as_dal(), Some(dal));
            assert_eq!(level.effective_policy_dal(), dal);
            // Bundle-facing label stays the DAL letter for DAL levels.
            assert_eq!(level.to_string(), dal.to_string());
        }
        assert_eq!(AssuranceLevel::Qm.as_dal(), None);
        assert_eq!(AssuranceLevel::Qm.to_string(), "QM");
    }

    /// Strictness ordering: unclassified is least, DAL-A most, so
    /// `max()` over a per-crate map yields the highest rigor in scope.
    #[test]
    fn ordering_is_by_policy_strictness() {
        assert!(AssuranceLevel::Unclassified < AssuranceLevel::Qm);
        assert!(AssuranceLevel::Qm < AssuranceLevel::DalD);
        assert!(AssuranceLevel::DalD < AssuranceLevel::DalC);
        assert!(AssuranceLevel::DalC < AssuranceLevel::DalB);
        assert!(AssuranceLevel::DalB < AssuranceLevel::DalA);
    }

    /// The `as_str` labels are the wire contract (report
    /// `assurance_level` field); they must match the serde
    /// representation exactly.
    #[test]
    fn wire_strings_are_snake_case() {
        let cases = [
            (AssuranceLevel::Unclassified, "unclassified"),
            (AssuranceLevel::Qm, "qm"),
            (AssuranceLevel::DalD, "dal_d"),
            (AssuranceLevel::DalC, "dal_c"),
            (AssuranceLevel::DalB, "dal_b"),
            (AssuranceLevel::DalA, "dal_a"),
        ];
        for (level, expected) in cases {
            assert_eq!(level.as_str(), expected);
            let serde_wire = serde_json::to_string(&level).unwrap();
            assert_eq!(serde_wire, format!("\"{expected}\""));
            let round: AssuranceLevel = serde_json::from_str(&serde_wire).unwrap();
            assert_eq!(round, level);
        }
    }

    #[test]
    fn require_for_named_claim_missing_file_fails_closed() {
        let tmp = tempfile::TempDir::new().unwrap();
        let err = AssuranceSelection::require_for_named_claim(
            &tmp.path().join("cert").join("boundary.toml"),
        )
        .unwrap_err();
        assert_eq!(err.code(), "POLICY_ASSURANCE_SELECTION_MISSING");
        assert_eq!(err.severity(), Severity::Error);
    }
}
