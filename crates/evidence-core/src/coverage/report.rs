//! Typed coverage report — wire shape of `coverage/coverage_summary.json`.
//!
//! See module-level docstring in `coverage.rs` for rationale on
//! the four-level `level` enum + per-file decision/condition vectors.

use serde::{Deserialize, Serialize};

/// Granularity of a coverage measurement. Four variants; only
/// `Statement` and `Branch` are emitted today. `PatternDecision`
/// and `Mcdc` are reserved so the wire contract absorbs a future
/// rustc stabilization of MC/DC without a breaking schema bump.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageLevel {
    /// Line/statement executed at least once under the test suite.
    /// DAL-C minimum (DO-178C A-7 Obj-5).
    Statement,
    /// LLVM branch coverage: every `if` / `match` arm entered.
    /// Approximation of DO-178C decision coverage. DAL-B minimum
    /// (A-7 Obj-6).
    Branch,
    /// Rust pattern-match refutability class coverage per arXiv
    /// 2409.08708 Section 4. Reserved — not emitted until rustc's
    /// MIR/THIR API stabilizes. Documented in
    /// `cert/QUALIFICATION.md`.
    PatternDecision,
    /// Modified Condition/Decision Coverage (DO-178C A-7 Obj-7,
    /// DAL-A). Reserved — rust-lang/rust#124144 is nightly-only;
    /// stable Rust submissions require an auxiliary qualified
    /// tool.
    Mcdc,
}

/// Line / statement coverage counts for a single file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineCoverage {
    /// Lines executed at least once.
    pub covered: u64,
    /// Total executable lines.
    pub total: u64,
}

/// Single branch coverage record (LLVM branch coverage mapped
/// into our decision vocabulary). Each decision corresponds to a
/// branch instruction in the LLVM IR — this is an approximation,
/// not the full DO-178C "decision" semantics. See
/// `cert/QUALIFICATION.md` for the semantic gap statement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionCoverage {
    /// Stable identifier built from `{path}:{line}:{col}`. Used
    /// by downstream tooling to correlate decisions across runs.
    pub id: String,
    /// Whether the decision's both outcomes were exercised.
    pub covered: bool,
    /// Kind of decision — `"branch"` for LLVM branches; future
    /// values include `"match_arm"`, `"if_let"`, `"guard"` once
    /// pattern-decision tracking is added.
    pub kind: String,
}

/// Single condition coverage record. Reserved for MC/DC
/// expansion — empty `Vec` today regardless of level.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConditionCoverage {
    /// Stable identifier for the condition.
    pub id: String,
    /// Decision this condition participates in
    /// ([`DecisionCoverage::id`]).
    pub decision_id: String,
    /// Whether the condition was observed to take both truth
    /// values across the test run.
    pub covered: bool,
}

/// Per-file measurement within a [`Measurement`]. `decisions` and
/// `conditions` are empty for `CoverageLevel::Statement`;
/// populated for `CoverageLevel::Branch`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileMeasurement {
    /// Workspace-relative path (normalized by
    /// [`super::parse_llvm_cov_export`]).
    pub path: String,
    /// Line coverage for this file.
    pub lines: LineCoverage,
    /// Decision records (branch-level only today).
    #[serde(default)]
    pub decisions: Vec<DecisionCoverage>,
    /// Condition records (MC/DC reserved; empty today).
    #[serde(default)]
    pub conditions: Vec<ConditionCoverage>,
}

/// One coverage measurement. Multiple [`Measurement`] entries
/// coexist in a single report — e.g., a `--coverage=all` run
/// emits both `level=statement` and `level=branch` measurements
/// from the same instrumented test pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Measurement {
    /// Granularity of the measurement.
    pub level: CoverageLevel,
    /// Coverage tool name. Today always `"llvm-cov"`; reserved
    /// value `"manual"` for auxiliary evidence in follow-up PRs.
    pub engine: String,
    /// Engine version string — `cargo-llvm-cov` crate version for
    /// LLVM-derived measurements.
    pub engine_version: String,
    /// Per-file breakdown. Files listed alphabetically by `path`
    /// for deterministic serialization.
    pub per_file: Vec<FileMeasurement>,
}

/// Top-level wire shape of `coverage/coverage_summary.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoverageReport {
    /// Pinned to [`crate::schema_versions::COVERAGE`].
    pub schema_version: String,
    /// Measurements in input order (statement before branch by
    /// convention).
    pub measurements: Vec<Measurement>,
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
    fn coverage_level_serializes_to_snake_case() {
        assert_eq!(
            serde_json::to_string(&CoverageLevel::Statement).unwrap(),
            "\"statement\""
        );
        assert_eq!(
            serde_json::to_string(&CoverageLevel::Branch).unwrap(),
            "\"branch\""
        );
        assert_eq!(
            serde_json::to_string(&CoverageLevel::PatternDecision).unwrap(),
            "\"pattern_decision\""
        );
        assert_eq!(
            serde_json::to_string(&CoverageLevel::Mcdc).unwrap(),
            "\"mcdc\""
        );
    }

    #[test]
    fn coverage_report_roundtrips_json() {
        let report = CoverageReport {
            schema_version: crate::schema_versions::COVERAGE.into(),
            measurements: vec![Measurement {
                level: CoverageLevel::Statement,
                engine: "llvm-cov".into(),
                engine_version: "0.8.5".into(),
                per_file: vec![FileMeasurement {
                    path: "crates/evidence-core/src/lib.rs".into(),
                    lines: LineCoverage {
                        covered: 42,
                        total: 100,
                    },
                    decisions: vec![],
                    conditions: vec![],
                }],
            }],
        };
        let json = serde_json::to_string(&report).unwrap();
        let parsed: CoverageReport = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, report);
    }

    #[test]
    fn empty_decisions_and_conditions_serialize_as_empty_arrays() {
        let fm = FileMeasurement {
            path: "a.rs".into(),
            lines: LineCoverage {
                covered: 0,
                total: 1,
            },
            decisions: vec![],
            conditions: vec![],
        };
        let json = serde_json::to_value(&fm).unwrap();
        assert!(json["decisions"].is_array());
        assert!(json["conditions"].is_array());
        assert_eq!(json["decisions"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn branch_level_file_with_decisions_roundtrips() {
        let fm = FileMeasurement {
            path: "b.rs".into(),
            lines: LineCoverage {
                covered: 10,
                total: 12,
            },
            decisions: vec![DecisionCoverage {
                id: "b.rs:5:8".into(),
                covered: true,
                kind: "branch".into(),
            }],
            conditions: vec![],
        };
        let json = serde_json::to_string(&fm).unwrap();
        let parsed: FileMeasurement = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, fm);
    }
}
