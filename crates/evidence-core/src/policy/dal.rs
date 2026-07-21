//! `Dal` — DO-178C Design Assurance Level + per-crate `DalConfig`.
//!
//! `Dal` deliberately has no `Default` impl: an assurance level is a
//! claim, and a missing `[dal]` section in `boundary.toml` must never
//! silently *become* one. Development surfaces construct
//! [`crate::policy::AssuranceSelection::unclassified`] instead — the same
//! least-strict policy row, named honestly. The `#[derive(Ord)]`
//! sort order (D < C < B < A) means later variants are *more*
//! stringent, so `max()` over a `dal_map` gives the highest required
//! rigor.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::diagnostic::{DiagnosticCode, Severity};

/// Design Assurance Level per DO-178C.
/// A is most stringent, D is least. No `Default`: selecting a DAL is
/// always an explicit act (see module rustdoc).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Dal {
    /// Lowest-rigor DO-178C level.
    D,
    /// DO-178C Level C.
    C,
    /// DO-178C Level B.
    B,
    /// Highest-rigor DO-178C level — most objectives required with
    /// independence.
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

/// Error type for parsing a [`Dal`] from a string.
#[derive(Debug, thiserror::Error)]
pub enum ParseDalError {
    /// Input didn't match any of `A` / `B` / `C` / `D` (case-insensitive).
    #[error("unknown DAL '{0}'; expected one of: A, B, C, D")]
    Unknown(String),
}

impl DiagnosticCode for ParseDalError {
    fn code(&self) -> &'static str {
        match self {
            ParseDalError::Unknown(_) => "POLICY_UNKNOWN_DAL",
        }
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }
}

impl std::str::FromStr for Dal {
    type Err = ParseDalError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "A" => Ok(Dal::A),
            "B" => Ok(Dal::B),
            "C" => Ok(Dal::C),
            "D" => Ok(Dal::D),
            _ => Err(ParseDalError::Unknown(s.to_string())),
        }
    }
}

/// Engineering quality-gate percentages per DO-178C Annex A
/// Table A-7 dimension (statement / branch). A `None` means the
/// dimension is not gated at this DAL.
///
/// These are **engineering gates used by this tool, NOT
/// DO-178C-mandated acceptance thresholds**. The standard's
/// structural-coverage objectives are objective-driven: uncovered
/// structure requires analysis and disposition, and no percentage
/// alone closes an objective (FAA AC 20-115D / DO-178C A-7).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DalCoverageThresholds {
    /// Statement-coverage engineering gate (Obj-5 dimension).
    /// Gated at DAL ≥ C; absent at D.
    pub statement_percent: Option<u8>,
    /// Branch-coverage engineering gate (Obj-6 dimension — LLVM
    /// branch coverage, an approximation of decision coverage, not
    /// MC/DC). Gated at DAL ≥ B; absent at C and D.
    pub branch_percent: Option<u8>,
}

impl Dal {
    /// Engineering quality gates for this DAL, used by this tool to
    /// refuse a cert/record bundle whose structural coverage falls
    /// below an agreed bar. **Not DO-178C-mandated acceptance
    /// thresholds**: meeting the gate never closes an A-7 objective
    /// by itself — uncovered structure still requires documented
    /// analysis/disposition, and approximate (LLVM branch) evidence
    /// caps at manual review (see `compliance::coverage_verdict`,
    /// LLR-108).
    ///
    /// A downstream project can override via
    /// `cert/boundary.toml.[dal.coverage]` in a future schema
    /// extension; today the gates are the single source of truth.
    ///
    /// - D: no gates (info-only).
    /// - C: statement ≥ 90% (Obj-5 dimension).
    /// - B: statement ≥ 95% + branch ≥ 85% (Obj-5 + 6 dimensions).
    /// - A: statement ≥ 95% + branch ≥ 90% (Obj-5 + 6 dimensions,
    ///   plus MC/DC handled by an auxiliary tool — see
    ///   `cert/QUALIFICATION.md`).
    pub fn coverage_thresholds(self) -> DalCoverageThresholds {
        match self {
            Dal::D => DalCoverageThresholds {
                statement_percent: None,
                branch_percent: None,
            },
            Dal::C => DalCoverageThresholds {
                statement_percent: Some(90),
                branch_percent: None,
            },
            Dal::B => DalCoverageThresholds {
                statement_percent: Some(95),
                branch_percent: Some(85),
            },
            Dal::A => DalCoverageThresholds {
                statement_percent: Some(95),
                branch_percent: Some(90),
            },
        }
    }
}

/// DAL configuration section in boundary.toml.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DalConfig {
    /// Default DAL for all in-scope crates without explicit override.
    /// Absent ⇒ no project-wide DAL is claimed; crates resolve to
    /// `unclassified` unless individually overridden, and cert/record
    /// evaluation fails closed (LLR-109).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_dal: Option<Dal>,
    /// Per-crate DAL overrides. Key is crate name.
    #[serde(default)]
    pub crate_overrides: BTreeMap<String, Dal>,
    /// Reference to an auxiliary qualified MC/DC tool whose evidence
    /// the project records by reference. Required at DAL-A (DO-178C
    /// Table A-7 Obj-7) because stable Rust cannot currently emit
    /// MC/DC instrumentation — the unstable `-Zcoverage-options=mcdc`
    /// flag was removed by rust-lang/rust#144999 (merged 2025-08-08)
    /// and tracking issue rust-lang/rust#124144 has no active
    /// reimplementation.
    ///
    /// Absent ⇒ this project produces no MC/DC evidence in-band.
    /// Present ⇒ the project asserts MC/DC is satisfied via the
    /// named auxiliary tool (LDRA, VectorCAST, Rapita RVS, etc.).
    /// The tool's qualification ID and report path live in the
    /// nested struct so an auditor can cross-reference both at
    /// review time. Free-form `name` is a reviewer-readable label
    /// (e.g. `"LDRA TBvision"`); `report` is the bundle-relative
    /// path the auxiliary report is recorded under (the bundle
    /// pipeline does not validate the file's content, only its
    /// presence + hash). See HLR-066 / LLR-073.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auxiliary_mcdc_tool: Option<AuxiliaryMcdcTool>,
}

/// Reference to an external qualified MC/DC tool whose evidence is
/// recorded by reference rather than measured in-band. See
/// [`DalConfig::auxiliary_mcdc_tool`].
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct AuxiliaryMcdcTool {
    /// Reviewer-readable label, e.g. `"LDRA TBvision"`.
    pub name: String,
    /// Tool qualification ID assigned by the auxiliary vendor /
    /// project. Free-form so projects can fold in their own
    /// internal tracking ID. Absent ⇒ this is treated as an
    /// undocumented reference and the auditor must resolve it
    /// out-of-band.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qualification_id: Option<String>,
    /// Bundle-relative path the auxiliary report is recorded
    /// under. Absent today ⇒ the project asserts MC/DC was
    /// measured externally but does not bind a specific report
    /// into the bundle. A future schema extension may make this
    /// required when DAL-A is in scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<String>,
}

impl Default for DalConfig {
    /// No claimed level by default: `default_dal` is `None`
    /// (unclassified), never a silent DAL-D.
    fn default() -> Self {
        Self {
            default_dal: None,
            crate_overrides: BTreeMap::new(),
            auxiliary_mcdc_tool: None,
        }
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
    fn test_dal_has_no_implicit_default() {
        // The Default derive was removed: an assurance level is a
        // claim, never a fallback. Development mode constructs
        // `AssuranceSelection::unclassified()` instead (LLR-109).
        let config = DalConfig::default();
        assert_eq!(config.default_dal, None);
        assert!(config.crate_overrides.is_empty());
    }

    #[test]
    fn test_dal_config_explicit_default_round_trips() {
        let config: DalConfig = toml::from_str(r#"default_dal = "C""#).unwrap();
        assert_eq!(config.default_dal, Some(Dal::C));
        let absent: DalConfig = toml::from_str("").unwrap();
        assert_eq!(absent.default_dal, None);
    }

    /// Pins the engineering-gate percentages per DAL (the values are
    /// the tool's quality gates, not DO-178C acceptance thresholds —
    /// see `coverage_thresholds` rustdoc). Changing a number here is
    /// a cert-contract change; bump the `COMPLIANCE` schema version
    /// in the same PR.
    #[test]
    fn coverage_thresholds_by_dal() {
        assert_eq!(
            Dal::D.coverage_thresholds(),
            DalCoverageThresholds {
                statement_percent: None,
                branch_percent: None,
            }
        );
        assert_eq!(
            Dal::C.coverage_thresholds(),
            DalCoverageThresholds {
                statement_percent: Some(90),
                branch_percent: None,
            }
        );
        assert_eq!(
            Dal::B.coverage_thresholds(),
            DalCoverageThresholds {
                statement_percent: Some(95),
                branch_percent: Some(85),
            }
        );
        assert_eq!(
            Dal::A.coverage_thresholds(),
            DalCoverageThresholds {
                statement_percent: Some(95),
                branch_percent: Some(90),
            }
        );
    }
}
